//! Linux 分册 —— tracefs `block_rq_issue` 直读（零依赖，新旧内核通吃）
//!
//! 1-243（内核 4.19.232，无 BTF、perf 包缺失）实测唯一可用路径；
//! 字段列位见 `parse_trace` 的文档注释（⛔ sector 是去前缀后第 5 个字段）。

use super::{RawIo, SampleOutput};
use std::path::Path;

/// 采样：开 tracefs → 跑 probe → 关 tracefs → 解析事件
///
/// ⚠️ 分桶基准用**整盘**扇区数：`block_rq_issue` 的 `sector` 是**整盘绝对偏移**
/// （不是分区内相对偏移）⇒ 用分区容量当上界会让桶边界错位。
pub fn sample_io<F>(device: &str, probe: F) -> Result<SampleOutput, String>
where
    F: FnOnce() -> Result<(), String>,
{
    let (device_sectors, partition_start_sectors) = device_geometry(device)?;

    // 1) 准备 tracefs：只开 block_rq_issue
    crate::platform::tracefs_reset()?;
    crate::platform::tracefs_enable("block/block_rq_issue", true)?;
    crate::platform::tracefs_set_buffer_kb(2048)?;

    // 2) 开 trace → 跑 probe → 关 trace
    crate::platform::tracefs_on(true)?;
    let probe_result = probe();
    crate::platform::tracefs_on(false)?;
    crate::platform::tracefs_enable("block/block_rq_issue", false)?;

    // 3) 取 trace 文本并解析
    let raw = crate::platform::tracefs_read(device)?;
    let events = parse_trace(&raw);

    probe_result?;

    Ok(SampleOutput {
        events,
        device_sectors,
        partition_start_sectors,
        backend: "tracefs".to_string(),
    })
}

/// 采样基准：整盘扇区数 + 本设备在整盘内的起始扇区
///
/// sysfs 有两种视图（1-243 实测），必须按需择用：
/// - `/sys/block/<name>`      —— **只有整盘**，分区读不到（`nvme0n1p1` ⇒ ENOENT）
/// - `/sys/class/block/<name>` —— **扁平视图，整盘与分区都在**
///
/// 目标盘常是分区（如 `/dev/nvme0n1p1`）⇒ 统一走 `/sys/class/block`，两态通吃。
pub fn device_geometry(device: &str) -> Result<(u64, u64), String> {
    let name = Path::new(device)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .ok_or_else(|| format!("无法解析设备名: {}", device))?;

    // 分区：class/block/<name>/partition 存在 ⇒ 取其所属整盘
    let is_part = read_block_attr(&name, "partition").is_some();
    let disk = if is_part {
        crate::platform::parent_disk(&name)
            .ok_or_else(|| format!("无法从分区名反解整盘: {}", name))?
    } else {
        name.clone()
    };

    let start = read_block_attr(&name, "start")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let disk_sectors = read_block_attr(&disk, "size")
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .ok_or_else(|| format!("读取 /sys/class/block/{}/size 失败", disk))?;

    Ok((disk_sectors, start))
}

/// 读块设备的 sysfs 属性（统一走 `/sys/class/block` 扁平视图，整盘与分区通吃）
fn read_block_attr(name: &str, attr: &str) -> Option<String> {
    let p = format!("/sys/class/block/{}/{}", name, attr);
    std::fs::read_to_string(&p).ok().map(|s| s.trim().to_string())
}

/// 解析 trace 文本 —— 提取每笔 IO 的 LBA / 长度 / 读写方向。
///
/// block_rq_issue 行格式：
/// ```text
///               dd-39063 [005] ....  9693.847770: block_rq_issue: 259,0 R 262144 () 2048 + 512 [dd]
/// ```
/// 字段（去 `block_rq_issue:` 前缀后按空白切分）：
/// `dev=$0 rw=$1 bytes=$2 ($3) sector=$4 +nr=$5 [comm]=$6 ...`
///
/// ⚠️ **sector = 去前缀后的第 5 个字段**（`parts[4]`）—— 曾误取 `$14`/`$7` 致统计全错。
fn parse_trace(raw: &str) -> Vec<RawIo> {
    let mut events = Vec::new();

    for line in raw.lines() {
        // 只看 block_rq_issue 事件
        let Some(pos) = line.find("block_rq_issue:") else {
            continue;
        };
        let rest = line[pos + "block_rq_issue:".len()..].trim();
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() < 6 {
            continue;
        }

        let rw = parts[1];
        let sector: u64 = match parts[4].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        // nr_sector 形如 "+512"
        let nr: u64 = parts[5].trim_start_matches('+').parse().unwrap_or(0);
        let bytes: u64 = parts[2].parse().unwrap_or(0);

        // 读写方向：R=读，W/WS/WF=写
        let is_write = rw.starts_with('W') || rw.starts_with('F');
        let is_read = rw.starts_with('R') || rw.starts_with('A');
        if !is_write && !is_read {
            continue;
        }

        // 扇区数：优先 nr，缺失时用 bytes/512 估算
        let sectors = if nr > 0 { nr } else { bytes / 512 };

        events.push(RawIo {
            sector,
            sectors,
            is_write,
        });
    }

    events
}
