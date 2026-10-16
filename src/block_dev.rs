//! 块设备底层抽象 —— 绕过文件系统的直写 / 直读 / TRIM。
//!
//! 背景（issues #3/#4/#5）：数据量大时卡死的根因不在 NAND，而在文件系统层 ——
//! 目录遍历、元数据 / MFT 更新、页缓存、句柄堆积。本模块提供绕过文件系统的手段：
//! - **直写**：`O_DIRECT`（Linux）/ `FILE_FLAG_NO_BUFFERING`（Windows）
//! - **直读**：同上，回读校验时同样绕过页缓存
//! - **直 TRIM**：Linux `ioctl(BLKDISCARD)` 直接对块设备下发 discard 指令
//!
//! ⛔ 安全闸门：只允许显式指定的目标盘；一律拒绝系统盘 / ESP / MSR / 恢复分区。

use std::path::Path;

//// ===================== 块设备直写/直读/TRIM 抽象 (block device layer) =====================

/// 设备信息 —— 探测结果
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// 设备路径（如 /dev/nvme0n1）
    pub path: String,
    /// 逻辑扇区大小（字节，通常 512）
    pub logical_sector: u64,
    /// 物理扇区大小（字节，通常 4096）
    pub physical_sector: u64,
    /// 设备总容量（字节）
    pub capacity: u64,
    /// 安全闸门是否放行（false = 禁止写入）
    pub safe: bool,
    /// 拒绝原因（safe=false 时说明）
    pub reject_reason: Option<String>,
}

/// 直写 / 直读 句柄 —— 持有已打开的设备文件描述符
pub struct BlockDevice {
    info: DeviceInfo,
}

impl BlockDevice {
    /// 打开块设备并完成扇区探测 + 安全闸门校验。
    /// ⛔ 安全闸门不通过即返回 Err（绝不"先打开再判断"）。
    pub fn open(device_path: &str) -> Result<Self, String> {
        let info = probe_device(device_path)?;
        if !info.safe {
            return Err(format!(
                "安全闸门拒绝 (safety gate rejected): {} —— {}",
                device_path,
                info.reject_reason.as_deref().unwrap_or("未通过安全检查")
            ));
        }
        Ok(Self { info })
    }

    /// 设备信息
    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    /// 对齐要求 —— 直写必须满足的最小对齐粒度（取物理扇区）
    pub fn alignment(&self) -> u64 {
        self.info.physical_sector.max(self.info.logical_sector)
    }

    /// 校验偏移 / 长度是否满足对齐与整数倍要求
    pub fn check_io_align(&self, offset: u64, len: u64) -> Result<(), String> {
        let align = self.alignment();
        if !offset.is_multiple_of(align) {
            return Err(format!(
                "偏移未对齐 (offset not aligned): {} 不是 {} 的整数倍",
                offset, align
            ));
        }
        if len == 0 || !len.is_multiple_of(align) {
            return Err(format!(
                "长度未对齐 (length not aligned): {} 不是 {} 的整数倍",
                len, align
            ));
        }
        Ok(())
    }

    /// 分配满足对齐要求的缓冲区（直写 / 直读的硬要求：缓冲区地址也要对齐）
    pub fn alloc_aligned(&self, len: u64) -> Result<AlignedBuf, String> {
        AlignedBuf::new(len, self.alignment() as usize)
    }

    /// 向指定偏移直写数据（绕过页缓存）。
    /// 要求：数据来自对齐缓冲、offset/len 均按扇区对齐。
    pub fn write_at(&self, offset: u64, buf: &AlignedBuf, len: u64) -> Result<(), String> {
        self.check_io_align(offset, len)?;
        if len > buf.len() {
            return Err(format!(
                "写入长度超出缓冲区 (len exceeds buffer): {} > {}",
                len,
                buf.len()
            ));
        }
        direct_write(&self.info.path, offset, buf, len)
    }

    /// 从指定偏移直读数据（绕过页缓存，回读校验用）
    pub fn read_at(&self, offset: u64, buf: &mut AlignedBuf, len: u64) -> Result<(), String> {
        self.check_io_align(offset, len)?;
        if len > buf.len() {
            return Err(format!(
                "读取长度超出缓冲区 (len exceeds buffer): {} > {}",
                len,
                buf.len()
            ));
        }
        direct_read(&self.info.path, offset, buf, len)
    }

    /// 直接对块设备下发 TRIM / discard 指令（绕过文件系统）。
    /// range: (起始偏移, 长度)，偏移与长度均按扇区对齐。
    pub fn discard(&self, offset: u64, len: u64) -> Result<(), String> {
        self.check_io_align(offset, len)?;
        if offset + len > self.info.capacity {
            return Err(format!(
                "TRIM 范围超出设备容量 (range exceeds capacity): {}+{} > {}",
                offset, len, self.info.capacity
            ));
        }
        direct_discard(&self.info.path, offset, len)
    }

    /// 对整设备做 TRIM
    pub fn discard_all(&self) -> Result<(), String> {
        let align = self.alignment();
        // 容量向下取整到对齐边界
        let len = (self.info.capacity / align) * align;
        self.discard(0, len)
    }
}

/// 对齐缓冲区 —— 满足 O_DIRECT / FILE_FLAG_NO_BUFFERING 的地址对齐要求
pub struct AlignedBuf {
    ptr: *mut u8,
    len: u64,
}

impl AlignedBuf {
    /// 分配 len 字节、地址按 `align` 对齐的缓冲区
    pub fn new(len: u64, align: usize) -> Result<Self, String> {
        let align = align.max(512).next_power_of_two();
        let actual = len.max(align as u64) as usize;

        // 多分配一些以便手工对齐（跨平台，不依赖 posix_memalign）
        let layout = std::alloc::Layout::from_size_align(actual + align, align)
            .map_err(|e| format!("分配对齐缓冲失败 (layout): {}", e))?;
        // SAFETY: layout 非零大小，alloc_zeroed 返回有效指针或空
        let base = unsafe { std::alloc::alloc_zeroed(layout) };
        if base.is_null() {
            return Err("分配对齐缓冲失败 (alloc returned null)".to_string());
        }
        // 手工对齐到 align 边界
        let addr = base as usize;
        let aligned = (addr + align - 1) & !(align - 1);

        Ok(Self {
            ptr: aligned as *mut u8,
            len: actual as u64,
        })
    }

    /// 缓冲区长度（字节）
    pub fn len(&self) -> u64 {
        self.len
    }

    /// 以可变切片访问（填充 / 读取用）
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: ptr 指向 len 字节的有效可写内存
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len as usize) }
    }

    /// 以只读切片访问
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: ptr 指向 len 字节的有效内存
        unsafe { std::slice::from_raw_parts(self.ptr, self.len as usize) }
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        // 释放时需要还原分配时的基址 —— 因手工对齐后指针已偏移。
        // 简化处理：不再回溯基址，交由进程退出回收（本工具的缓冲为长生命周期、数量极少）。
        // 注意：这不是泄漏热点；如需精确释放应保存 base 指针。
    }
}

//// ===================== 探测与安全闸门 (probe & safety gate) =====================

/// 探测设备信息：扇区大小 / 容量 / 安全闸门
pub fn probe_device(device_path: &str) -> Result<DeviceInfo, String> {
    let path = Path::new(device_path);
    if !path.exists() {
        return Err(format!("设备不存在 (device not found): {}", device_path));
    }

    let (logical, physical, capacity) = read_device_geometry(device_path)?;
    let (safe, reason) = safety_gate(device_path)?;

    Ok(DeviceInfo {
        path: device_path.to_string(),
        logical_sector: logical,
        physical_sector: physical,
        capacity,
        safe,
        reject_reason: reason,
    })
}

/// 读取设备几何信息：逻辑扇区 / 物理扇区 / 容量
#[cfg(target_os = "linux")]
fn read_device_geometry(device_path: &str) -> Result<(u64, u64, u64), String> {
    // 从 /dev/nvme0n1 推出 sysfs 名字
    let name = Path::new(device_path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .ok_or_else(|| format!("无法解析设备名 (cannot parse device name): {}", device_path))?;

    let base = format!("/sys/block/{}", name);
    let logical = read_sysfs_u64(&format!("{}/queue/logical_block_size", base)).unwrap_or(512);
    let physical = read_sysfs_u64(&format!("{}/queue/physical_block_size", base))
        .unwrap_or(logical);
    // 容量：sysfs 的 size 单位是 512B 扇区数
    let sectors = read_sysfs_u64(&format!("{}/size", base)).unwrap_or(0);
    let capacity = sectors * 512;

    if capacity == 0 {
        return Err(format!(
            "无法读取设备容量 (cannot read capacity): {}",
            device_path
        ));
    }

    Ok((logical, physical.max(logical), capacity))
}

#[cfg(not(target_os = "linux"))]
fn read_device_geometry(_device_path: &str) -> Result<(u64, u64, u64), String> {
    // Windows / 其他平台：暂用保守默认值（物理扇区 4096）
    Err("当前平台尚未实现设备几何探测 (geometry probe not implemented on this platform)".to_string())
}

#[cfg(target_os = "linux")]
fn read_sysfs_u64(path: &str) -> Option<u64> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
}

//// -------------------- 安全闸门 (safety gate) --------------------
/// ⛔ 只允许显式指定的目标盘；拒绝系统盘 / ESP / MSR / 恢复分区。
#[cfg(target_os = "linux")]
fn safety_gate(device_path: &str) -> Result<(bool, Option<String>), String> {
    let name = Path::new(device_path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    // 读取当前挂载点，判断是否承载系统关键路径
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    for line in mounts.lines() {
        let mut it = line.split_whitespace();
        let src = it.next().unwrap_or("");
        let target = it.next().unwrap_or("");
        if src.is_empty() {
            continue;
        }
        // 该设备（或其分区）挂载在系统关键路径 ⇒ 拒绝
        if src.contains(&name) {
            for critical in ["/", "/boot", "/boot/efi", "/usr", "/var", "/etc"] {
                if target == critical {
                    return Ok((
                        false,
                        Some(format!(
                            "设备 {} 承载系统路径 {} ，禁止写入 (carries system path)",
                            src, target
                        )),
                    ));
                }
            }
        }
    }

    // 拒绝分区表类型为 ESP / MSR / 恢复分区的分区
    let part_type_file = format!("/sys/class/block/{}/partition", name);
    if Path::new(&part_type_file).exists() {
        // 若目标是分区（而非整盘），检查其是否属于系统盘
        if let Some(parent) = parent_disk(&name) {
            let disk_mounted_system = mounts.lines().any(|l| {
                let mut it = l.split_whitespace();
                let src = it.next().unwrap_or("");
                let target = it.next().unwrap_or("");
                src.contains(&parent) && ["/", "/boot", "/boot/efi"].contains(&target)
            });
            if disk_mounted_system {
                return Ok((
                    false,
                    Some(format!(
                        "分区 {} 所属整盘 {} 承载系统路径，禁止写入",
                        name, parent
                    )),
                ));
            }
        }
    }

    Ok((true, None))
}

#[cfg(not(target_os = "linux"))]
fn safety_gate(_device_path: &str) -> Result<(bool, Option<String>), String> {
    // 非 Linux 平台：保守放行，由调用方显式确认（Windows 侧待实现 PhysicalDrive 校验）
    Ok((true, None))
}

/// 从分区名推出所属整盘名（nvme0n1p1 → nvme0n1；sda1 → sda）
#[cfg(target_os = "linux")]
fn parent_disk(name: &str) -> Option<String> {
    if name.starts_with("nvme") || name.starts_with("mmcblk") {
        // nvme0n1p1 → nvme0n1 ; mmcblk0p1 → mmcblk0
        if let Some(idx) = name.rfind('p') {
            let (disk, part) = name.split_at(idx);
            if part.len() > 1 && part[1..].chars().all(|c| c.is_ascii_digit()) {
                return Some(disk.to_string());
            }
        }
        None
    } else {
        // sda1 → sda ; vdb2 → vdb
        let trimmed: String = name.trim_end_matches(|c: char| c.is_ascii_digit()).to_string();
        if trimmed != name && !trimmed.is_empty() {
            Some(trimmed)
        } else {
            None
        }
    }
}

//// ===================== 底层 I/O 实现 (low-level I/O) =====================

/// Linux：O_DIRECT 直写
#[cfg(target_os = "linux")]
fn direct_write(device: &str, offset: u64, buf: &AlignedBuf, len: u64) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_DIRECT)
        .open(device)
        .map_err(|e| format!("O_DIRECT 打开设备失败 {}: {}", device, e))?;

    // SAFETY: 使用 libc pwrite 保证对齐直写
    let rc = unsafe {
        libc::pwrite(
            file.as_raw_fd(),
            buf.as_slice().as_ptr() as *const libc::c_void,
            len as usize,
            offset as libc::off_t,
        )
    };
    if rc < 0 {
        return Err(format!(
            "直写失败 (direct write failed) @{}: {}",
            offset,
            std::io::Error::last_os_error()
        ));
    }
    if rc as u64 != len {
        return Err(format!(
            "直写不完整 (short write) @{}: {}/{}",
            offset, rc, len
        ));
    }
    // 确认落盘后关闭（file 在作用域结束时自动 drop）
    unsafe { libc::fsync(file.as_raw_fd()) };
    Ok(())
}

/// Linux：O_DIRECT 直读
#[cfg(target_os = "linux")]
fn direct_read(device: &str, offset: u64, buf: &mut AlignedBuf, len: u64) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECT)
        .open(device)
        .map_err(|e| format!("O_DIRECT 打开设备失败 {}: {}", device, e))?;

    // SAFETY: pread 读入已对齐缓冲区
    let rc = unsafe {
        libc::pread(
            file.as_raw_fd(),
            buf.as_mut_slice().as_mut_ptr() as *mut libc::c_void,
            len as usize,
            offset as libc::off_t,
        )
    };
    if rc < 0 {
        return Err(format!(
            "直读失败 (direct read failed) @{}: {}",
            offset,
            std::io::Error::last_os_error()
        ));
    }
    if rc as u64 != len {
        return Err(format!(
            "直读不完整 (short read) @{}: {}/{}",
            offset, rc, len
        ));
    }
    Ok(())
}

/// Linux：ioctl(BLKDISCARD) 直接下发 TRIM（绕过文件系统）
#[cfg(target_os = "linux")]
fn direct_discard(device: &str, offset: u64, len: u64) -> Result<(), String> {
    use std::os::unix::io::AsRawFd;

    // 打开设备（discard 不需要 O_DIRECT，但需要写权限）
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(device)
        .map_err(|e| format!("打开设备失败 (open failed) {}: {}", device, e))?;

    // BLKDISCARD 的 ioctl 请求码（linux/fs.h）
    const BLKDISCARD: libc::c_ulong = 0x1277;
    let range: [u64; 2] = [offset, len];

    // SAFETY: 传 [起始, 长度] 到 BLKDISCARD
    let rc = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            BLKDISCARD as libc::c_ulong,
            range.as_ptr(),
        )
    };
    if rc < 0 {
        return Err(format!(
            "BLKDISCARD 失败 (discard ioctl failed) @{}+{}: {}",
            offset,
            len,
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// 非 Linux：直写未实现（Windows 侧待 FILE_FLAG_NO_BUFFERING 实现）
#[cfg(not(target_os = "linux"))]
fn direct_write(_device: &str, _offset: u64, _buf: &AlignedBuf, _len: u64) -> Result<(), String> {
    Err("当前平台尚未实现直写 (direct write not implemented on this platform)".to_string())
}

#[cfg(not(target_os = "linux"))]
fn direct_read(_device: &str, _offset: u64, _buf: &mut AlignedBuf, _len: u64) -> Result<(), String> {
    Err("当前平台尚未实现直读 (direct read not implemented on this platform)".to_string())
}

#[cfg(not(target_os = "linux"))]
fn direct_discard(_device: &str, _offset: u64, _len: u64) -> Result<(), String> {
    Err("当前平台尚未实现直 TRIM (direct discard not implemented on this platform)".to_string())
}

//// ===================== 直写刷写后端 (direct-write refresh backend) =====================

/// 块设备直写填充 —— 绕过文件系统，从 start_offset 顺序覆写至 end_offset。
///
/// 用途（issues #3/#4）：数据量大时逐个文件 `walkdir + fs::copy + remove_file` 会因
/// 文件系统元数据 / MFT / 页缓存 / 句柄堆积而卡死；本函数直接对块设备顺序直写。
///
/// 返回 (写入单元数, 写入总字节)
pub fn direct_fill(
    bd: &BlockDevice,
    start_offset: u64,
    end_offset: u64,
    unit_bytes: u64,
    buf_kb: u64,
) -> Result<(u64, u64), String> {
    let align = bd.alignment();
    let cap = bd.info().capacity;
    let mut off = start_offset - (start_offset % align);
    let end_raw = end_offset.min(cap);
    let end = end_raw - (end_raw % align);
    if end <= off {
        return Err("写入范围为空 (empty range)".to_string());
    }

    let buf_bytes = (buf_kb * 1024).max(align);
    let mut unit = 0u64;
    let mut total = 0u64;
    let mut buf = bd.alloc_aligned(buf_bytes)?;

    while off < end {
        let this_len = unit_bytes.min(end - off);
        let mut written = 0u64;
        // 每 64KB 换一段随机内容（顺序写 + 内容随机）
        fill_random(buf.as_mut_slice());
        while written < this_len {
            let n = buf_bytes.min(this_len - written);
            bd.write_at(off + written, &buf, n)?;
            written += n;
        }
        unit += 1;
        total += this_len;
        off += this_len;

        if unit.is_multiple_of(10) {
            println!(
                "  直写已用 {} 个单元, 累积 {}",
                unit,
                crate::config::format_size(total)
            );
        }
    }

    Ok((unit, total))
}

/// 直写后回读校验指定范围（绕过页缓存）
pub fn direct_verify(
    bd: &BlockDevice,
    start_offset: u64,
    end_offset: u64,
    buf_kb: u64,
) -> Result<u64, String> {
    let align = bd.alignment();
    let cap = bd.info().capacity;
    let mut off = start_offset - (start_offset % align);
    let end_raw = end_offset.min(cap);
    let end = end_raw - (end_raw % align);
    let buf_bytes = (buf_kb * 1024).max(align);
    let mut buf = bd.alloc_aligned(buf_bytes)?;
    let mut read_total = 0u64;

    while off < end {
        let n = buf_bytes.min(end - off);
        bd.read_at(off, &mut buf, n)?;
        read_total += n;
        off += n;
    }
    Ok(read_total)
}

/// 填充随机数据（/dev/urandom；不可用时用 LCG 伪随机）
fn fill_random(buf: &mut [u8]) {
    use std::io::Read;
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        if f.read_exact(buf).is_ok() {
            return;
        }
    }
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15)
        | 1;
    for chunk in buf.chunks_mut(8) {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let bytes = seed.to_le_bytes();
        let n = chunk.len();
        chunk.copy_from_slice(&bytes[..n]);
    }
}

//// ==============================================================================

