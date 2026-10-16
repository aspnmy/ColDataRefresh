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
