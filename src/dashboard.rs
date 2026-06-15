use std::time::{Duration, Instant};

use crate::config;
use crate::terminal::{self, Terminal};

/// 操作统计
#[derive(Debug, Default, Clone)]
pub struct Stats {
    pub scanned: u64,
    pub processed: u64,
    pub total_bytes: u64,
    pub processed_bytes: u64,
    pub large: u64,
    pub medium: u64,
    pub small: u64,
    pub corrupted: u64,
    pub speed: f64,
    pub progress: f64,
}

/// 进度仪表盘
pub struct Dashboard {
    start_time: Instant,
    last_update: Instant,
    last_phase: String,
    pub working_directory: String,
    pub full_refresh: bool,
    pub trim_mode: bool,
    pub min_days: u32,
    pub skip_small: bool,
    pub buffer_size_mb: u32,
    sub_progress: f64,
}

impl Dashboard {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            last_update: Instant::now(),
            last_phase: String::new(),
            working_directory: String::new(),
            full_refresh: false,
            trim_mode: false,
            min_days: 0,
            skip_small: false,
            buffer_size_mb: 512,
            sub_progress: 0.0,
        }
    }

    /// 更新显示（带频率控制，阶段变化时强制刷新）
    pub fn update(&mut self, stats: &Stats, phase: &str) {
        let cfg = config::config();
        let phase_changed = phase != self.last_phase;
        if !phase_changed && self.last_update.elapsed() < Duration::from_secs_f32(cfg.report_interval_secs) {
            return;
        }

        self.last_phase = phase.to_string();

        // 子进度条独立于实际处理，按时间循环，每 1.5 秒完成一次 0→100%
        let elapsed = self.start_time.elapsed().as_secs_f64();
        self.sub_progress = (elapsed / 1.5) % 1.0;

        Terminal::clear();
        self.render_header();
        self.render_stats(stats, phase);
        use std::io::Write;
        std::io::stdout().flush().ok();
        self.last_update = Instant::now();
    }

    fn render_header(&self) {
        let (h, _v, _) = terminal::border_chars();
        let h_line = h.repeat(70);

        let header = Terminal::colored(
            " SSD掉速激活-冷数据维护系统 v5.0.0 作者:support@e2bank.cn By Rust",
            37,
            44,
        );

        println!("\n{}", h_line);
        println!("{:^70}", header);
        println!("{}", h_line);
    }

    fn render_stats(&self, stats: &Stats, phase: &str) {
        let (_h, v, _) = terminal::border_chars();
        let (fill, empty) = terminal::progress_chars();
        let elapsed = self.start_time.elapsed().as_secs_f64();

        let mode_text = if self.full_refresh {
            "全盘刷新模式"
        } else if self.trim_mode {
            "TRIM模式"
        } else {
            "智能模式"
        };

        let progress_bar = {
            let filled = (50.0 * stats.progress) as usize;
            let unfilled = 50_usize.saturating_sub(filled);
            format!("[{}{}]", fill.repeat(filled), empty.repeat(unfilled))
        };

        let lines = [
            format!("{} 智能检测固态硬盘的冷数据并解决冷数据掉速问题。", v),
            format!("{} GitHub: https://github.com/aspnmy/ColDataRefresh.git", v),
            format!("{} 工作路径: {}  ", v, self.working_directory),
            format!("{} 操作模式: {}  ", v, Terminal::colored(mode_text, 32, 44)),
            format!(
                "{} 数据时效: {} 天, 跳过小文件: {}  ",
                v,
                self.min_days,
                if self.skip_small { "是" } else { "否" }
            ),
            format!(
                "{} 缓冲区: {} MB",
                v, self.buffer_size_mb
            ),
            format!(
                "{} 数据量: {}/{}",
                v,
                crate::config::format_size(stats.processed_bytes),
                crate::config::format_size(stats.total_bytes.max(stats.processed_bytes)),
            ),
            format!(
                "{} 运行阶段: {} 耗时: {:.1}s",
                v,
                Terminal::fg(phase, 33),
                elapsed
            ),
            format!(
                "{} 处理进度: {} {:.1}%",
                v,
                progress_bar,
                stats.progress * 100.0
            ),
            format!(
                "{} 文件进度: [{}]",
                v,
                "▓".repeat((self.sub_progress * 20.0) as usize)
                    + &"░".repeat(20_usize.saturating_sub((self.sub_progress * 20.0) as usize)),
            ),
            format!("{} 发现文件: {}", v, stats.scanned),
            format!("{} 处理速度: {:.1} MB/s", v, stats.speed),
            format!(
                "{} 文件分类: 大(>100MB)({}) 中(10-100MB)({}) 小(<10MB)({})",
                v, stats.large, stats.medium, stats.small
            ),
            format!(
                "{} 损坏的文件: {}",
                v,
                Terminal::colored(&stats.corrupted.to_string(), 31, 44)
            ),
            format!("{} 按 Ctrl+C 退出程序", v),
        ];

        for line in &lines {
            println!("{}", line);
        }

        let (h, _, _) = terminal::border_chars();
        println!("{}{}", h, h.repeat(69));
    }

    /// 清理并显示最终结果
    pub fn final_summary(&self, stats: &Stats, elapsed: f64, log_file: &str) {
        Terminal::clear();
        let eq = "=".repeat(60);

        println!("\n{}", eq);
        println!("          操作完成！");
        println!("{}", eq);

        if self.full_refresh {
            println!("操作模式: 全盘刷新");
            println!("总耗时: {:.2} 秒", elapsed);
            println!("最大写入速度: {:.2} MB/s", stats.speed);
        } else if self.trim_mode {
            println!("操作模式: TRIM模式");
            println!("总耗时: {:.2} 秒", elapsed);
        } else {
            println!("操作模式: 智能模式");
            println!("总耗时: {:.2} 秒", elapsed);
            println!(
                "处理文件数: {} 个 (共发现 {} 个)",
                stats.processed, stats.scanned
            );
            println!(
                "大文件: {}, 中等文件: {}, 小文件: {}",
                stats.large, stats.medium, stats.small
            );
            println!("损坏文件: {}", stats.corrupted);
            println!("平均处理速度: {:.2} MB/s", stats.speed);
        }

        println!("操作日志: {}", log_file);
        println!("错误记录: {}", config::config().corrupted_log.display());
        println!("{}", eq);
    }
}

/// 计算扫描速度 — 公开 API，预留使用
#[allow(dead_code)]
pub fn compute_scan_speed(scanned: u64, elapsed_secs: f64) -> f64 {
    if elapsed_secs > 0.0 {
        scanned as f64 / (1024.0 * 1024.0) / elapsed_secs
    } else {
        0.0
    }
}
