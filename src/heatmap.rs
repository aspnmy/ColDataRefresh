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
#[cfg(target_os = "linux")]
use std::path::Path;

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

/// 采样 LBA 热度：开 trace → 执行 probe 动作 → 关 trace → 解析。
///
/// `device`: 目标块设备（如 /dev/nvme0n1）
/// `bucket_mib`: 分桶粒度（MiB）
/// `probe`: 造 IO 的闭包（由调用方提供，便于测试与不同业务场景）
pub fn sample<F>(
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))] device: &str,
    bucket_mib: u64,
    probe: F,
) -> Result<HeatMap, String>
where
    F: FnOnce() -> Result<(), String>,
{
    let backend = probe_backend();
    if backend == TraceBackend::None {
        return Err("无可用采样后端 (no trace backend available)".to_string());
    }
    if backend != TraceBackend::TraceFs {
        return Err(format!(
            "当前仅实现 tracefs 后端，探测到 {:?} (only tracefs implemented)",
            backend
        ));
    }

    let bucket_sectors = HeatMap::bucket_size_from_mib(bucket_mib);

    // 1) 准备 tracefs：只开 block_rq_issue
    // ⚠️ tracefs 与 sysfs 都是 Linux 专有 —— 非 Linux 平台走下方 else 分支直接返回"不支持"，
    // 避免编译期引用不存在的符号（CI 实测 Windows 目标 error[E0425]）。
    #[cfg(target_os = "linux")]
    {
        let (device_sectors, partition_start_sectors) = device_capacity_sectors(device)?;

        crate::platform::tracefs_reset()?;
        crate::platform::tracefs_enable("block/block_rq_issue", true)?;
        crate::platform::tracefs_set_buffer_kb(2048)?;

        // 2) 开 trace → 跑 probe → 关 trace
        crate::platform::tracefs_on(true)?;
        let probe_result = probe();
        crate::platform::tracefs_on(false)?;
        crate::platform::tracefs_enable("block/block_rq_issue", false)?;

        // 3) 取 trace 文本
        let raw = crate::platform::tracefs_read(device)?;

        // 4) 解析
        let (events, buckets) = parse_trace(&raw, bucket_sectors)?;

        let hm = HeatMap {
            backend: "tracefs".to_string(),
            events,
            bucket_sectors,
            buckets,
            device_sectors,
            partition_start_sectors,
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

        probe_result?;
        Ok(hm)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (bucket_sectors, probe, bucket_mib);
        Err("LBA 热度采样依赖 tracefs，当前平台不支持 (tracefs is Linux-only)".to_string())
    }
}

/// 解析 trace 文本 —— 提取每笔 IO 的 LBA / 长度 / 读写方向，按桶聚合。
///
/// block_rq_issue 行格式：
/// ```text
///               dd-39063 [005] ....  9693.847770: block_rq_issue: 259,0 R 262144 () 2048 + 512 [dd]
/// ```
/// 字段（按空白切分）：dev=$1 rw=$2 bytes=$3 ... sector=$5 +nr=$7 [comm]=$9
#[cfg(target_os = "linux")]
fn parse_trace(
    raw: &str,
    bucket_sectors: u64,
) -> Result<(u64, HashMap<u64, LbaBucket>), String> {
    let mut buckets: HashMap<u64, LbaBucket> = HashMap::new();
    let mut events = 0u64;

    for line in raw.lines() {
        // 只看 block_rq_issue 事件
        let Some(pos) = line.find("block_rq_issue:") else {
            continue;
        };
        let rest = line[pos + "block_rq_issue:".len()..].trim();
        let parts: Vec<&str> = rest.split_whitespace().collect();
        // dev=$0 rw=$1 bytes=$2 ($3) sector=$4 +$5 nr=$6 [$7]
        if parts.len() < 7 {
            continue;
        }

        let rw = parts[1];
        // sector 是 "+" 前的那个数
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

        let idx = sector / bucket_sectors;
        let entry = buckets.entry(idx).or_insert_with(|| LbaBucket {
            index: idx,
            start_sector: idx * bucket_sectors,
            sectors: bucket_sectors,
            ..Default::default()
        });

        // 扇区数：优先 nr，缺失时用 bytes/512 估算
        let sectors = if nr > 0 { nr } else { bytes / 512 };
        if is_write {
            entry.writes += 1;
            entry.write_sectors += sectors;
        } else if is_read {
            entry.reads += 1;
            entry.read_sectors += sectors;
        }
        events += 1;
    }

    Ok((events, buckets))
}

/// 读块设备的 sysfs 属性。
///
/// ⚠️ sysfs 有两种视图，必须按需择用（1-243 实测）：
/// - `/sys/block/<name>` —— **只有整盘**，分区读不到（`nvme0n1p1` ⇒ ENOENT）
/// - `/sys/class/block/<name>` —— **扁平视图，整盘与分区都在**
///
/// 目标盘常是分区（如 `/dev/nvme0n1p1`）⇒ 统一走 `/sys/class/block`，两态通吃。
///
/// ⚠️ 只有 Linux 分支会用到（`device_capacity_sectors` 按平台门控）⇒ 同样加门控防死代码告警。
#[cfg(target_os = "linux")]
fn read_block_attr(name: &str, attr: &str) -> Option<String> {
    let p = format!("/sys/class/block/{}/{}", name, attr);
    std::fs::read_to_string(&p).ok().map(|s| s.trim().to_string())
}

/// 采样基准：整盘扇区数 + 本设备在整盘内的起始扇区。
///
/// ⚠️ `block_rq_issue` 的 `sector` 是**整盘绝对偏移**（不是分区内相对偏移）
/// ⇒ 分桶上界必须用**整盘**容量，否则桶边界与真实 LBA 错位。
/// 返回 `(整盘扇区数, 本设备起始扇区)`；整盘自身起始为 0。
///
/// ⚠️ sysfs 是 Linux 专有 —— 本函数只在 Linux 分支被调用（见 `sample()`），
/// 非 Linux 目标下 `parent_disk` 不存在 ⇒ 整段按平台门控，避免 Windows 目标编译失败。
#[cfg(target_os = "linux")]
fn device_capacity_sectors(device: &str) -> Result<(u64, u64), String> {
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
