use std::path::PathBuf;
use std::sync::OnceLock;

/// 文件分类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileCategory {
    Small,
    Medium,
    Large,
}

/// 全局线程安全配置
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Config {
    pub script_dir: PathBuf,
    pub log_file: PathBuf,
    pub corrupted_log: PathBuf,
    pub error_log: PathBuf,
    pub buffer_size: usize,
    pub max_retries: u32,
    pub large_file_threshold: u64,
    pub medium_file_threshold: u64,
    pub report_interval_secs: f32,
    pub skip_small_threshold: u64,
    pub max_workers: usize,
    pub memory_limit_mb: u32,
    pub full_refresh_pattern: Vec<u8>,
    pub trim_block_size: u64,
    pub version: &'static str,
}

impl Default for Config {
    fn default() -> Self {
        let script_dir = std::env::current_dir().expect("无法获取当前目录");

        Self {
            script_dir: script_dir.clone(),
            log_file: script_dir.join("refresh_log.json"),
            corrupted_log: script_dir.join("corrupted_files.log"),
            error_log: script_dir.join("error.log"),
            buffer_size: 256 * 1024,
            max_retries: 3,
            large_file_threshold: 100 * 1024 * 1024,
            medium_file_threshold: 10 * 1024 * 1024,
            report_interval_secs: 0.2,
            skip_small_threshold: 1024 * 1024,
            max_workers: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4),
            memory_limit_mb: 512,
            full_refresh_pattern: vec![0xFF],
            trim_block_size: 1024 * 1024,
            version: "5.0.0",
        }
    }
}

static CONFIG: OnceLock<Config> = OnceLock::new();

/// 获取全局配置引用（线程安全）
pub fn config() -> &'static Config {
    CONFIG.get_or_init(Config::default)
}

/// 文件分类函数
pub fn categorize_file(size: u64) -> FileCategory {
    let cfg = config();
    if size > cfg.large_file_threshold {
        FileCategory::Large
    } else if size > cfg.medium_file_threshold {
        FileCategory::Medium
    } else {
        FileCategory::Small
    }
}

pub fn format_size(size: u64) -> String {
    if size < 1024 {
        format!("{} B", size)
    } else if size < 1024u64.pow(2) {
        format!("{:.2} KB", size as f64 / 1024.0)
    } else if size < 1024u64.pow(3) {
        format!("{:.2} MB", size as f64 / (1024u64.pow(2) as f64))
    } else if size < 1024u64.pow(4) {
        format!("{:.2} GB", size as f64 / (1024u64.pow(3) as f64))
    } else {
        format!("{:.2} TB", size as f64 / (1024u64.pow(4) as f64))
    }
}
