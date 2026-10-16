//! Linux 专有实现 —— 平台无关流程留在 `platform/mod.rs`，本文件只放 Linux 特有代码。
//!
//! 覆盖：磁盘空间（`statvfs`）、设备解析（`/proc/mounts`）、TRIM（`fstrim`）、
//! 管理员检测（`geteuid`）、系统信息（`/etc/os-release`）、块设备反解（`findmnt`）。

use std::path::Path;
use std::process::Command;

use crate::log::logger;

/// 判断路径是否为盘符根（Linux: `/` 或单级路径）
pub fn is_root_path(path: &str) -> bool {
    let p = path.trim_end_matches(&['\\', '/', ' ', '\t'][..]);
    p == "/" || p.matches('/').count() <= 1
}

/// 解析路径中的设备标识符 —— Linux 返回最长前缀匹配的挂载点
pub fn resolve_device_name(path: &str) -> Option<String> {
    let p = path.trim_end_matches(&['\\', '/', ' ', '\t'][..]);
    if p.starts_with('/') {
        resolve_best_mount_point(p)
    } else {
        None
    }
}

/// 获取磁盘空间信息 —— 返回 (总字节, 已用字节, 可用字节)
pub fn get_disk_space(path: &Path) -> (u64, u64, u64) {
    unsafe {
        let p_cstr = std::ffi::CString::new(path.to_string_lossy().as_bytes()).unwrap_or_default();
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(p_cstr.as_ptr(), &mut stat) == 0 {
            let total = (stat.f_frsize as u64).saturating_mul(stat.f_blocks as u64);
            let free = (stat.f_frsize as u64).saturating_mul(stat.f_bfree as u64);
            let used = total.saturating_sub(free);
            (total, used, free)
        } else {
            (0, 0, 0)
        }
    }
}

/// 检查当前进程是否以 root 运行
pub fn is_admin() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// 执行 TRIM —— Linux 走 `fstrim`
pub fn trim_volume(device: &str) -> bool {
    logger().log(&format!("开始对 {} 执行 TRIM 操作", device), "INFO");
    match Command::new("fstrim").arg(device).output() {
        Ok(output) => {
            if output.status.success() {
                let msg = String::from_utf8_lossy(&output.stdout);
                logger().log(&format!("fstrim 成功: {}", msg.trim()), "INFO");
                true
            } else {
                let err = String::from_utf8_lossy(&output.stderr);
                logger().log(&format!("fstrim 失败: {}", err.trim()), "WARNING");
                false
            }
        }
        Err(e) => {
            logger().log(&format!("执行 fstrim 失败: {}", e), "ERROR");
            false
        }
    }
}

/// 获取操作系统信息 —— 读 `/etc/os-release` 的 `PRETTY_NAME`
pub fn get_os_display() -> String {
    if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            if let Some(val) = line.strip_prefix("PRETTY_NAME=\"") {
                return val.trim_end_matches('"').to_string();
            } else if let Some(val) = line.strip_prefix("PRETTY_NAME=") {
                return val.to_string();
            }
        }
    }
    "Linux".to_string()
}

/// 从挂载点反解出底层块设备路径（如 /home → /dev/nvme0n1p1）
pub fn resolve_block_device(path: &str) -> Option<String> {
    // 已是 /dev/... 形式：直接返回
    if path.starts_with("/dev/") {
        return Some(path.to_string());
    }

    // 用 findmnt 反查挂载该路径的源设备
    let out = Command::new("findmnt")
        .args(["-n", "-o", "SOURCE", "--target", path])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let src = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if src.is_empty() || !src.starts_with("/dev/") {
        return None;
    }
    Some(src)
}

/// 从 /proc/mounts 查找路径的最长前缀挂载点
pub fn resolve_best_mount_point(path: &str) -> Option<String> {
    let mounts = std::fs::read_to_string("/proc/mounts").ok()?;
    let mut best: Option<String> = None;

    for line in mounts.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 2 {
            continue;
        }
        // 解码转义序列（/proc/mounts 中使用 \040 表示空格等）
        let mount_point = decode_mount_point(fields[1]);

        if path.starts_with(&mount_point) {
            let should_replace = match best.as_ref() {
                None => true,
                Some(current) => mount_point.len() > current.len(),
            };
            if should_replace {
                best = Some(mount_point);
            }
        }
    }

    best
}

/// 解码 /proc/mounts 中的转义序列
fn decode_mount_point(encoded: &str) -> String {
    let mut result = String::new();
    let mut chars = encoded.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let digits: String = chars.by_ref().take(3).collect();
            if digits.len() == 3 {
                if let Ok(code) = u32::from_str_radix(&digits, 8) {
                    result.push(char::from_u32(code).unwrap_or('?'));
                    continue;
                }
            }
            result.push('\\');
            result.push_str(&digits);
        } else {
            result.push(c);
        }
    }
    result
}

//// ===================== 块设备底层实现 (block device low-level) =====================
// 以下为「块设备直写 / 直读 / 直 TRIM」的 Linux 专有实现（issue #7：平台代码只进分册）。
// 抽象层与对外类型在 `block_dev.rs`；本文件只保留落到内核的那一层。

/// 设备几何探测：逻辑扇区 / 物理扇区 / 容量（sysfs）
pub fn block_geometry(device_path: &str) -> Result<(u64, u64, u64), String> {
    let name = Path::new(device_path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .ok_or_else(|| format!("无法解析设备名 (cannot parse device name): {}", device_path))?;

    let base = format!("/sys/block/{}", name);
    let logical = read_sysfs_u64(&format!("{}/queue/logical_block_size", base)).unwrap_or(512);
    let physical =
        read_sysfs_u64(&format!("{}/queue/physical_block_size", base)).unwrap_or(logical);
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

fn read_sysfs_u64(path: &str) -> Option<u64> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
}

/// 安全闸门：拒绝承载系统路径（/ /boot /boot/efi /usr /var /etc）的设备及分区
pub fn block_safety_gate(device_path: &str) -> Result<(bool, Option<String>), String> {
    let name = Path::new(device_path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    for line in mounts.lines() {
        let mut it = line.split_whitespace();
        let src = it.next().unwrap_or("");
        let target = it.next().unwrap_or("");
        if src.is_empty() {
            continue;
        }
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

    // 若目标是分区（而非整盘），检查其所属整盘是否承载系统
    let part_file = format!("/sys/class/block/{}/partition", name);
    if Path::new(&part_file).exists() {
        if let Some(parent) = parent_disk(&name) {
            let disk_on_system = mounts.lines().any(|l| {
                let mut it = l.split_whitespace();
                let src = it.next().unwrap_or("");
                let target = it.next().unwrap_or("");
                src.contains(&parent) && ["/", "/boot", "/boot/efi"].contains(&target)
            });
            if disk_on_system {
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

/// 从分区名推出所属整盘名（nvme0n1p1 → nvme0n1；sda1 → sda）
fn parent_disk(name: &str) -> Option<String> {
    if name.starts_with("nvme") || name.starts_with("mmcblk") {
        if let Some(idx) = name.rfind('p') {
            let (disk, part) = name.split_at(idx);
            if part.len() > 1 && part[1..].chars().all(|c| c.is_ascii_digit()) {
                return Some(disk.to_string());
            }
        }
        None
    } else {
        let trimmed = name.trim_end_matches(|c: char| c.is_ascii_digit());
        if trimmed != name && !trimmed.is_empty() {
            Some(trimmed.to_string())
        } else {
            None
        }
    }
}

/// 直写：`O_DIRECT` + `pwrite`（绕过页缓存），写后 `fsync` 确认落盘
pub fn block_direct_write(
    device: &str,
    offset: u64,
    buf: &crate::block_dev::AlignedBuf,
    len: u64,
) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_DIRECT)
        .open(device)
        .map_err(|e| format!("O_DIRECT 打开设备失败 {}: {}", device, e))?;

    // SAFETY: 传入已对齐缓冲区指针，长度与偏移均已按扇区对齐
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
    unsafe { libc::fsync(file.as_raw_fd()) };
    Ok(())
}

/// 直读：`O_DIRECT` + `pread`（绕过页缓存）
pub fn block_direct_read(
    device: &str,
    offset: u64,
    buf: &mut crate::block_dev::AlignedBuf,
    len: u64,
) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECT)
        .open(device)
        .map_err(|e| format!("O_DIRECT 打开设备失败 {}: {}", device, e))?;

    // SAFETY: 读入已对齐缓冲区
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

/// 直 TRIM：`ioctl(BLKDISCARD)` 直接对块设备下发 discard（绕过文件系统）
pub fn block_direct_discard(device: &str, offset: u64, len: u64) -> Result<(), String> {
    use std::os::unix::io::AsRawFd;

    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(device)
        .map_err(|e| format!("打开设备失败 (open failed) {}: {}", device, e))?;

    // BLKDISCARD 的 ioctl 请求码（linux/fs.h）
    const BLKDISCARD: libc::c_ulong = 0x1277;
    let range: [u64; 2] = [offset, len];

    // SAFETY: 传 [起始偏移, 长度]，两者均已按扇区对齐
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), BLKDISCARD, range.as_ptr()) };
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

//// ===================== 文件属性 (file attributes) =====================
// 平台专有的「文件属性操作」——原散在 `full_refresh.rs`，按 issue #7 移入分册。

/// 移除文件的只读属性 —— Linux 无该概念，空操作
pub fn clear_readonly(_path: &std::path::Path) {}

/// 保留文件的创建时间 —— Linux 无创建时间概念，空操作
pub fn preserve_creation_time(_path: &std::path::Path) {}
