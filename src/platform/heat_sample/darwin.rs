//! macOS 分册 —— LBA 热度采样在 macOS 上**平台不提供**，返明确成因
//!
//! 成因（不是"未实现"，是平台能力边界）：
//! - macOS 内核**不允许用户态**读取块层 IO 事件（无 tracefs / ETW 等价物）
//! - `fs_usage` 只能给**文件级**访问（粒度是文件，不是 LBA）⇒ 换不来 LBA 热度
//! - `iotop`/`dtrace` 在 SIP 保护的发行版上对块层受限
//!
//! ⇒ 按平台分册口径：**只有平台真不提供的能力才返"不支持"，并写清成因**
//! （⛔ 不是空壳占位；Linux/Windows 均已给出等价实现）。

use super::{RawIo, SampleOutput};

/// macOS 无 LBA 采样能力 —— 返明确成因
pub fn sample_io<F>(_device: &str, _probe: F) -> Result<SampleOutput, String>
where
    F: FnOnce() -> Result<(), String>,
{
    Err(
        "LBA 热度采样在 macOS 上不可用 (LBA heat sampling unavailable on macOS)：\
         内核不允许用户态读取块层 IO 事件，且无 tracefs / ETW 等价机制；\
         fs_usage 只能提供文件级访问（粒度非 LBA）"
            .to_string(),
    )
}

/// macOS 侧设备几何：`diskutil` 可读，但采样不可用 ⇒ 本函数仅作信息性返回
pub fn device_geometry(_device: &str) -> Result<(u64, u64), String> {
    Ok((0, 0))
}

/// 供内部使用：构造空事件（保持与其它平台同形，便于测试）
#[allow(dead_code)]
fn empty_events() -> Vec<RawIo> {
    Vec::new()
}
