use std::io::{self, Write};
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::config::{self, FileCategory};
use crate::dashboard::{Dashboard, Stats};
use crate::file_op;
use crate::full_refresh::FullRefresh;
use crate::log::logger;
use crate::platform;

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

    /// 主入口
    pub fn run(
        &mut self,
        full_refresh: bool,
        trim_mode: bool,
        cli_age: Option<u32>,
        cli_skip_smaller_mb: Option<u64>,
    ) {
        // 注册 Ctrl+C 信号处理器
        let _ = ctrlc::set_handler(|| {
            // 先设置中断标志，让当前操作优雅退出
            file_op::INTERRUPTED.store(true, Ordering::Relaxed);
            // 短暂延迟后直接退出，避免阻塞在 stdin 读上
            std::thread::sleep(std::time::Duration::from_millis(200));
            std::process::exit(0);
        });

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

            logger().log(
                &format!(
                    "用户配置: 目录='{}', 数据时效={}天, 跳过小文件={}, 缓冲区={}MB",
                    directory, min_days, skip_small, self.dashboard.buffer_size_mb
                ),
                "INFO",
            );

            // 扫描文件（每 100 个条目刷新一次仪表盘，保持界面活跃）
            self.dashboard.update(&self.stats, "扫描中");
            let mut files = file_op::collect_files(
                &directory, min_days, skip_small,
                |_count| { self.dashboard.update(&self.stats, "扫描中"); },
            );
            self.stats.scanned = files.len() as u64;

            if files.is_empty() {
                crate::terminal::Terminal::clear();
                let eq = "=".repeat(50);
                println!("\n{}", eq);
                println!("          未发现符合条件的冷数据文件");
                println!("{}", eq);
                if !self.ask_return_to_menu() {
                    break;
                }
                continue;
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
            self.dashboard.update(&self.stats, "扫描完成");

            // 通知用户扫描结果，准备开始处理
            println!(
                "\n  → 发现 {} 个文件 ({})，正在处理中，请稍候...",
                files.len(),
                crate::config::format_size(self.stats.total_bytes)
            );

            self.dashboard.update(&self.stats, "处理中");

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
                    self.dashboard.update(&self.stats, "处理中");
                }
            }

            let elapsed = start.elapsed().as_secs_f64();

            // 汇总
            self.dashboard.update(&self.stats, "完成");

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

            if !self.ask_return_to_menu() {
                break;
            }
        }
    }

    /// 打印启动横幅
    fn show_startup_banner(&self) {
        let admin = if platform::is_admin() {
            " [管理员]"
        } else {
            ""
        };
        let os_info = platform::get_os_display();
        let eq = "=".repeat(50);

        println!("{}", eq);
        println!("SSD掉速激活-冷数据维护系统 v5.0.0{}", admin);
        println!("运行平台: {}", os_info);
        println!("作者: support@e2bank.cn  QQ群: 115405294");
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
        let admin = if platform::is_admin() { " [管理员]" } else { "" };
        let os_info = platform::get_os_display();
        let eq = "=".repeat(50);

        println!("{}", eq);
        println!("SSD掉速激活-冷数据维护系统 v5.0.0{}", admin);
        println!("运行平台: {}  |  作者: support@e2bank.cn  QQ群: 115405294", os_info);
        println!("GitHub: https://github.com/aspnmy/ColDataRefresh");
        println!("{}", eq);

        println!("\n{}", eq);
        println!("          冷数据维护工具 - 操作模式选择");
        println!("{}", eq);
        println!("1. 智能模式 (推荐) - 保留原文件内容，仅激活冷数据");
        println!("2. 全盘激活冷数据模式 (所有文件全部丢失无法找回) - 将文件内容替换为 FF 值");
        println!("3. TRIM优化模式 (清理/如需找回数据不要使用这个模式) - 操作系统API来通知SSD哪些数据块是无效的");
        println!("{}", eq);

        loop {
            print!("请选择操作模式 [1/2/3]: ");
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
                _ => println!("无效的选择，请输入 1、2 或 3"),
            }
        }
    }

    fn confirm_full_refresh(&self) -> bool {
        println!("\n⚠️  警告: 正在使用全盘数据刷新模式！");
        println!("   使用此模式将完全擦除SSD硬盘中的数据，所有文件内容将丢失且无法找回！");
        println!();

        print!("请输入 'yes' 确认执行全盘刷新操作 (第一次): ");
        io::stdout().flush().ok();
        let mut c1 = String::new();
        io::stdin().read_line(&mut c1).ok();
        if c1.trim().to_lowercase() != "yes" {
            println!("操作已取消");
            return false;
        }

        print!("请再次输入 'yes' 确认执行全盘刷新操作 (第二次): ");
        io::stdout().flush().ok();
        let mut c2 = String::new();
        io::stdin().read_line(&mut c2).ok();
        if c2.trim().to_lowercase() != "yes" {
            println!("操作已取消");
            return false;
        }

        logger().log("用户已确认两次，开始执行全盘刷新操作", "INFO");
        true
    }

    fn confirm_trim(&self) -> bool {
        println!("\n⚠️  警告: 正在使用TRIM优化模式！");
        println!("   如需找回SSD中删除的数据请不要使用此模式，先找回数据以后再使用！");
        println!();

        print!("请输入 'yes' 确认执行TRIM优化操作: ");
        io::stdout().flush().ok();
        let mut c = String::new();
        io::stdin().read_line(&mut c).ok();
        if c.trim().to_lowercase() != "yes" {
            println!("操作已取消");
            return false;
        }

        logger().log("用户已确认，开始执行TRIM优化操作", "INFO");
        true
    }

    fn prompt_directory(&mut self) -> String {
        print!("扫描目录: ");
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
        print!("数据时效(天): ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        input.trim().parse().unwrap_or(0)
    }

    fn prompt_skip_small(&self) -> bool {
        print!("跳过小文件? (y/n): ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        input.trim().to_lowercase() == "y"
    }

    fn prompt_buffer_size(&self) -> u32 {
        print!("处理缓冲区大小(MB, 默认512, 最大2048): ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        let val: u32 = input.trim().parse().unwrap_or(512);
        val.clamp(64, 2048)
    }

    fn ask_return_to_menu(&self) -> bool {
        crate::terminal::Terminal::clear();
        println!("\n{}", "=".repeat(50));
        println!("1. 返回 - 回到交互界面");
        println!("2. 退出 - 关闭程序");
        println!("{}", "=".repeat(50));

        loop {
            print!("请选择操作 [1/2]: ");
            io::stdout().flush().ok();
            let mut input = String::new();
            io::stdin().read_line(&mut input).ok();
            match input.trim() {
                "1" => return true,
                "2" => {
                    println!("感谢使用冷数据维护工具，再见！");
                    return false;
                }
                _ => println!("无效的选择，请输入 1 或 2"),
            }
        }
    }

    fn run_trim_mode(&self, directory: &str) {
        logger().log(&format!("开始 TRIM 模式: 路径='{}'", directory), "INFO");

        let (result, device) = if let Some(dev) = platform::resolve_device_name(directory) {
            // 显示 TRIM 执行中界面
            crate::terminal::Terminal::clear();
            let eq = "=".repeat(50);
            println!("{}", eq);
            println!("          正在执行 TRIM 优化");
            println!("{}", eq);
            println!("设备:     {}", dev);
            println!("{}", eq);
            println!("注意事项：");
            println!("1. 后台执行时间预计需要10-30分钟");
            println!("2. 30分钟内请不要对该存储设备进行断电操作");
            println!("3. TRIM操作有助于提高SSD性能并延长使用寿命");
            println!("{}", eq);
            print!("状态: 执行中 ");
            use std::io::Write;
            std::io::stdout().flush().ok();

            // 在后台线程执行 TRIM，主线程显示旋转动画
            use std::sync::atomic::{AtomicBool, Ordering};
            let done = std::sync::Arc::new(AtomicBool::new(false));
            let done_clone = done.clone();
            let d = dev.clone();
            let _handle = std::thread::spawn(move || {
                let _ = platform::trim_volume(&d);
                done_clone.store(true, Ordering::Relaxed);
            });

            let spinner = ['|', '/', '-', '\\'];
            let mut i = 0;
            while !done.load(Ordering::Relaxed) {
                print!("\r状态: 执行中 {} 请耐心等待...", spinner[i % 4]);
                std::io::stdout().flush().ok();
                std::thread::sleep(std::time::Duration::from_millis(500));
                i += 1;
            }

            let ok = true; // trim_volume already logged the result
            logger().log(&format!("TRIM 操作完成: 设备={}", dev), "INFO");
            println!("\r状态: ✅ 完成                         ");
            (ok, dev)
        } else {
            println!("无法从路径 '{}' 中识别存储设备", directory);
            (false, directory.to_string())
        };

        // 显示 TRIM 完成汇总
        std::thread::sleep(std::time::Duration::from_millis(500));
        crate::terminal::Terminal::clear();
        let eq = "=".repeat(50);
        println!("\n{}", eq);
        println!("          TRIM 操作完成");
        println!("{}", eq);
        println!("设备:     {}", device);
        println!("状态:     {}", if result { "✅ 成功" } else { "❌ 失败" });
        println!("操作日志: {}", crate::config::config().error_log.display());
        println!("{}", eq);
    }

    fn run_full_refresh_mode(&self, directory: &str) {
        crate::terminal::Terminal::clear();
        logger().log(&format!("开始全盘刷新模式: 路径='{}'", directory), "INFO");

        let is_drive = platform::is_root_path(directory);
        let eq = "=".repeat(50);
        println!("{}", eq);
        println!("          全盘刷新模式");
        println!("{}", eq);
        println!(
            "路径类型: {}",
            if is_drive {
                "整个盘符"
            } else {
                "文件目录"
            }
        );

        // 计算目录内文件总大小（递归）
        println!("\n正在统计目录大小...");
        let dir_size: u64 = walkdir::WalkDir::new(directory)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum();

        // 获取硬盘可用空间
        let (_total, _used, free) = platform::get_disk_space(std::path::Path::new(directory));
        println!("目录信息:");
        println!("  目录大小: {}", config::format_size(dir_size));
        println!("  可用空间: {}", config::format_size(free));

        // 询问是否保留文件（总是需要）
        print!("\n是否保留已使用空间中的文件？ (Y/N, 默认Y): ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        let keep = !input.trim().to_lowercase().starts_with('n');

        // 询问是否额外填充空闲空间
        print!("\n是否同时填充空闲空间？ (Y/N, 默认N): ");
        io::stdout().flush().ok();
        let mut fill_input = String::new();
        io::stdin().read_line(&mut fill_input).ok();
        let want_fill = fill_input.trim().to_lowercase().starts_with('y');

        // 如果用户选择填充，再次确认
        let fill_free = if want_fill {
            print!("\n⚠️  填充空闲空间将覆写所有未分配空间，数据无法还原！\n是否确认执行？ (Y/N, 默认N): ");
            io::stdout().flush().ok();
            let mut confirm = String::new();
            io::stdin().read_line(&mut confirm).ok();
            confirm.trim().to_lowercase().starts_with('y')
        } else {
            false
        };

        // 询问写入参数（仅填充空闲空间时需要）
        let (unit_gb, write_buf_kb) = if fill_free {
            print!("\n请输入每个文件的写入容量 (1-100GB，默认50GB): ");
            io::stdout().flush().ok();
            let mut cap = String::new();
            io::stdin().read_line(&mut cap).ok();
            let val: u64 = cap.trim().parse().unwrap_or(50);
            let unit_gb = val.clamp(1, 100);

            print!("\n请输入写入缓冲区大小 (64KB~1GB，默认512MB): ");
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
