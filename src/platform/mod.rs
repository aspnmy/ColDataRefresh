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

/// LBA 热度采样分册（issue #12）—— 各平台机制不同、**输出契约相同**：
/// Linux = tracefs / Windows = ETW / macOS = 明确不支持并写清成因。
pub mod heat_sample;

#[cfg(target_os = "linux")]
pub use linux::{
    get_disk_space, get_os_display, is_admin, is_root_path, resolve_block_device,
    resolve_device_name, trim_volume,
};

#[cfg(target_os = "linux")]
pub use linux::{
    block_direct_discard, block_direct_read, block_direct_write, block_geometry, block_safety_gate,
    clear_readonly, parent_disk, preserve_creation_time,
};

#[cfg(target_os = "windows")]
pub use windows::{
    get_disk_space, get_os_display, is_admin, is_root_path, resolve_block_device,
    resolve_device_name, trim_volume,
};

#[cfg(target_os = "windows")]
pub use windows::{
    block_direct_discard, block_direct_read, block_direct_write, block_geometry, block_safety_gate,
    clear_readonly, enter_background_io_mode, exit_background_io_mode, preserve_creation_time,
};

/// 切入「后台 IO 模式」—— Windows 下降低本线程磁盘 IO 优先级防整机假死；
/// 其它平台为无操作（Linux/macOS 用 ionice/cgroup 或本就无此问题，保持行为不变）。
#[cfg(not(target_os = "windows"))]
#[inline]
pub fn enter_background_io_mode() -> bool {
    true
}

/// 退出「后台 IO 模式」—— 非 Windows 平台为无操作。
#[cfg(not(target_os = "windows"))]
#[inline]
pub fn exit_background_io_mode() -> bool {
    true
}

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

//// -------------------- 工作目录（统一入口，issue #14） --------------------
// 平台无关：全盘刷新与跑圈校验共用的「工作目录」解析与准备。
// ⛔ 不再由各模式各写一遍拼路径（原为各自的 `$aspnmytools`，且各系统兜底值不同）。

/// 工作目录的子用途（同一 `--work-dir` 下按用途分目录）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkSubDir {
    /// 备份落点（全盘刷新 `--keep-files`）
    Backup,
    /// 填充文件临时落点（全盘刷新 `--fill-free`）
    Fill,
}

impl WorkSubDir {
    /// 子目录名
    pub fn dir_name(self) -> &'static str {
        match self {
            WorkSubDir::Backup => "backup",
            WorkSubDir::Fill => "fill",
        }
    }
}

/// 解析工作目录——**必填**，空白视为未提供（issue #14：⛔ 不用兜底默认值）。
///
/// 各系统的临时目录不同，内置默认值会写错位置 ⇒ 未配置一律返回 Err，
/// 由调用方打印双语错误并中止，不静默取默认值。
pub fn resolve_work_dir(work_dir: Option<&str>) -> Result<std::path::PathBuf, String> {
    match work_dir.map(str::trim) {
        Some(w) if !w.is_empty() => Ok(std::path::PathBuf::from(w)),
        _ => Err("工作目录未配置 (work dir not configured)：请显式指定 --work-dir <路径>".to_string()),
    }
}

/// 取工作目录下某个用途的子目录路径（只拼路径，不建目录）
pub fn work_subdir(work_dir: &std::path::Path, sub: WorkSubDir) -> std::path::PathBuf {
    work_dir.join(sub.dir_name())
}

/// 准备（创建）工作目录及其子用途目录，并做可写性探测。
///
/// 返回 Err = 不可创建或不可写；调用方据此中止，不继续执行破坏性流程。
pub fn prepare_work_dir(
    work_dir: &std::path::Path,
    sub: Option<WorkSubDir>,
) -> Result<std::path::PathBuf, String> {
    let target = match sub {
        Some(s) => work_subdir(work_dir, s),
        None => work_dir.to_path_buf(),
    };

    std::fs::create_dir_all(&target)
        .map_err(|e| format!("无法创建工作目录 {} (cannot create work dir): {}", target.display(), e))?;

    // 可写性探测：建一个探针文件再删掉
    let probe = target.join(".coldatafresh-writable-probe");
    std::fs::write(&probe, b"probe")
        .map_err(|e| format!("工作目录不可写 {} (work dir not writable): {}", target.display(), e))?;
    let _ = std::fs::remove_file(&probe);

    Ok(target)
}

//// -------------------- tracefs 操作 (tracefs ops) --------------------
// ⚠️ tracefs 是 Linux 专有（`/sys/kernel/debug/tracing` + unix dev_t）⇒ 整段须加平台守卫，
// 否则 Windows/musl 目标编译时会执行到 `std::os::unix`（CI 实测 error[E0433]）。
/// tracefs 根目录（内核 4.19 与 6.x 路径一致）
#[cfg(target_os = "linux")]
const TRACEFS: &str = "/sys/kernel/debug/tracing";

/// 重置 trace：关采样、清空 buffer、关全部事件
#[cfg(target_os = "linux")]
pub fn tracefs_reset() -> Result<(), String> {
    tracefs_write("tracing_on", "0")?;
    tracefs_write("trace", "")?;
    // 关全部事件（可能失败，不致命）
    let _ = tracefs_write("events/enable", "0");
    Ok(())
}

/// 开关 trace
#[cfg(target_os = "linux")]
pub fn tracefs_on(on: bool) -> Result<(), String> {
    tracefs_write("tracing_on", if on { "1" } else { "0" })
}

/// 启用 / 关闭指定事件（如 "block/block_rq_issue"）
#[cfg(target_os = "linux")]
pub fn tracefs_enable(event: &str, on: bool) -> Result<(), String> {
    tracefs_write(
        &format!("events/{}/enable", event),
        if on { "1" } else { "0" },
    )
}

/// 设置 ring buffer 大小（KB）
#[cfg(target_os = "linux")]
pub fn tracefs_set_buffer_kb(kb: u64) -> Result<(), String> {
    tracefs_write("buffer_size_kb", &kb.to_string())
}

/// 读取 trace 文本，并只保留目标设备的行
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
fn device_dev_t(device: &str) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let md = std::fs::metadata(device).ok()?;
    let rdev = md.rdev();
    let major = (rdev >> 8) & 0xfff;
    let minor = (rdev & 0xff) | ((rdev >> 12) & 0xfff00);
    Some(format!("{},{}", major, minor))
}

/// 写 tracefs 文件（统一错误处理）
#[cfg(target_os = "linux")]
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
