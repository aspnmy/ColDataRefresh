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

#[cfg(target_os = "windows")]
pub use windows::{
    get_disk_space, get_os_display, is_admin, is_root_path, resolve_block_device,
    resolve_device_name, trim_volume,
};

#[cfg(target_os = "macos")]
pub use darwin::{
    get_disk_space, get_os_display, is_admin, is_root_path, resolve_block_device,
    resolve_device_name, trim_volume,
};

use crate::log::logger;

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
