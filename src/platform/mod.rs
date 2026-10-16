//! 平台抽象层入口 —— 跨平台能力的统一门面。
//!
//! 结构（issue #7）：平台无关的**公共流程**留在本文件；各平台的**专有实现**分册维护：
//! - `linux.rs`   —— Linux 专有实现（`O_DIRECT`、`ioctl(BLKDISCARD)`、sysfs 探测）
//! - `windows.rs` —— Windows 专有实现（`FILE_FLAG_NO_BUFFERING`、`Optimize-Volume`）
//! - `darwin.rs`  —— macOS 占位（暂未支持块设备直写）
//!
//! ⛔ 平台专有代码只进对应分册；公共流程（`app.rs` / `full_refresh.rs` / `file_op.rs`）
//! 只依赖本模块导出的抽象，不直接写平台代码。

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
mod darwin;

#[cfg(target_os = "linux")]
pub use linux::{
    get_disk_space, get_os_display, is_admin, is_root_path, resolve_block_device,
    resolve_device_name, trim_volume,
};

#[cfg(target_os = "linux")]
pub use linux::{
    block_direct_discard, block_direct_read, block_direct_write, block_geometry, block_safety_gate,
    clear_readonly, preserve_creation_time,
};

#[cfg(target_os = "windows")]
pub use windows::{
    get_disk_space, get_os_display, is_admin, is_root_path, resolve_block_device,
    resolve_device_name, trim_volume,
};

#[cfg(target_os = "windows")]
pub use windows::{
    block_direct_discard, block_direct_read, block_direct_write, block_geometry, block_safety_gate,
    clear_readonly, preserve_creation_time,
};

#[cfg(target_os = "macos")]
pub use darwin::{
    get_disk_space, get_os_display, is_admin, is_root_path, resolve_block_device,
    resolve_device_name, trim_volume,
};

#[cfg(target_os = "macos")]
pub use darwin::{
    block_direct_discard, block_direct_read, block_direct_write, block_geometry, block_safety_gate,
    clear_readonly, preserve_creation_time,
};

use crate::log::logger;

//// ===================== 采样后端能力探测 (trace backend capability) =====================

/// tracefs 是否可用（挂载点存在且可写）
pub fn tracefs_available() -> bool {
    std::path::Path::new("/sys/kernel/debug/tracing/events/block").exists()
}

/// perf 是否**真正**可运行（⛔ 只看 `which perf` 会误判：Debian 的 /usr/bin/perf
/// 只是包装脚本，`linux-perf-<ver>` 未装时 exec 会失败 —— 1-243 实测踩到）
pub fn perf_available() -> bool {
    use std::process::Command;
    Command::new("perf")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// bpftrace 是否可用（需工具 + BTF）
pub fn bpftrace_available() -> bool {
    use std::process::Command;
    let has_tool = Command::new("bpftrace")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let has_btf = std::path::Path::new("/sys/kernel/btf/vmlinux").exists();
    has_tool && has_btf
}

//// -------------------- tracefs 操作 (tracefs ops) --------------------
/// tracefs 根目录（内核 4.19 与 6.x 路径一致）
const TRACEFS: &str = "/sys/kernel/debug/tracing";

/// 重置 trace：关采样、清空 buffer、关全部事件
pub fn tracefs_reset() -> Result<(), String> {
    tracefs_write("tracing_on", "0")?;
    tracefs_write("trace", "")?;
    // 关全部事件（可能失败，不致命）
    let _ = tracefs_write("events/enable", "0");
    Ok(())
}

/// 开关 trace
pub fn tracefs_on(on: bool) -> Result<(), String> {
    tracefs_write("tracing_on", if on { "1" } else { "0" })
}

/// 启用 / 关闭指定事件（如 "block/block_rq_issue"）
pub fn tracefs_enable(event: &str, on: bool) -> Result<(), String> {
    tracefs_write(
        &format!("events/{}/enable", event),
        if on { "1" } else { "0" },
    )
}

/// 设置 ring buffer 大小（KB）
pub fn tracefs_set_buffer_kb(kb: u64) -> Result<(), String> {
    tracefs_write("buffer_size_kb", &kb.to_string())
}

/// 读取 trace 文本，并只保留目标设备的行
pub fn tracefs_read(device: &str) -> Result<String, String> {
    let path = format!("{}/trace", TRACEFS);
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取 {} 失败: {}", path, e))?;

    // 目标设备的 dev_t（trace 里形如 "259,0"）
    let Some(want) = device_dev_t(device) else {
        return Ok(raw);
    };

    let mut out = String::with_capacity(raw.len());
    for line in raw.lines() {
        if let Some(pos) = line.find("block_rq_issue:") {
            let rest = line[pos..].trim();
            // 第一个字段即 dev
            if let Some(dev) = rest.split_whitespace().nth(1) {
                if dev == want {
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
    }
    Ok(out)
}

/// 读取设备的 dev_t 字符串（major,minor —— 与 trace 输出格式一致）
fn device_dev_t(device: &str) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let md = std::fs::metadata(device).ok()?;
    let rdev = md.rdev();
    let major = (rdev >> 8) & 0xfff;
    let minor = (rdev & 0xff) | ((rdev >> 12) & 0xfff00);
    Some(format!("{},{}", major, minor))
}

/// 写 tracefs 文件（统一错误处理）
fn tracefs_write(name: &str, value: &str) -> Result<(), String> {
    use std::io::Write;
    let path = format!("{}/{}", TRACEFS, name);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .map_err(|e| format!("打开 {} 失败: {}", path, e))?;
    f.write_all(value.as_bytes())
        .map_err(|e| format!("写入 {} 失败: {}", path, e))?;
    Ok(())
}

//// ======================================================================================

/// 执行 TRIM 操作（绕过文件系统的块层直下，issue #5）
///
/// 优先走 `block_dev` 的 `ioctl(BLKDISCARD)` 直接对块设备下发 discard；
/// 失败（如设备不支持、权限不足）时回退到各平台的常规 TRIM 实现。
pub fn trim_volume_direct(device: &str) -> bool {
    logger().log(
        &format!("开始对 {} 执行底层直 TRIM (BLKDISCARD)...", device),
        "INFO",
    );

    // 尝试块设备直下：从挂载点反解出块设备，再走 block_dev
    let blk = match resolve_block_device(device) {
        Some(d) => d,
        None => {
            logger().log(
                &format!("无法从 {} 解析块设备，回退常规 TRIM (fallback)", device),
                "WARNING",
            );
            return trim_volume(device);
        }
    };

    match crate::block_dev::BlockDevice::open(&blk) {
        Ok(bd) => match bd.discard_all() {
            Ok(()) => {
                logger().log(
                    &format!("块层直 TRIM 成功 (BLKDISCARD ok): {}", blk),
                    "INFO",
                );
                true
            }
            Err(e) => {
                logger().log(
                    &format!("块层直 TRIM 失败: {}，回退常规 TRIM (fallback)", e),
                    "WARNING",
                );
                trim_volume(device)
            }
        },
        Err(e) => {
            logger().log(
                &format!("打开块设备失败: {}，回退常规 TRIM (fallback)", e),
                "WARNING",
            );
            trim_volume(device)
        }
    }
}
