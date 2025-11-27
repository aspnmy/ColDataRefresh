use std::time::{SystemTime, UNIX_EPOCH};

use crate::terminal_manager::TerminalManager;

/// 操作统计结构体
pub struct OperationStats {
    /// 已扫描文件总数
    pub scanned: u64,
    /// 已处理文件数
    pub processed: u64,
    /// 大文件数量
    pub large: u64,
    /// 中等文件数量
    pub medium: u64,
    /// 小文件数量
    pub small: u64,
    /// 损坏文件数量
    pub corrupted: u64,
    /// 处理速度
    pub speed: f64,
    /// 总体进度百分比
    pub progress: f64,
}

impl Default for OperationStats {
    fn default() -> Self {
        Self {
            scanned: 0,
            processed: 0,
            large: 0,
            medium: 0,
            small: 0,
            corrupted: 0,
            speed: 0.0,
            progress: 0.0,
        }
    }
}

/// 仪表盘，负责显示程序的运行状态和进度
pub struct Dashboard {
    /// 终端管理器
    terminal: &'static mut TerminalManager,
    /// 开始时间
    start_time: SystemTime,
    /// 上次更新时间
    last_update: SystemTime,
    /// 上次扫描的文件数
    last_scanned: u64,
    /// 当前工作路径
    working_directory: String,
    /// 全盘刷新模式标志
    full_refresh: bool,
    /// TRIM模式标志
    trim_mode: bool,
    /// 数据时效
    min_days: u32,
    /// 是否跳过小文件
    skip_small: bool,
}

impl Dashboard {
    /// 创建新的仪表盘实例
    pub fn new() -> Self {
        let terminal = TerminalManager::get_instance();
        let now = SystemTime::now();
        
        Self {
            terminal,
            start_time: now,
            last_update: now,
            last_scanned: 0,
            working_directory: "".to_string(),
            full_refresh: false,
            trim_mode: false,
            min_days: 0,
            skip_small: false,
        }
    }
    
    /// 设置工作目录
    pub fn set_working_directory(&mut self, directory: &str) {
        self.working_directory = directory.to_string();
    }
    
    /// 设置全盘刷新模式
    pub fn set_full_refresh(&mut self, full_refresh: bool) {
        self.full_refresh = full_refresh;
    }
    
    /// 设置TRIM模式
    pub fn set_trim_mode(&mut self, trim_mode: bool) {
        self.trim_mode = trim_mode;
    }
    
    /// 设置数据时效
    pub fn set_min_days(&mut self, min_days: u32) {
        self.min_days = min_days;
    }
    
    /// 设置是否跳过小文件
    pub fn set_skip_small(&mut self, skip_small: bool) {
        self.skip_small = skip_small;
    }
    
    /// 更新显示
    pub fn update_display(&mut self, stats: &OperationStats, phase: &str) {
        // 控制更新频率
        let now = SystemTime::now();
        let elapsed = now.duration_since(self.last_update).unwrap_or(std::time::Duration::from_secs(0)).as_secs_f32();
        if elapsed < 0.2 { // 每0.2秒更新一次
            return;
        }
        
        // 清除屏幕
        self.terminal.clear();
        
        // 渲染标题
        self.render_header();
        
        // 渲染统计信息
        self.render_stats(stats, phase);
        
        // 刷新输出
        std::io::stdout().flush().unwrap();
        
        // 更新上次更新时间
        self.last_update = now;
    }
    
    /// 渲染标题
    fn render_header(&self) {
        let border = if self.terminal.safe_mode() {
            "="
        } else {
            "═"
        };
        let h_line = border.repeat(70);
        
        let header = self.terminal.colored_text(
            " SSD掉速激活-冷数据维护系统 v4.7.0 作者:support@e2bank.cn By Rust", 
            37, 44
        );
        
        // 打印主标题
        println!("\n{}", h_line);
        println!("{:^70}", header);
        println!("{}", h_line);
    }
    
    /// 渲染统计信息
    fn render_stats(&mut self, stats: &OperationStats, phase: &str) {
        let border = if self.terminal.safe_mode() {
            "|"
        } else {
            "│"
        };
        let fill = if self.terminal.safe_mode() {
            "#"
        } else {
            "▓"
        };
        let empty = if self.terminal.safe_mode() {
            "-"
        } else {
            "░"
        };
        
        // 计算扫描速度
        let scan_speed = if stats.scanned > self.last_scanned {
            let elapsed = SystemTime::now().duration_since(self.last_update).unwrap_or(std::time::Duration::from_secs(0)).as_secs_f64();
            if elapsed > 0.0 {
                (stats.scanned - self.last_scanned) as f64 / elapsed
            } else {
                0.0
            }
        } else {
            0.0
        };
        self.last_scanned = stats.scanned;
        
        // 构建双重进度信息
        let scan_info = if phase == "扫描中" {
            format!("发现文件: {}", stats.scanned)
        } else {
            "".to_string()
        };
        let process_bar = format!("[{}{}]", fill.repeat((50.0 * stats.progress) as usize), empty.repeat(50 - (50.0 * stats.progress) as usize));
        
        // 获取操作模式显示文本
        let mode_text = if self.full_refresh {
            "全盘刷新模式"
        } else if self.trim_mode {
            "TRIM模式"
        } else {
            "常规模式"
        };
        
        let info_lines = vec![
            "智能检测固态硬盘的冷数据并解决冷数据掉速问题。",
            "GitHub:https://github.com/aspnmy/ColDataRefresh.git",
            &format!("工作路径: {}  ", self.working_directory),
            &format!("操作模式: {}  ", self.terminal.colored_text(mode_text, 32, 44)),
            &format!("数据时效: {} 天, 跳过小文件: {}  ", self.min_days, if self.skip_small { "是" } else { "否" }),
            &format!("当前时间: {}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()),
            &format!("运行阶段: {} 耗时: {:.1}s", phase, SystemTime::now().duration_since(self.start_time).unwrap_or_default().as_secs_f64()),
            &format!("处理进度: {} {:.1}%", process_bar, stats.progress * 100.0),
            &scan_info,
            &format!("扫描速度: {:.1} MB/s, 处理速度: {:.1} MB/s ", scan_speed, stats.speed),
            &format!("文件分类: 大（>100MB）({}) 中（10-100MB）({}) 小（<10MB）({})", stats.large, stats.medium, stats.small),
            &format!("损坏的文件: {}", self.terminal.colored_text(&stats.corrupted.to_string(), 31, 44)),
            "按Ctrl+C退出程序",
        ];
        
        // 清理空行并渲染
        for line in info_lines {
            if !line.is_empty() {
                println!("{} {}", border, line);
            }
        }
        
        // 打印底部边框
        let border = if self.terminal.safe_mode() {
            "="
        } else {
            "═"
        };
        println!("{}{}", border, border.repeat(69));
    }
}
