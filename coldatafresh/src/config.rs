use std::path::PathBuf;

/// 配置结构体，包含应用程序的所有配置项
#[derive(Debug, Clone)]
pub struct Config {
    /// 程序运行目录
    pub script_dir: PathBuf,
    /// 日志文件路径
    pub log_file: PathBuf,
    /// 损坏文件日志路径
    pub corrupted_log: PathBuf,
    /// 错误日志路径
    pub error_log: PathBuf,
    /// 缓冲区大小
    pub buffer_size: usize,
    /// 最大重试次数
    pub max_retries: u32,
    /// 大文件阈值
    pub large_file: u64,
    /// 中等文件阈值
    pub medium_file: u64,
    /// 报告间隔
    pub report_interval: f32,
    /// 跳过小文件阈值
    pub skip_small: u64,
    /// 最大工作线程数
    pub max_workers: usize,
    /// 内存限制(MB)
    pub memory_limit_mb: u32,
    /// 全盘刷新模式标志
    pub full_refresh_mode: bool,
    /// 全盘刷新时写入的填充值
    pub full_refresh_pattern: Vec<u8>,
    /// TRIM模式标志
    pub trim_mode: bool,
    /// TRIM操作的块大小
    pub trim_block_size: u64,
}

impl Default for Config {
    fn default() -> Self {
        // 获取程序运行目录
        let script_dir = std::env::current_dir().expect("无法获取当前目录");
        
        Self {
            script_dir: script_dir.clone(),
            log_file: script_dir.join("refresh_log.json"),
            corrupted_log: script_dir.join("corrupted_files.log"),
            error_log: script_dir.join("error.log"),
            buffer_size: 4 * 1024, // 4KB
            max_retries: 3,
            large_file: 100 * 1024 * 1024, // 100MB
            medium_file: 10 * 1024 * 1024, // 10MB
            report_interval: 0.2,
            skip_small: 1 * 1024 * 1024, // 1MB
            max_workers: 4,
            memory_limit_mb: 512,
            full_refresh_mode: false,
            full_refresh_pattern: vec![0xFF], // FF值
            trim_mode: false,
            trim_block_size: 1 * 1024 * 1024, // 1MB
        }
    }
}

/// 全局配置实例
pub static mut CONFIG: Config = Config::default();

/// 文件分类枚举
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FileCategory {
    Small,
    Medium,
    Large,
}
