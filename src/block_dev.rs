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
///
/// ⚠️ 采用「自持所有权」设计：内部持 `Vec<u8>`（由分配器管理首地址），
/// 使用前用 `align_offset` 手工对齐访问窗口 ⇒ **随 `Vec` 析构自动归还内存**，
/// 无需手写 `dealloc`（issue #10 第 4 条：运行时能自行 GC 的要 GC 掉，防 Win 假死）。
pub struct AlignedBuf {
    /// 实际持有内存的容器（drop 时自动释放）
    buf: Vec<u8>,
    /// 对齐后的起始偏移（buf 内的整数偏移，非指针）
    offset: usize,
    /// 有效长度（字节）
    len: u64,
}

impl AlignedBuf {
    /// 分配 len 字节、起始地址按 `align` 对齐的缓冲区
    pub fn new(len: u64, align: usize) -> Result<Self, String> {
        let align = align.max(512).next_power_of_two();
        let actual = len.max(align as u64) as usize;

        // 多分配 align 字节，保证内部总能找到对齐起点
        let total = actual
            .checked_add(align)
            .ok_or_else(|| format!("缓冲区尺寸溢出 (size overflow): {}+{}", actual, align))?;

        let buf = vec![0u8; total];
        // 手工对齐：把首地址上取整到 align 边界
        let base = buf.as_ptr() as usize;
        let aligned = (base + align - 1) & !(align - 1);
        let offset = aligned - base;

        Ok(Self {
            buf,
            offset,
            len: actual as u64,
        })
    }

    /// 对齐后的起始指针 —— 供 Linux 直写直读使用（其他平台无块设备实现）
    #[cfg(target_os = "linux")]
    fn as_ptr(&self) -> *const u8 {
        // SAFETY: offset < buf.len()（构造时保证 offset <= align < total）
        unsafe { self.buf.as_ptr().add(self.offset) }
    }

    /// 缓冲区长度（字节）
    pub fn len(&self) -> u64 {
        self.len
    }

    /// 以可变切片访问（填充 / 读取用）
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        let off = self.offset;
        let n = self.len as usize;
        // SAFETY: off + n <= buf.len()（构造时 total = actual + align 且 off <= align）
        unsafe { std::slice::from_raw_parts_mut(self.buf.as_mut_ptr().add(off), n) }
    }

    /// 以只读切片访问 —— 供 Linux 直写使用
    #[cfg(target_os = "linux")]
    pub fn as_slice(&self) -> &[u8] {
        let n = self.len as usize;
        // SAFETY: 同上，窗口落在 buf 容量之内
        unsafe { std::slice::from_raw_parts(self.as_ptr(), n) }
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
///
/// 平台专有实现见 `platform/{linux,windows,darwin}.rs`（issue #7：平台代码只进对应分册）。
fn read_device_geometry(device_path: &str) -> Result<(u64, u64, u64), String> {
    crate::platform::block_geometry(device_path)
}

//// -------------------- 安全闸门 (safety gate) --------------------
/// ⛔ 只允许显式指定的目标盘；拒绝系统盘 / ESP / MSR / 恢复分区。
///
/// 平台专有实现见 `platform/{linux,windows,darwin}.rs`。
fn safety_gate(device_path: &str) -> Result<(bool, Option<String>), String> {
    crate::platform::block_safety_gate(device_path)
}

//// ===================== 底层 I/O 实现 (low-level I/O) =====================

/// 直写 —— 平台专有实现派发（Linux `O_DIRECT`+`pwrite`；其他平台见各自分册）
fn direct_write(device: &str, offset: u64, buf: &AlignedBuf, len: u64) -> Result<(), String> {
    crate::platform::block_direct_write(device, offset, buf, len)
}

/// 直读 —— 平台专有实现派发
fn direct_read(device: &str, offset: u64, buf: &mut AlignedBuf, len: u64) -> Result<(), String> {
    crate::platform::block_direct_read(device, offset, buf, len)
}

/// 直 TRIM —— 平台专有实现派发（Linux `ioctl(BLKDISCARD)`）
fn direct_discard(device: &str, offset: u64, len: u64) -> Result<(), String> {
    crate::platform::block_direct_discard(device, offset, len)
}

//// ===================== 范围分块迭代 (aligned range chunks) =====================

/// 对齐后的 I/O 范围 —— 起止均已按扇区取整，保证落在设备容量内。
///
/// 抽出本类型的目的（issue #10 第 2 条「可抽象的尽量抽象」）：
/// `direct_fill` / `direct_verify` 原先各自重复一遍「起止对齐 + 容量截断 + 空范围判定」，
/// 现统一由 `AlignedRange::new` 负责，调用方只关心遍历。
#[derive(Debug, Clone, Copy)]
pub struct AlignedRange {
    /// 对齐后的起始偏移
    pub start: u64,
    /// 对齐后的结束偏移（开区间）
    pub end: u64,
}

impl AlignedRange {
    /// 构造对齐范围：起点下取整、终点按容量截断后下取整。
    /// 范围为空（`end <= start`）时返回 `None`。
    pub fn new(start_offset: u64, end_offset: u64, align: u64, capacity: u64) -> Option<Self> {
        let align = align.max(1);
        let start = start_offset - (start_offset % align);
        let capped = end_offset.min(capacity);
        let end = capped - (capped % align);
        (end > start).then_some(Self { start, end })
    }

    /// 总长度（字节）
    pub fn len(&self) -> u64 {
        self.end - self.start
    }

    /// 按 `unit_bytes` 切成单元区间（最后一块取剩余量）。
    /// 单元大小下限 = 1，避免除零/死循环。
    pub fn units(&self, unit_bytes: u64) -> impl Iterator<Item = (u64, u64)> + '_ {
        let unit = unit_bytes.max(1);
        let start = self.start;
        let end = self.end;
        std::iter::successors(Some(start), move |&off| {
            let next = off + unit.min(end - off);
            (next < end).then_some(next)
        })
        .map(move |off| (off, unit.min(end - off)))
    }
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
    let range = AlignedRange::new(start_offset, end_offset, align, bd.info().capacity)
        .ok_or_else(|| "写入范围为空 (empty range)".to_string())?;

    let buf_bytes = (buf_kb * 1024).max(align);
    let mut buf = bd.alloc_aligned(buf_bytes)?;
    let mut unit = 0u64;
    let mut total = 0u64;

    for (off, this_len) in range.units(unit_bytes) {
        // 每单元换一段随机内容（顺序写 + 内容随机）
        fill_random(buf.as_mut_slice());
        // 单元内再按缓冲区切分，直到写完 this_len
        let mut written = 0u64;
        while written < this_len {
            let n = buf_bytes.min(this_len - written);
            bd.write_at(off + written, &buf, n)?;
            written += n;
        }
        unit += 1;
        total += this_len;

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
    let range = AlignedRange::new(start_offset, end_offset, align, bd.info().capacity)
        .ok_or_else(|| "校验范围为空 (empty range)".to_string())?;

    let buf_bytes = (buf_kb * 1024).max(align);
    let mut buf = bd.alloc_aligned(buf_bytes)?;
    let mut read_total = 0u64;

    // 以「整段」为单位切分（unit_bytes = 剩余全部 ⇒ 退化为缓冲区粒度遍历）
    for (off, chunk_len) in range.units(range.len()) {
        let mut done = 0u64;
        while done < chunk_len {
            let n = buf_bytes.min(chunk_len - done);
            bd.read_at(off + done, &mut buf, n)?;
            done += n;
            read_total += n;
        }
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

