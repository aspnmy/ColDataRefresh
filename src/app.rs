//! 应用控制器 —— CLI 非交互模式与交互菜单模式的流程编排。

use std::io::Write;
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::config::{self, FileCategory};
use crate::dashboard::{Dashboard, Stats};
use crate::file_op;
use crate::full_refresh::FullRefresh;
use crate::log::logger;
use crate::menu;
use crate::platform;

//// ========================== CLI 模式 (CLI mode) ==========================
/// 运行参数 — 由 CLI 解析结果注入
pub struct RunOptions {
    pub cli: bool,
    pub path: Option<String>,
    pub full_refresh: bool,
    pub trim: bool,
    /// 跑圈校验模式（写入 1GB 随机数据 + 校验码，写满后回读校验以定位坏块）
    pub cycle_check: bool,
    /// 跑圈校验圈数（默认 2）
    pub cycles: u64,
    /// LBA 热力图采样模式
    pub heatmap: bool,
    /// 热力图：分桶粒度（MiB）
    pub bucket_mib: u64,
    /// 热力图：热桶阈值（访问次数）
    pub hot_threshold: u64,
    /// 热力图：温桶阈值（访问次数）
    pub warm_threshold: u64,
    pub age: Option<u32>,
    pub skip_smaller: Option<u64>,
    pub yes: bool,
    pub buffer_size_mb: Option<u32>,
    // 全盘刷新参数（CLI 非交互路径使用，替代原有的 stdin 提问）
    pub keep_files: bool,
    pub no_keep_files: bool,
    pub fill_free: bool,
    pub unit_gb: u64,
    pub write_buf_kb: u64,
}

/// 应用控制器 — 交互菜单与模式路由
pub struct App {
    dashboard: Dashboard,
    stats: Stats,
}

impl App {
    pub fn new() -> Self {
        Self {
            dashboard: Dashboard::new(),
            stats: Stats::default(),
        }
    }

    /// 主入口 — 返回进程退出码（0 成功 / 1 运行失败 / 2 参数错误）
    pub fn run(&mut self, opts: RunOptions) -> i32 {
        // 注册 Ctrl+C 信号处理器
        let _ = ctrlc::set_handler(|| {
            // 先设置中断标志，让当前操作优雅退出
            file_op::INTERRUPTED.store(true, Ordering::Relaxed);
            // 短暂延迟后直接退出，避免阻塞在 stdin 读上
            std::thread::sleep(std::time::Duration::from_millis(200));
            // 130 = 128 + SIGINT，符合 shell 惯例
            std::process::exit(130);
        });

        if opts.cli {
            return self.run_cli(opts);
        }

        // 无 -cli ⇒ 交互模式（行为保持原样）
        self.run_interactive(opts)
    }

    //// -------------------- CLI 非交互执行路径 (non-interactive path) --------------------
    /// CLI 非交互模式 — 全程不读 stdin；缺参即报错退出（exit 2）
    fn run_cli(&mut self, opts: RunOptions) -> i32 {
        // 1) 目标目录必填（跨平台路径是硬需求）
        let directory = match opts.path.as_deref().map(str::trim) {
            Some(p) if !p.is_empty() => p.to_string(),
            _ => {
                eprintln!("错误 (Error): -cli 模式必须指定目标目录 (target dir required) -p <路径>");
                return 2;
            }
        };
        let dir_path = std::path::Path::new(&directory);
        if !dir_path.exists() {
            eprintln!("错误 (Error): 目标目录不存在 (target dir not found): {}", directory);
            return 2;
        }
        if !dir_path.is_dir() {
            eprintln!("错误 (Error): 目标路径不是目录 (not a directory): {}", directory);
            return 2;
        }

        // 2) 模式互斥
        if opts.full_refresh && opts.trim {
            eprintln!("错误 (Error): --full-refresh 与 --trim 不能同时使用 (mutually exclusive)");
            return 2;
        }
        if opts.cycle_check && (opts.full_refresh || opts.trim) {
            eprintln!("错误 (Error): --cycle-check 不能与 --full-refresh / --trim 同时使用 (mutually exclusive)");
            return 2;
        }
        if opts.heatmap && (opts.full_refresh || opts.trim || opts.cycle_check) {
            eprintln!("错误 (Error): --heatmap 不能与 --full-refresh / --trim / --cycle-check 同时使用 (mutually exclusive)");
            return 2;
        }

        // 3) 智能模式必须有 -a
        if !opts.full_refresh && !opts.trim && !opts.cycle_check && !opts.heatmap && opts.age.is_none() {
            eprintln!("错误 (Error): 智能模式必须指定数据时效 (data age required) -a <天数/days>");
            return 2;
        }

        // 4) 破坏性操作必须显式 -y
        if (opts.full_refresh || opts.trim || opts.cycle_check) && !opts.yes {
            eprintln!(
                "错误 (Error): {} 模式会破坏数据，-cli 下必须显式加 -y 确认 (destructive, -y required)",
                if opts.full_refresh {
                    "全盘刷新 (full-refresh)"
                } else if opts.cycle_check {
                    "跑圈校验 (cycle-check)"
                } else {
                    "TRIM"
                }
            );
            return 2;
        }

        self.dashboard.full_refresh = opts.full_refresh;
        self.dashboard.trim_mode = opts.trim;
        self.dashboard.working_directory = directory.clone();

        // 跑圈校验（CLI 路径）
        if opts.cycle_check {
            let cycles = opts.cycles.max(1);
            let unit_gb = opts.unit_gb.clamp(1, 100);
            let write_buf_kb = opts.write_buf_kb.clamp(64, 1024 * 1024);
            return if self.run_cycle_check_cli(&directory, cycles, unit_gb, write_buf_kb) {
                0
            } else {
                1
            };
        }

        // LBA 热力图（CLI 路径）
        if opts.heatmap {
            let bucket_mib = opts.bucket_mib.clamp(1, 1024);
            let hot_th = opts.hot_threshold.max(1);
            let warm_th = opts.warm_threshold.max(1);
            return if self.run_heatmap(&directory, bucket_mib, hot_th, warm_th, false) {
                0
            } else {
                1
            };
        }

        // TRIM
        if opts.trim {
            let ok = self.run_trim_mode(&directory);
            return if ok { 0 } else { 1 };
        }

        // 全盘刷新（CLI 路径：参数取自命令行，不读 stdin）
        if opts.full_refresh {
            return if self.run_full_refresh_cli(&directory, &opts) { 0 } else { 1 };
        }

        // 智能模式
        let min_days = opts.age.unwrap_or(0);
        let skip_small = opts.skip_smaller.unwrap_or(0) > 0;
        self.dashboard.min_days = min_days;
        self.dashboard.skip_small = skip_small;
        self.dashboard.buffer_size_mb = opts.buffer_size_mb.unwrap_or(512).clamp(64, 2048);

        logger().log(
            &format!(
                "[CLI] 目录='{}', 时效={}天, 跳过小文件={}, 缓冲区={}MB",
                directory, min_days, skip_small, self.dashboard.buffer_size_mb
            ),
            "INFO",
        );

        self.run_smart_refresh(&directory, min_days, skip_small)
    }

    //// ==================== 交互模式 (interactive mode) ====================
    /// 交互模式 — 保持原有菜单/确认/输入流程
    fn run_interactive(&mut self, opts: RunOptions) -> i32 {
        let full_refresh = opts.full_refresh;
        let trim_mode = opts.trim;
        let cycle_check = opts.cycle_check;
        let cli_age = opts.age;
        let cli_skip_smaller_mb = opts.skip_smaller;

        // 打印启动横幅
        menu::show_startup_banner();

        loop {
            // 重置统计
            self.stats = Stats::default();

            // 获取模式选择
            let (mode_full, mode_trim, mode_cycle, mode_heatmap) =
                if !full_refresh && !trim_mode && !cycle_check && !opts.heatmap {
                    match menu::show_mode_menu() {
                        menu::ModeChoice::FullRefresh => (true, false, false, false),
                        menu::ModeChoice::Trim => (false, true, false, false),
                        menu::ModeChoice::CycleCheck => (false, false, true, false),
                        menu::ModeChoice::Heatmap => (false, false, false, true),
                        menu::ModeChoice::Smart => (false, false, false, false),
                    }
                } else {
                    (full_refresh, trim_mode, cycle_check, opts.heatmap)
                };

            // 确认操作
            if mode_full {
                if !menu::confirm_full_refresh() {
                    continue;
                }
            } else if (mode_trim && !menu::confirm_trim())
                || (mode_cycle && !menu::confirm_cycle_check())
            {
                continue;
            }

            self.dashboard.full_refresh = mode_full;
            self.dashboard.trim_mode = mode_trim;

            // 获取目录
            let directory = menu::prompt_directory();

            // TRIM 模式 — 直接执行
            if mode_trim {
                self.run_trim_mode(&directory);
                if !menu::ask_return_to_menu() {
                    break;
                }
                continue;
            }

            // 跑圈校验模式（交互：询问圈数，默认 2）
            if mode_cycle {
                let cycles = opts.cycles.max(1);
                let unit_gb = opts.unit_gb.clamp(1, 100);
                let write_buf_kb = opts.write_buf_kb.clamp(64, 1024 * 1024);
                self.run_cycle_check_cli(&directory, cycles, unit_gb, write_buf_kb);
                if !menu::ask_return_to_menu() {
                    break;
                }
                continue;
            }

            // LBA 热力图模式（交互：询问分桶与阈值；采样只读 ⇒ 无需 yes 确认）
            if mode_heatmap {
                let hp = menu::prompt_heatmap_params();
                // 交互入口 ⇒ 走交互视图（带色阶图），与 CLI 文本视图分离
                self.run_heatmap(&directory, hp.bucket_mib, hp.hot_threshold, hp.warm_threshold, true);
                if !menu::ask_return_to_menu() {
                    break;
                }
                continue;
            }

            // 全盘刷新模式
            if mode_full {
                self.run_full_refresh_mode(&directory);
                if !menu::ask_return_to_menu() {
                    break;
                }
                continue;
            }

            // 智能模式
            let min_days = cli_age.unwrap_or_else(menu::prompt_age);
            let skip_small = if cli_skip_smaller_mb.unwrap_or(0) > 0 {
                true
            } else {
                menu::prompt_skip_small()
            };
            self.dashboard.min_days = min_days;
            self.dashboard.skip_small = skip_small;
            self.dashboard.buffer_size_mb = menu::prompt_buffer_size();
            self.dashboard.working_directory = directory.clone();

            let code = self.run_smart_refresh(&directory, min_days, skip_small);

            // 智能模式下扫描结果为空 ⇒ 不询问，直接回到菜单
            if code == 3 {
                continue;
            }

            if !menu::ask_return_to_menu() {
                break;
            }
        }

        0
    }

    /// 智能模式主体 — 交互与 CLI 共用
    /// 返回 0 正常完成 / 1 存在失败项 / 3 未发现符合条件的文件
    fn run_smart_refresh(&mut self, directory: &str, min_days: u32, skip_small: bool) -> i32 {
        logger().log(
            &format!(
                "用户配置: 目录='{}', 数据时效={}天, 跳过小文件={}, 缓冲区={}MB",
                directory, min_days, skip_small, self.dashboard.buffer_size_mb
            ),
            "INFO",
        );

        // 扫描文件（每 100 个条目刷新一次仪表盘，保持界面活跃）
        self.dashboard.update(&self.stats, "扫描中 (Scanning)");
        let mut files = file_op::collect_files(
            directory, min_days, skip_small,
            |_count| { self.dashboard.update(&self.stats, "扫描中 (Scanning)"); },
        );
        self.stats.scanned = files.len() as u64;

        if files.is_empty() {
            crate::terminal::Terminal::clear();
            let eq = "=".repeat(50);
            println!("\n{}", eq);
            println!("          未发现符合条件的冷数据文件 (No matching cold data files found)");
            println!("{}", eq);
            return 3;
        }

        // 按文件大小升序排序，先处理小文件，让进度条快速起步
        files.sort_by_cached_key(|f| std::fs::metadata(f).ok().map(|m| m.len()).unwrap_or(0));

        // 统计所有扫描文件分类，让分类数 = 扫描数
        for f in &files {
            if let Ok(meta) = std::fs::metadata(f) {
                self.update_file_stats(meta.len());
                self.stats.total_bytes += meta.len();
            }
        }

        // 扫描完成后立即刷新仪表盘，让用户看到文件数量和阶段切换
        self.dashboard.update(&self.stats, "扫描完成 (Scan done)");

        // 通知用户扫描结果，准备开始处理
        println!(
            "\n  → 发现 {} 个文件 ({})，正在处理中，请稍候... (Found {} files, processing...)",
            files.len(),
            crate::config::format_size(self.stats.total_bytes),
            files.len()
        );

        self.dashboard.update(&self.stats, "处理中 (Processing)");

        // 使用 rayon 分块并行处理，每块处理后实时更新进度
        use rayon::prelude::*;
        let buf_mult = (self.dashboard.buffer_size_mb as usize / 64).max(1);
        let chunk_size = (config::config().max_workers * buf_mult).max(1);
        let mini_batch = 32; // 每处理 32 个文件就更新一次进度，保证界面实时响应
        let mut processed = 0u64;
        let start = Instant::now();
        let total = files.len() as u64;

        for chunk in files.chunks(chunk_size) {
            if file_op::INTERRUPTED.load(Ordering::Relaxed) {
                break;
            }

            // 将大块拆成小批量，每批处理后更新进度，避免用户长时间看不到变化
            for batch in chunk.chunks(mini_batch) {
                if file_op::INTERRUPTED.load(Ordering::Relaxed) {
                    break;
                }

                // 并行处理当前小批量
                let results: Vec<(Result<u64, String>, u64)> = batch
                    .par_iter()
                    .map(|path| {
                        if file_op::INTERRUPTED.load(Ordering::Relaxed) {
                            return (Err("用户中断".into()), 0);
                        }
                        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                        let result = file_op::refresh_file(path);
                        (result, size)
                    })
                    .collect();

                // 汇总本小批结果
                for (result, file_size) in &results {
                    match result {
                        Ok(speed) => {
                            self.stats.processed += 1;
                            self.stats.processed_bytes += file_size;
                            if *speed > (self.stats.speed * 100.0) as u64 {
                                self.stats.speed = *speed as f64 / 100.0;
                            }
                        }
                        Err(_) => {
                            self.stats.corrupted += 1;
                        }
                    }
                    processed += 1;
                }
                // 每批立即刷新仪表盘，让用户看到实时进展
                self.stats.progress = if self.stats.total_bytes > 0 {
                    self.stats.processed_bytes as f64 / self.stats.total_bytes as f64
                } else {
                    processed as f64 / total as f64
                };
                self.stats.speed = self
                    .stats
                    .speed
                    .max(processed as f64 / start.elapsed().as_secs_f64().max(0.001) / 1_048_576.0);
                self.dashboard.update(&self.stats, "处理中 (Processing)");
            }
        }

        let elapsed = start.elapsed().as_secs_f64();

        // 汇总
        self.dashboard.update(&self.stats, "完成 (Done)");

        let log_path = config::config().error_log.display().to_string();
        self.dashboard
            .final_summary(&self.stats, elapsed, &log_path);

        logger().save_summary(
            self.stats.scanned,
            self.stats.processed,
            self.stats.corrupted,
            self.stats.large,
            self.stats.medium,
            self.stats.small,
            self.stats.speed,
            elapsed,
        );

        if self.stats.corrupted > 0 {
            1
        } else {
            0
        }
    }


    /// 更新文件分类统计
    fn update_file_stats(&mut self, file_size: u64) {
        match config::categorize_file(file_size) {
            FileCategory::Large => self.stats.large += 1,
            FileCategory::Medium => self.stats.medium += 1,
            FileCategory::Small => self.stats.small += 1,
        }
    }










    /// LBA 热力图采样 — 抓 block 层 IO，按 LBA 分桶统计冷热。
    /// `interactive`: true = 走交互视图（带色阶图）；false = 走 CLI 文本视图。
    /// 返回 true = 采样成功；false = 无可用后端或采样失败。
    fn run_heatmap(
        &self,
        directory: &str,
        bucket_mib: u64,
        hot_th: u64,
        warm_th: u64,
        interactive: bool,
    ) -> bool {
        crate::terminal::Terminal::clear();
        let eq = "=".repeat(50);
        println!("{}", eq);
        println!("          LBA 热力图模式 (LBA Heat Map Mode)");
        println!("{}", eq);
        println!("目标路径 (Target):     {}", directory);
        println!("分桶粒度 (Bucket):     {} MiB", bucket_mib);
        println!("热桶阈值 (Hot th):     {} 次", hot_th);
        println!("温桶阈值 (Warm th):    {} 次", warm_th);
        println!("采样后端 (Backend):    {}", {
            let b = crate::heatmap::probe_backend();
            format!("{:?}", b).to_lowercase()
        });
        println!("{}", eq);

        // 解析块设备
        let device = match platform::resolve_block_device(directory) {
            Some(d) => d,
            None => {
                eprintln!(
                    "错误 (Error): 无法从路径解析块设备 (cannot resolve block device): {}",
                    directory
                );
                return false;
            }
        };
        println!("块设备 (Block device): {}\n", device);

        // probe 动作：造少量 IO 以便采到 LBA（读目标目录 + 块设备）
        let probe_dir = directory.to_string();
        let probe = move || -> Result<(), String> {
            use std::io::Read;
            // 读目录下若干文件
            if let Ok(entries) = std::fs::read_dir(&probe_dir) {
                for e in entries.flatten().take(50) {
                    let p = e.path();
                    if p.is_file() {
                        if let Ok(mut f) = std::fs::File::open(&p) {
                            let mut buf = vec![0u8; 64 * 1024];
                            let _ = f.read(&mut buf);
                        }
                    }
                }
            }
            Ok(())
        };

        match crate::heatmap::sample(&device, bucket_mib, probe) {
            Ok(hm) => {
                // 交互入口与 CLI 入口分离（issue #8）：交互走带色阶图的视图，CLI 走文本视图
                if interactive {
                    crate::heatmap::print_heatmap_interactive(&hm, hot_th, warm_th);
                } else {
                    crate::heatmap::print_heatmap_cli(&hm, hot_th, warm_th);
                }
                let report = crate::heatmap::save_heatmap_report(&hm, hot_th, warm_th);
                println!("\n报告已保存 (Report): {}", report);
                true
            }
            Err(e) => {
                eprintln!("❌ 采样失败 (sampling failed): {}", e);
                false
            }
        }
    }

    /// 跑圈校验（CLI 路径）— 参数来自命令行，全程不读 stdin
    fn run_cycle_check_cli(
        &self,
        directory: &str,
        cycles: u64,
        unit_gb: u64,
        write_buf_kb: u64,
    ) -> bool {
        crate::terminal::Terminal::clear();
        let eq = "=".repeat(50);
        println!("{}", eq);
        println!("          跑圈校验模式 (Cycle Check Mode)");
        println!("{}", eq);
        println!("目标路径 (Target):     {}", directory);
        println!("圈数 (Cycles):         {}", cycles);
        println!("单元大小 (Unit):       {} GB (1GB 随机数据 + CRC32 校验码)", unit_gb);
        println!("写入缓冲区 (Buffer):   {} KB", write_buf_kb);
        println!("每圈流程 (Per cycle):  写入 → 校验 → 保存报告 → 删除 → TRIM");
        println!("{}", eq);

        let stats = crate::cycle_check::CycleCheck::execute(
            directory, cycles, unit_gb, write_buf_kb,
        );

        // 无任何圈完成 ⇒ 失败
        if stats.is_empty() {
            return false;
        }

        // 存在校验失败区块 ⇒ 返回非零（但报告已保存，便于定位坏块）
        let total_failed: u64 = stats.iter().map(|s| s.failed).sum();
        if total_failed > 0 {
            println!(
                "\n⚠️  共发现 {} 个校验失败区块，详见报告 (see report in logs/)",
                total_failed
            );
            false
        } else {
            println!("\n✅ 全部区块校验通过 (all blocks verified)");
            true
        }
    }

    /// TRIM 模式 — 返回真实执行结果（成功 true / 失败 false）
    fn run_trim_mode(&self, directory: &str) -> bool {
        logger().log(&format!("开始 TRIM 模式: 路径='{}'", directory), "INFO");

        let (result, device) = if let Some(dev) = platform::resolve_device_name(directory) {
            // 显示 TRIM 执行中界面
            crate::terminal::Terminal::clear();
            let eq = "=".repeat(50);
            println!("{}", eq);
            println!("          正在执行 TRIM 优化 (Running TRIM optimization)");
            println!("{}", eq);
            println!("设备 (Device):     {}", dev);
            println!("{}", eq);
            println!("注意事项 (Notes)：");
            println!("1. 后台执行时间预计需要10-30分钟 (expected 10-30 min)");
            println!("2. 30分钟内请不要对该存储设备进行断电操作 (do NOT cut power within 30 min)");
            println!("3. TRIM操作有助于提高SSD性能并延长使用寿命 (improves SSD performance/lifespan)");
            println!("{}", eq);
            print!("状态 (Status): 执行中 (running) ");
            std::io::stdout().flush().ok();

            // 用共享槽把工作线程的真实结果带回主线程（原实现丢弃了它）
            let d = dev.clone();
            let ok = menu::run_with_spinner("状态 (Status): 执行中 (running)", move || {
                platform::trim_volume(&d)
            });
            logger().log(
                &format!("TRIM 操作完成: 设备={}, 成功={}", dev, ok),
                "INFO",
            );
            (ok, dev)
        } else {
            println!(
                "无法从路径中识别存储设备 (Cannot resolve device from path): '{}'",
                directory
            );
            (false, directory.to_string())
        };

        // 显示 TRIM 完成汇总
        std::thread::sleep(std::time::Duration::from_millis(500));
        crate::terminal::Terminal::clear();
        let eq = "=".repeat(50);
        println!("\n{}", eq);
        println!("          TRIM 操作完成 (TRIM Completed)");
        println!("{}", eq);
        println!("设备 (Device):     {}", device);
        println!(
            "状态 (Status):     {}",
            if result { "✅ 成功 (Success)" } else { "❌ 失败 (Failed)" }
        );
        println!("操作日志 (Log): {}", crate::config::config().error_log.display());
        println!("{}", eq);

        result
    }

    /// 全盘刷新（交互模式）— 通过 stdin 询问参数，返回真实执行结果
    fn run_full_refresh_mode(&self, directory: &str) -> bool {
        let params = menu::prompt_full_refresh_params();
        self.execute_full_refresh(directory, params)
    }

    /// 全盘刷新（CLI 模式）— 参数来自命令行，全程不读 stdin
    fn run_full_refresh_cli(&self, directory: &str, opts: &RunOptions) -> bool {
        // -cli 下必须显式指定是否保留文件
        let keep = if opts.keep_files {
            true
        } else if opts.no_keep_files {
            false
        } else {
            eprintln!(
                "错误 (Error): 全盘刷新在 -cli 下必须显式指定 --keep-files 或 --no-keep-files (keep/no-keep required)"
            );
            return false;
        };

        let params = crate::full_refresh::FullRefreshParams {
            keep_files: keep,
            fill_free: opts.fill_free,
            unit_gb: opts.unit_gb.clamp(1, 100),
            write_buf_kb: opts.write_buf_kb.clamp(64, 1024 * 1024),
        };
        self.execute_full_refresh(directory, params)
    }


    /// 全盘刷新执行核心 — 交互与 CLI 共用，全程无 stdin
    fn execute_full_refresh(
        &self,
        directory: &str,
        params: crate::full_refresh::FullRefreshParams,
    ) -> bool {
        crate::terminal::Terminal::clear();
        logger().log(&format!("开始全盘刷新模式: 路径='{}'", directory), "INFO");

        let is_drive = platform::is_root_path(directory);
        let eq = "=".repeat(50);
        println!("{}", eq);
        println!("          全盘刷新模式 (Full Refresh Mode)");
        println!("{}", eq);
        println!(
            "路径类型 (Path type): {}",
            if is_drive {
                "整个盘符 (whole drive)"
            } else {
                "文件目录 (directory)"
            }
        );

        // 计算目录内文件总大小（递归）
        println!("\n正在统计目录大小... (Calculating directory size...)");
        let dir_size: u64 = walkdir::WalkDir::new(directory)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum();

        // 获取硬盘可用空间
        let (_total, _used, free) = platform::get_disk_space(std::path::Path::new(directory));
        println!("目录信息 (Directory info):");
        println!("  目录大小 (size): {}", config::format_size(dir_size));
        println!("  可用空间 (free): {}", config::format_size(free));

        // 回显本次参数（替代原先的交互提问）
        println!("  保留文件 (keep files): {}", if params.keep_files { "是/yes" } else { "否/no" });
        println!("  填充空闲空间 (fill free): {}", if params.fill_free { "是/yes" } else { "否/no" });
        if params.fill_free {
            println!("  写入容量 (unit): {} GB", params.unit_gb);
            println!("  写入缓冲区 (buffer): {} KB", params.write_buf_kb);
        }

        // 执行全盘刷新（目录空间始终填充，空闲空间由 fill_free 控制）
        FullRefresh::execute(
            directory,
            params.keep_files,
            params.fill_free,
            dir_size,
            params.unit_gb,
            params.write_buf_kb,
        )
    }
}
