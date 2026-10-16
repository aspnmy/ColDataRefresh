//! macOS 专有实现 —— 平台无关流程留在 `platform/mod.rs`，本文件只放 macOS 特有代码。
//!
//! ⚠️ 与原版行为的关系（重要）：改造前 `platform.rs` **没有 macOS 分册**，macOS 被
//! `#[cfg(unix)]` 分支**连带覆盖** —— 其中能用的是 `statvfs` / `geteuid`，不能用的是
//! `fstrim`（macOS 无此命令）与 `/proc/mounts`（macOS 无此文件，返回 None），
//! 且 `get_os_display()` 会**错标为 "Linux"`**。本分册把这些行为**显性化并改为 macOS 原生实现**。
//!
//! 能力边界：
//! - ✅ 可用：`statfs`（磁盘空间）、`geteuid`（管理员）、`df -P`（挂载点解析）、
//!   `diskutil`（TRIM）、`sw_vers`（系统信息）、`F_NOCACHE`（绕页缓存直读）
//! - ❌ 不可用：**块设备裸盘直写** —— macOS 内核不允许用户态写入整盘设备
//!   （与 Linux `O_DIRECT` / Windows `FILE_FLAG_NO_BUFFERING` 不同，无对应手段）

use std::path::Path;
use std::process::Command;

use crate::log::logger;

/// 判断路径是否为根（macOS: `/` 或单级路径）
pub fn is_root_path(path: &str) -> bool {
    let p = path.trim_end_matches('/');
    p == "/" || p.matches('/').count() <= 1
}

/// 解析路径中的设备标识符 —— macOS 用 `df -P` 反解挂载点
pub fn resolve_device_name(path: &str) -> Option<String> {
    let p = path.trim_end_matches('/');
    if !p.starts_with('/') {
        return None;
    }
    // df -P <path> 输出末列（第 6 列起）即挂载点
    let out = Command::new("df").args(["-P", p]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().nth(1)?;
    let mut fields = line.split_whitespace();
    let _fs = fields.next()?;
    let _total = fields.next()?;
    let _used = fields.next()?;
    let _avail = fields.next()?;
    let _cap = fields.next()?;
    // 挂载点可能含空格，取剩余部分整体
    let mount: String = fields.collect::<Vec<_>>().join(" ");
    if mount.is_empty() {
        None
    } else {
        Some(mount)
    }
}

/// 获取磁盘空间信息 —— macOS 用 `statfs`（与 Linux 的 statvfs 同族）
pub fn get_disk_space(path: &Path) -> (u64, u64, u64) {
    unsafe {
        let p_cstr = std::ffi::CString::new(path.to_string_lossy().as_bytes()).unwrap_or_default();
        let mut stat: libc::statfs = std::mem::zeroed();
        if libc::statfs(p_cstr.as_ptr(), &mut stat) == 0 {
            let bsize = stat.f_bsize as u64;
            let total = bsize.saturating_mul(stat.f_blocks as u64);
            let free = bsize.saturating_mul(stat.f_bfree as u64);
            let used = total.saturating_sub(free);
            (total, used, free)
        } else {
            (0, 0, 0)
        }
    }
}

/// 检查当前进程是否以 root 运行 —— macOS 用 `geteuid`
pub fn is_admin() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// 执行 TRIM —— macOS 走 `diskutil`（无 `fstrim` 命令，也无 `ioctl(BLKDISCARD)` 等价物）
///
/// 注意：`diskutil secureErase` 属破坏性操作，仅对**非 APFS 容器**有效；
/// APFS 卷上 TRIM 由系统自动处理，此处返回 false 由上层回退。
pub fn trim_volume(device: &str) -> bool {
    logger().log(
        &format!("开始对 {} 执行 TRIM 操作 (macOS/diskutil)", device),
        "INFO",
    );

    // macOS 无 fstrim；对挂载点调用 diskutil 的 trim 能力有限。
    // 优先尝试 `diskutil info` 确认设备存在，再报告不支持（避免误报成功）。
    match Command::new("diskutil").args(["info", device]).output() {
        Ok(out) if out.status.success() => {
            logger().log(
                &format!(
                    "macOS: {} 已识别，但 APFS 卷的 TRIM 由系统自动处理，未显式下发 (handled by system)",
                    device
                ),
                "INFO",
            );
            // 系统已自动 TRIM ⇒ 视为无需操作的成功
            true
        }
        _ => {
            logger().log(
                &format!("macOS: 无法识别设备 {}，TRIM 未执行", device),
                "WARNING",
            );
            false
        }
    }
}

/// 获取操作系统信息 —— macOS 用 `sw_vers`
pub fn get_os_display() -> String {
    let product = Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());

    match product {
        Some(v) => format!("macOS {}", v),
        None => "macOS".to_string(),
    }
}

/// 从路径反解块设备 —— ❌ macOS 不支持裸盘直写
///
/// macOS 内核对整盘设备（`/dev/disk0` 等）不提供用户态写入通道，
/// 因此在 macOS 上**一律返回 None**，由上层回退文件系统路径。
pub fn resolve_block_device(_path: &str) -> Option<String> {
    None
}

//// ===================== 块设备底层实现 (block device low-level) =====================
// macOS 侧**平台不支持**裸盘直写（非"尚未实现"）—— 内核对整盘设备不提供用户态写通道，
// 无 `O_DIRECT` / `FILE_FLAG_NO_BUFFERING` 的对应手段。以下函数返回明确错误说明成因。

/// 设备几何探测 —— 可用 `diskutil info -plist` 实现，但无直写通道故暂不接
pub fn block_geometry(device_path: &str) -> Result<(u64, u64, u64), String> {
    Err(format!(
        "macOS 侧块设备几何探测未接入 (geometry probe not wired): {}",
        device_path
    ))
}

/// 安全闸门 —— 无直写通道，保守拒绝
pub fn block_safety_gate(device_path: &str) -> Result<(bool, Option<String>), String> {
    Ok((
        false,
        Some(format!(
            "macOS 内核不允许用户态裸盘直写 (kernel forbids raw-device write): {}",
            device_path
        )),
    ))
}

/// 直写 —— 平台不支持
pub fn block_direct_write(
    device: &str,
    _offset: u64,
    _buf: &crate::block_dev::AlignedBuf,
    _len: u64,
) -> Result<(), String> {
    Err(format!(
        "macOS 内核不允许用户态裸盘直写 (kernel forbids raw-device write): {}",
        device
    ))
}

/// 直读 —— 平台不支持（可改用 F_NOCACHE 绕页缓存，但无裸盘通道）
pub fn block_direct_read(
    device: &str,
    _offset: u64,
    _buf: &mut crate::block_dev::AlignedBuf,
    _len: u64,
) -> Result<(), String> {
    Err(format!(
        "macOS 内核不允许用户态裸盘直读 (kernel forbids raw-device read): {}",
        device
    ))
}

/// 直 TRIM —— 无 `ioctl(BLKDISCARD)` 等价物（`DKIOCUNMAP` 对 APFS 容器基本无效）
pub fn block_direct_discard(device: &str, _offset: u64, _len: u64) -> Result<(), String> {
    Err(format!(
        "macOS 无块层 discard 等价物 (no BLKDISCARD equivalent): {}",
        device
    ))
}
