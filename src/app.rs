//! 应用控制器 —— CLI 非交互模式与交互菜单模式的流程编排。

use std::io::{self, Write};
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::config::{self, FileCategory};
use crate::dashboard::{Dashboard, Stats};
use crate::file_op;
use crate::full_refresh::FullRefresh;
use crate::log::logger;
use crate::platform;

//// ========================== CLI 模式 (CLI mode) ==========================
/// 运行参数 — 由 CLI 解析结果注入
pub struct RunOptions {
    pub cli: bool,
    pub path: Option<String>,
    pub full_refresh: bool,
    pub trim: bool,
    pub age: Option<u32>,
    pub skip_smaller: Option<u64>,
    pub yes: bool,
    pub buffer_size_mb: Option<u32>,
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

        // 3) 智能模式必须有 -a
        if !opts.full_refresh && !opts.trim && opts.age.is_none() {
            eprintln!("错误 (Error): 智能模式必须指定数据时效 (data age required) -a <天数/days>");
            return 2;
        }

        // 4) 破坏性操作必须显式 -y
        if (opts.full_refresh || opts.trim) && !opts.yes {
            eprintln!(
                "错误 (Error): {} 模式会破坏数据，-cli 下必须显式加 -y 确认 (destructive, -y required)",
                if opts.full_refresh { "全盘刷新 (full-refresh)" } else { "TRIM" }
            );
            return 2;
        }

        self.dashboard.full_refresh = opts.full_refresh;
        self.dashboard.trim_mode = opts.trim;
        self.dashboard.working_directory = directory.clone();

        // TRIM
        if opts.trim {
            let ok = self.run_trim_mode(&directory);
            return if ok { 0 } else { 1 };
        }

        // 全盘刷新
        if opts.full_refresh {
            self.run_full_refresh_mode(&directory);
            return 0;
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
        let cli_age = opts.age;
        let cli_skip_smaller_mb = opts.skip_smaller;

        // 打印启动横幅
        self.show_startup_banner();

        loop {
            // 重置统计
            self.stats = Stats::default();

            // 获取模式选择
            let (mode_full, mode_trim) = if !full_refresh && !trim_mode {
                self.show_mode_menu()
            } else {
                (full_refresh, trim_mode)
            };

            // 确认操作
            if mode_full {
                if !self.confirm_full_refresh() {
                    continue;
                }
            } else if mode_trim && !self.confirm_trim() {
                continue;
            }

            self.dashboard.full_refresh = mode_full;
            self.dashboard.trim_mode = mode_trim;

            // 获取目录
            let directory = self.prompt_directory();

            // TRIM 模式 — 直接执行
            if mode_trim {
                self.run_trim_mode(&directory);
                if !self.ask_return_to_menu() {
                    break;
                }
                continue;
            }

            // 全盘刷新模式
            if mode_full {
                self.run_full_refresh_mode(&directory);
                if !self.ask_return_to_menu() {
                    break;
                }
                continue;
            }

            // 智能模式
            let min_days = cli_age.unwrap_or_else(|| self.prompt_age());
            let skip_small = if cli_skip_smaller_mb.unwrap_or(0) > 0 {
                true
            } else {
                self.prompt_skip_small()
            };
            self.dashboard.min_days = min_days;
            self.dashboard.skip_small = skip_small;
            self.dashboard.buffer_size_mb = self.prompt_buffer_size();
            self.dashboard.working_directory = directory.clone();

            let code = self.run_smart_refresh(&directory, min_days, skip_small);

            // 智能模式下扫描结果为空 ⇒ 不询问，直接回到菜单
            if code == 3 {
                continue;
            }

            if !self.ask_return_to_menu() {
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

    /// 打印启动横幅
    fn show_startup_banner(&self) {
        let admin = if platform::is_admin() {
            " [管理员/Admin]"
        } else {
            ""
        };
        let os_info = platform::get_os_display();
        let eq = "=".repeat(50);

        println!("{}", eq);
        println!("SSD掉速激活-冷数据维护系统 v5.0.2{}", admin);
        println!("运行平台 (Platform): {}", os_info);
        println!("作者 (Author): support@e2bank.cn  QQ群 (Group): 115405294");
        println!("GitHub: https://github.com/aspnmy/ColDataRefresh");
        println!("{}", eq);
    }

    /// 更新文件分类统计
    fn update_file_stats(&mut self, file_size: u64) {
        match config::categorize_file(file_size) {
            FileCategory::Large => self.stats.large += 1,
            FileCategory::Medium => self.stats.medium += 1,
            FileCategory::Small => self.stats.small += 1,
        }
    }

    fn show_mode_menu(&self) -> (bool, bool) {
        crate::terminal::Terminal::clear();
        let admin = if platform::is_admin() { " [管理员/Admin]" } else { "" };
        let os_info = platform::get_os_display();
        let eq = "=".repeat(50);

        println!("{}", eq);
        println!("SSD掉速激活-冷数据维护系统 v5.0.2{}", admin);
        println!("运行平台 (Platform): {}  |  作者 (Author): support@e2bank.cn  QQ群 (Group): 115405294", os_info);
        println!("GitHub: https://github.com/aspnmy/ColDataRefresh");
        println!("{}", eq);

        println!("\n{}", eq);
        println!("          冷数据维护工具 - 操作模式选择 (Cold Data Tool - Mode Selection)");
        println!("{}", eq);
        println!("1. 智能模式 (Smart Mode) (推荐/Recommended) - 保留原文件内容，仅激活冷数据 (keep contents, refresh cold data only)");
        println!("2. 全盘刷新模式 (Full Refresh Mode) (所有文件全部丢失无法找回/ALL DATA LOST, UNRECOVERABLE) - 将文件内容替换为 FF 值 (overwrite with 0xFF)");
        println!("3. TRIM优化模式 (TRIM Optimization Mode) (如需找回数据不要使用/CANNOT recover deleted data) - 通知SSD哪些数据块无效 (notify SSD which blocks are invalid)");
        println!("{}", eq);

        loop {
            print!("请选择操作模式 (Select mode) [1/2/3]: ");
            io::stdout().flush().ok();
            let mut input = String::new();
            io::stdin().read_line(&mut input).ok();
            match input.trim() {
                "1" => {
                    logger().log("用户选择操作模式: 智能模式", "INFO");
                    return (false, false);
                }
                "2" => {
                    logger().log("用户选择操作模式: 全盘刷新模式", "INFO");
                    return (true, false);
                }
                "3" => {
                    logger().log("用户选择操作模式: TRIM模式", "INFO");
                    return (false, true);
                }
                _ => println!("无效的选择，请输入 1、2 或 3 (Invalid choice, enter 1, 2 or 3)"),
            }
        }
    }

    fn confirm_full_refresh(&self) -> bool {
        println!("\n⚠️  警告 (WARNING): 正在使用全盘数据刷新模式 (Full Refresh Mode)！");
        println!("   此模式将完全擦除SSD数据，所有文件内容将丢失且无法找回 (erases all data, unrecoverable)！");
        println!();

        print!("请输入 'yes' 确认执行全盘刷新操作 (第一次) (type 'yes' to confirm, 1st): ");
        io::stdout().flush().ok();
        let mut c1 = String::new();
        io::stdin().read_line(&mut c1).ok();
        if c1.trim().to_lowercase() != "yes" {
            println!("操作已取消 (Cancelled)");
            return false;
        }

        print!("请再次输入 'yes' 确认执行全盘刷新操作 (第二次) (type 'yes' again, 2nd): ");
        io::stdout().flush().ok();
        let mut c2 = String::new();
        io::stdin().read_line(&mut c2).ok();
        if c2.trim().to_lowercase() != "yes" {
            println!("操作已取消 (Cancelled)");
            return false;
        }

        logger().log("用户已确认两次，开始执行全盘刷新操作", "INFO");
        true
    }

    fn confirm_trim(&self) -> bool {
        println!("\n⚠️  警告 (WARNING): 正在使用TRIM优化模式 (TRIM Optimization)！");
        println!("   如需找回SSD中删除的数据请勿使用此模式 (do NOT use if you need to recover data)！");
        println!();

        print!("请输入 'yes' 确认 (type 'yes' to confirm): ");
        io::stdout().flush().ok();
        let mut c = String::new();
        io::stdin().read_line(&mut c).ok();
        if c.trim().to_lowercase() != "yes" {
            println!("操作已取消 (Cancelled)");
            return false;
        }

        logger().log("用户已确认，开始执行TRIM优化操作", "INFO");
        true
    }

    fn prompt_directory(&mut self) -> String {
        print!("扫描目录 (Scan directory): ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        let dir = input.trim().trim_matches('"').to_string();

        // 补齐路径分隔符
        let dir = if !dir.ends_with(&['\\', '/'][..]) {
            dir + "\\"
        } else {
            dir
        };

        self.dashboard.working_directory = dir.clone();
        dir
    }

    fn prompt_age(&self) -> u32 {
        print!("数据时效/天 (Data age in days): ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        input.trim().parse().unwrap_or(0)
    }

    fn prompt_skip_small(&self) -> bool {
        print!("跳过小文件? (Skip small files?) (y/n): ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        input.trim().to_lowercase() == "y"
    }

    fn prompt_buffer_size(&self) -> u32 {
        print!("处理缓冲区大小/MB (Buffer size, default 512, max 2048): ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        let val: u32 = input.trim().parse().unwrap_or(512);
        val.clamp(64, 2048)
    }

    fn ask_return_to_menu(&self) -> bool {
        crate::terminal::Terminal::clear();
        println!("\n{}", "=".repeat(50));
        println!("1. 返回 - 回到交互界面 (Back to menu)");
        println!("2. 退出 - 关闭程序 (Exit)");
        println!("{}", "=".repeat(50));

        loop {
            print!("请选择操作 (Select action) [1/2]: ");
            io::stdout().flush().ok();
            let mut input = String::new();
            io::stdin().read_line(&mut input).ok();
            match input.trim() {
                "1" => return true,
                "2" => {
                    println!("感谢使用冷数据维护工具，再见！(Thanks for using, goodbye!)");
                    return false;
                }
                _ => println!("无效的选择，请输入 1 或 2 (Invalid choice, enter 1 or 2)"),
            }
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
            use std::io::Write;
            std::io::stdout().flush().ok();

            // 在后台线程执行 TRIM，主线程显示旋转动画
            use std::sync::atomic::{AtomicBool, Ordering};
            use std::sync::{Arc, Mutex};
            let done = Arc::new(AtomicBool::new(false));
            let done_clone = done.clone();
            // 用共享槽把工作线程的真实结果带回主线程（原实现丢弃了它）
            let outcome = Arc::new(Mutex::new(false));
            let outcome_clone = outcome.clone();
            let d = dev.clone();
            let _handle = std::thread::spawn(move || {
                let ok = platform::trim_volume(&d);
                if let Ok(mut slot) = outcome_clone.lock() {
                    *slot = ok;
                }
                done_clone.store(true, Ordering::Relaxed);
            });

            let spinner = ['|', '/', '-', '\\'];
            let mut i = 0;
            while !done.load(Ordering::Relaxed) {
                print!("\r状态 (Status): 执行中 (running) {} 请耐心等待 (please wait)...", spinner[i % 4]);
                std::io::stdout().flush().ok();
                std::thread::sleep(std::time::Duration::from_millis(500));
                i += 1;
            }

            let ok = *outcome.lock().unwrap_or_else(|e| e.into_inner());
            logger().log(
                &format!("TRIM 操作完成: 设备={}, 成功={}", dev, ok),
                "INFO",
            );
            println!(
                "\r状态 (Status): {}                    ",
                if ok { "✅ 完成 (Done)" } else { "❌ 失败 (Failed)" }
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

    fn run_full_refresh_mode(&self, directory: &str) {
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

        // 询问是否保留文件（总是需要）
        print!("\n是否保留已使用空间中的文件? (Keep existing files?) (Y/N, 默认/default Y): ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        let keep = !input.trim().to_lowercase().starts_with('n');

        // 询问是否额外填充空闲空间
        print!("\n是否同时填充空闲空间? (Also fill free space?) (Y/N, 默认/default N): ");
        io::stdout().flush().ok();
        let mut fill_input = String::new();
        io::stdin().read_line(&mut fill_input).ok();
        let want_fill = fill_input.trim().to_lowercase().starts_with('y');

        // 如果用户选择填充，再次确认
        let fill_free = if want_fill {
            print!("\n⚠️  填充空闲空间将覆写所有未分配空间，数据无法还原 (unrecoverable)！\n是否确认执行? (Confirm?) (Y/N, 默认/default N): ");
            io::stdout().flush().ok();
            let mut confirm = String::new();
            io::stdin().read_line(&mut confirm).ok();
            confirm.trim().to_lowercase().starts_with('y')
        } else {
            false
        };

        // 询问写入参数（仅填充空闲空间时需要）
        let (unit_gb, write_buf_kb) = if fill_free {
            print!("\n请输入每个文件的写入容量 (Write size per file) (1-100GB, 默认/default 50GB): ");
            io::stdout().flush().ok();
            let mut cap = String::new();
            io::stdin().read_line(&mut cap).ok();
            let val: u64 = cap.trim().parse().unwrap_or(50);
            let unit_gb = val.clamp(1, 100);

            print!("\n请输入写入缓冲区大小 (Write buffer size) (64KB~1GB, 默认/default 512MB): ");
            io::stdout().flush().ok();
            let mut buf_input = String::new();
            io::stdin().read_line(&mut buf_input).ok();
            let buf_val: u64 = buf_input.trim().parse().unwrap_or(512);
            let write_buf_kb = buf_val.clamp(1, 1024) * 1024;

            (unit_gb, write_buf_kb)
        } else {
            (0, 64)
        };

        // 执行全盘刷新（目录空间始终填充，空闲空间由 fill_free 控制）
        FullRefresh::execute(directory, keep, fill_free, dir_size, unit_gb, write_buf_kb);
    }
}
