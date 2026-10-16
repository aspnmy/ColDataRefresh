//! LBA 热度采样分册（issue #12 —— Windows 也须实现，⛔ 不以「该平台不支持」收口）
//!
//! 各平台机制不同、**输出契约相同**：
//! - `linux.rs`  —— tracefs `block_rq_issue` 直读（零依赖，新旧内核通吃）
//! - `windows.rs` —— ETW `Microsoft-Windows-Kernel-Disk` 实时会话
//! - `darwin.rs` —— 明确不支持并写清成因（macOS 无用户态块层追踪）
//!
//! 三个平台的实现都返回**同一种原始事件**（`RawIo`），分桶与展示全在
//! `heatmap.rs` 公共层 ⇒ 输出天然一致，加平台 = 加一个文件。

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
mod darwin;

// 只导出 `sample_io`（各平台统一入口）；`device_geometry` 为分册内部使用，
// 不对外导出（⛔ 避免非宿主目标的 unused_import 告警）
#[cfg(target_os = "linux")]
pub use linux::sample_io;

#[cfg(target_os = "windows")]
pub use windows::sample_io;

/// macOS：平台不提供 LBA 采样 ⇒ `sample_io` 返明确成因
#[cfg(target_os = "macos")]
pub use darwin::sample_io;

/// 一笔块层 IO 的原始记录（三平台统一形态）
///
/// 对应 Linux tracefs `block_rq_issue` 的 `sector`/`nr_sector`/`rwbs`，
/// 与 Windows ETW `DiskIo_TypeGroup1` 的 `ByteOffset`/`TransferSize`。
#[derive(Debug, Clone, Copy)]
pub struct RawIo {
    /// 起始扇区（**整盘绝对偏移**，扇区单位）
    pub sector: u64,
    /// 扇区数
    pub sectors: u64,
    /// 是否写（false = 读）
    pub is_write: bool,
}

/// 采样结果：原始事件流 + 采样基准
#[derive(Debug, Clone, Default)]
pub struct SampleOutput {
    /// 采到的全部 IO 事件
    pub events: Vec<RawIo>,
    /// 采样基准：**整盘**总扇区数（因 event 的 sector 是整盘绝对偏移）
    pub device_sectors: u64,
    /// 本设备（可能是分区）在整盘内的起始扇区；整盘为 0
    pub partition_start_sectors: u64,
    /// 后端名（用于报告）
    pub backend: String,
}
