//! LBA 热度采样 —— 通过 tracefs 抓 block 层 IO，按 LBA 分桶统计冷热。
//!
//! 目的（issue #8 + 用户口径）：热力图对象 = **SSD 的 LBA 块**（不只盯 KV），
//! 用于识别**长期不读不写的冷块**（电荷保持最差、最该刷新），而非无差别全盘写。
//!
//! 采样后端（运行时探测，⛔ 不编译期写死）：
//! - `tracefs`  —— 零依赖，写 `events/block/block_rq_issue/enable` + 读 `trace`；
//!   在旧内核（如 1-243 的 4.19，无 BTF、perf 包缺失）上是**唯一可用**路径。
//! - 未来可扩：`perf`（需 linux-perf 包）、`bpftrace`（需 BTF）—— 见 `probe_backend()`。
//!
//! 平台专有部分（tracefs 路径、ioctl）在 `platform/*.rs`；本文件只留平台无关逻辑。

use std::collections::HashMap;
use std::io::Write;

use crate::log::logger;

//// ===================== LBA 热度采样 (LBA heat sampling) =====================

/// 采样后端
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceBackend {
    /// tracefs 直读（零依赖，新旧内核均可）
    TraceFs,
    /// perf（需 linux-perf 包）
    Perf,
    /// bpftrace（需 BTF，仅新内核）
    Bpftrace,
    /// 无可用后端
    None,
}

/// 单个 LBA 桶的热度
#[derive(Debug, Clone, Default)]
pub struct LbaBucket {
    /// 桶序号（从 0 开始）
    pub index: u64,
    /// 桶起始 LBA（扇区）
    pub start_sector: u64,
    /// 桶覆盖扇区数
    pub sectors: u64,
    /// 读次数
    pub reads: u64,
    /// 写次数
    pub writes: u64,
    /// 读扇区总量
    pub read_sectors: u64,
    /// 写扇区总量
    pub write_sectors: u64,
}

impl LbaBucket {
    /// 总访问次数
    pub fn total_io(&self) -> u64 {
        self.reads + self.writes
    }

    /// 热度分级：hot / warm / cold / dead
    pub fn hotness(&self, hot_threshold: u64, warm_threshold: u64) -> Hotness {
        let n = self.total_io();
        if n == 0 {
            Hotness::Dead
        } else if n >= hot_threshold {
            Hotness::Hot
        } else if n >= warm_threshold {
            Hotness::Warm
        } else {
            Hotness::Cold
        }
    }
}

/// 热度分级（issue #8 口径）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hotness {
    /// 近期高频命中
    Hot,
    /// 偶尔复用
    Warm,
    /// 长期未命中（**该刷新的对象**）
    Cold,
    /// 可回收 / 从未访问
    Dead,
}

impl Hotness {
    /// 中文+英文标签
    pub fn label(&self) -> &'static str {
        match self {
            Hotness::Hot => "热 (hot)",
            Hotness::Warm => "温 (warm)",
            Hotness::Cold => "冷 (cold)",
            Hotness::Dead => "死 (dead)",
        }
    }
}

/// 采样结果
#[derive(Debug, Clone, Default)]
pub struct HeatMap {
    /// 使用的后端
    pub backend: String,
    /// 采样事件总数
    pub events: u64,
    /// 桶大小（扇区）
    pub bucket_sectors: u64,
    /// 各桶热度（稀疏：只含有访问的桶）
    pub buckets: HashMap<u64, LbaBucket>,
    /// 采样基准容量：**整盘**总扇区数（`block_rq_issue` 的 sector 是整盘绝对偏移）
    pub device_sectors: u64,
    /// 本设备（可能是分区）在整盘内的起始扇区；整盘自身为 0
    pub partition_start_sectors: u64,
}

impl HeatMap {
    /// 桶大小（按 MiB 指定，换算为扇区）
    pub fn bucket_size_from_mib(mib: u64) -> u64 {
        mib.max(1) * 2048 // 1 MiB = 2048 扇区
    }

    /// 冷热分级统计（含"从未访问"的桶）
    pub fn classify(&self, hot_th: u64, warm_th: u64) -> (u64, u64, u64, u64) {
        let mut hot = 0u64;
        let mut warm = 0u64;
        let mut cold = 0u64;

        for b in self.buckets.values() {
            match b.hotness(hot_th, warm_th) {
                Hotness::Hot => hot += 1,
                Hotness::Warm => warm += 1,
                Hotness::Cold => cold += 1,
                Hotness::Dead => {}
            }
        }

        // 从未访问的桶 = 总桶数 - 有访问的桶数
        let total_buckets = self
            .device_sectors
            .checked_div(self.bucket_sectors)
            .unwrap_or(0);
        let dead = total_buckets.saturating_sub(self.buckets.len() as u64);

        (hot, warm, cold, dead)
    }

    /// 冷块清单（按桶起始 LBA 升序）—— 刷写决策的输入
    pub fn cold_buckets(&self, cold_threshold: u64) -> Vec<LbaBucket> {
        let mut v: Vec<LbaBucket> = self
            .buckets
            .values()
            .filter(|b| b.total_io() > 0 && b.total_io() < cold_threshold)
            .cloned()
            .collect();
        v.sort_by_key(|b| b.start_sector);
        v
    }
}

/// 探测可用采样后端（运行时，⛔ 不编译期写死）
pub fn probe_backend() -> TraceBackend {
    // tracefs 优先：零依赖，新旧内核通吃（1-243 的 4.19 只有这条可用）
    if crate::platform::tracefs_available() {
        return TraceBackend::TraceFs;
    }
    if crate::platform::perf_available() {
        return TraceBackend::Perf;
    }
    if crate::platform::bpftrace_available() {
        return TraceBackend::Bpftrace;
    }
    TraceBackend::None
}

/// 采样 LBA 热度：按平台分册取数（Linux = tracefs / Windows = ETW），
/// 本函数只负责「分桶 + 组装 HeatMap」，展示与判据全在公共层。
///
/// `device`: 目标块设备（如 /dev/nvme0n1）
/// `bucket_mib`: 分桶粒度（MiB）
/// `probe`: 造 IO 的闭包（由调用方提供，便于测试与不同业务场景）
pub fn sample<F>(
    device: &str,
    bucket_mib: u64,
    probe: F,
) -> Result<HeatMap, String>
where
    F: FnOnce() -> Result<(), String>,
{
    let bucket_sectors = HeatMap::bucket_size_from_mib(bucket_mib);

    // 各平台机制不同、输出契约相同（platform/heat_sample 分册）
    let out = crate::platform::heat_sample::sample_io(device, probe)?;

    // 按桶聚合（平台无关：三平台的 RawIo 形态一致）
    let mut buckets: HashMap<u64, LbaBucket> = HashMap::new();
    for io in &out.events {
        let idx = io.sector / bucket_sectors;
        let entry = buckets.entry(idx).or_insert_with(|| LbaBucket {
            index: idx,
            start_sector: idx * bucket_sectors,
            sectors: bucket_sectors,
            reads: 0,
            writes: 0,
            read_sectors: 0,
            write_sectors: 0,
        });
        if io.is_write {
            entry.writes += 1;
            entry.write_sectors += io.sectors;
        } else {
            entry.reads += 1;
            entry.read_sectors += io.sectors;
        }
    }

    let hm = HeatMap {
        backend: out.backend,
        events: out.events.len() as u64,
        bucket_sectors,
        buckets,
        device_sectors: out.device_sectors,
        partition_start_sectors: out.partition_start_sectors,
    };

    logger().log(
        &format!(
            "LBA 热度采样完成: 后端={}, 事件={}, 桶数={}, 桶大小={} MiB",
            hm.backend,
            hm.events,
            hm.buckets.len(),
            bucket_mib
        ),
        "INFO",
    );

    Ok(hm)
}

/// 冷热四档分布条（按占比画实心/空心块，供交互界面展示）
///
/// `total` 为总桶数；四档各自画一条按占比长度的色阶条。
fn hotness_bar(count: u64, total: u64, width: usize) -> String {
    if total == 0 {
        return "-".to_string();
    }
    let filled = ((count as f64 / total as f64) * width as f64).round() as usize;
    let filled = filled.min(width);
    format!("{}{}", "█".repeat(filled), "·".repeat(width - filled))
}

/// 打印热力图（交互界面版，issue #8 要求交互界面热力图）
///
/// 与 `print_heatmap_cli` 的区别：带分桶色阶图与四档分布条，供人在菜单里直观看冷热。
pub fn print_heatmap_interactive(hm: &HeatMap, hot_th: u64, warm_th: u64) {
    let eq = "=".repeat(60);
    println!("\n{}", eq);
    println!("          LBA 热力图 (LBA Heat Map) — 交互视图");
    println!("{}", eq);

    let (hot, warm, cold, dead) = hm.classify(hot_th, warm_th);
    let total_buckets = hm
        .device_sectors
        .checked_div(hm.bucket_sectors)
        .unwrap_or(0);

    println!("采样后端 (Backend):   {}", hm.backend);
    println!("采样事件 (Events):    {}", hm.events);
    println!("桶大小 (Bucket):      {} MiB", hm.bucket_sectors / 2048);
    println!("整盘容量 (Disk):      {} MiB", hm.device_sectors / 2048);
    println!("采样基准 (LBA base):  整盘绝对偏移 / disk-absolute");
    if hm.partition_start_sectors > 0 {
        println!(
            "分区起始 (Part start):{} 扇区 ({} MiB)",
            hm.partition_start_sectors,
            hm.partition_start_sectors / 2048
        );
    }
    println!("{}", eq);

    // 四档分布条
    println!("冷热分级 (Hotness distribution):");
    println!(
        "  🔥 热 (hot)  {:>6} 桶  {}  (>= {} 次)",
        hot,
        hotness_bar(hot, total_buckets, 30),
        hot_th
    );
    println!(
        "  🌤 温 (warm) {:>6} 桶  {}  (>= {} 次)",
        warm,
        hotness_bar(warm, total_buckets, 30),
        warm_th
    );
    println!(
        "  🧊 冷 (cold) {:>6} 桶  {}  (< {} 次) ← 建议刷新",
        cold,
        hotness_bar(cold, total_buckets, 30),
        warm_th
    );
    println!(
        "  ⬜ 死 (dead) {:>6} 桶  {}  (从未访问)",
        dead,
        hotness_bar(dead, total_buckets, 30)
    );

    // 桶级色阶图：按桶序号升序，用字符表示热力强度
    println!("\nLBA 分桶热力条 (Bucket heat strip, 冷→热):");
    let mut idx: Vec<u64> = hm.buckets.keys().copied().collect();
    idx.sort_unstable();
    if idx.is_empty() {
        println!("  (无采样数据 / no samples)");
    } else if idx.len() > 64 {
        println!("  (共 {} 个有访问的桶，仅示前 64 / showing first 64)", idx.len());
    }
    let max_io = hm
        .buckets
        .values()
        .map(|b| b.total_io())
        .max()
        .unwrap_or(1)
        .max(1);

    for (n, i) in idx.iter().enumerate() {
        if n >= 64 {
            break;
        }
        let b = &hm.buckets[i];
        let io = b.total_io();
        let ratio = io as f64 / max_io as f64;
        let level = (ratio * 8.0).ceil() as usize;
        let glyph = match level {
            0..=1 => "▁",
            2..=3 => "▃",
            4..=5 => "▅",
            6..=7 => "▆",
            _ => "█",
        };
        let tag = b.hotness(hot_th, warm_th).label();
        println!(
            "  {:>6} {}{:<7} LBA[{:>10}..{:>10})  读写 {:>5}/{:>5}",
            b.index,
            glyph.repeat(((ratio * 20.0).ceil() as usize).clamp(1, 20)),
            tag,
            b.start_sector,
            b.start_sector + b.sectors,
            b.reads,
            b.writes
        );
    }
    println!("{}", eq);
}

/// 打印热力图（CLI 模式，issue #8 要求 CLI 热力图）
pub fn print_heatmap_cli(hm: &HeatMap, hot_th: u64, warm_th: u64) {
    let eq = "=".repeat(60);
    println!("\n{}", eq);
    println!("          LBA 热力图 (LBA Heat Map) — CLI 模式");
    println!("{}", eq);
    println!("采样后端 (Backend):   {}", hm.backend);
    println!("采样事件 (Events):    {}", hm.events);
    println!("桶大小 (Bucket):      {} MiB ({} 扇区)", hm.bucket_sectors / 2048, hm.bucket_sectors);
    println!("设备容量 (Capacity):  {} MiB", hm.device_sectors / 2048);
    println!("{}", eq);

    let (hot, warm, cold, dead) = hm.classify(hot_th, warm_th);
    println!("冷热分级 (Hotness):");
    println!("  🔥 热 (hot)   : {:6} 桶 (>= {} 次)", hot, hot_th);
    println!("  🌤 温 (warm)  : {:6} 桶 (>= {} 次)", warm, warm_th);
    println!("  🧊 冷 (cold)  : {:6} 桶 (< {} 次) ← 建议刷新", cold, warm_th);
    println!("  ⬜ 死 (dead)  : {:6} 桶 (从未访问)", dead);

    // 按访问次数降序打印 top 桶
    let mut v: Vec<&LbaBucket> = hm.buckets.values().collect();
    v.sort_by_key(|b| std::cmp::Reverse(b.total_io()));

    println!("\n热点桶 (Top hot buckets):");
    for b in v.iter().take(10) {
        println!(
            "  桶{:>6} LBA[{:>10}..{:>10})  读 {:>5} 写 {:>5}  {}",
            b.index,
            b.start_sector,
            b.start_sector + b.sectors,
            b.reads,
            b.writes,
            b.hotness(hot_th, warm_th).label()
        );
    }

    let cold_list = hm.cold_buckets(warm_th);
    if !cold_list.is_empty() {
        println!("\n冷块候选 (Cold buckets, 建议刷新) — 前 10:");
        for b in cold_list.iter().take(10) {
            println!(
                "  桶{:>6} LBA[{:>10}..{:>10})  读 {:>5} 写 {:>5}",
                b.index,
                b.start_sector,
                b.start_sector + b.sectors,
                b.reads,
                b.writes
            );
        }
        if cold_list.len() > 10 {
            println!("  ... 共 {} 个冷块", cold_list.len());
        }
    }
    println!("{}", eq);
}

/// 保存热力图报告（JSON Lines，落程序 logs/ 下）
pub fn save_heatmap_report(hm: &HeatMap, hot_th: u64, warm_th: u64) -> String {
    let cfg = crate::config::config();
    let log_dir = cfg
        .error_log
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("logs"));
    let _ = std::fs::create_dir_all(&log_dir);

    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let path = log_dir.join(format!("lba-heatmap-{}.jsonl", ts));
    let path_str = path.display().to_string();

    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let (hot, warm, cold, dead) = hm.classify(hot_th, warm_th);
        let summary = serde_json::json!({
            "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
            "kind": "summary",
            "backend": hm.backend,
            "events": hm.events,
            "bucket_sectors": hm.bucket_sectors,
            "device_sectors": hm.device_sectors,
            "hot": hot, "warm": warm, "cold": cold, "dead": dead,
            "hot_threshold": hot_th, "warm_threshold": warm_th,
        });
        let _ = writeln!(f, "{}", summary);

        // 逐桶明细（按 LBA 升序）
        let mut v: Vec<&LbaBucket> = hm.buckets.values().collect();
        v.sort_by_key(|b| b.start_sector);
        for b in v {
            let rec = serde_json::json!({
                "kind": "bucket",
                "index": b.index,
                "start_sector": b.start_sector,
                "sectors": b.sectors,
                "reads": b.reads,
                "writes": b.writes,
                "read_sectors": b.read_sectors,
                "write_sectors": b.write_sectors,
                "hotness": format!("{:?}", b.hotness(hot_th, warm_th)).to_lowercase(),
            });
            let _ = writeln!(f, "{}", rec);
        }
    }

    logger().log(&format!("LBA 热力图报告已保存: {}", path_str), "INFO");
    path_str
}

//// ==============================================================================
