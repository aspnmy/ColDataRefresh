use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

use chrono::Local;

use crate::config;

/// 线程安全的日志管理器
#[allow(dead_code)]
pub struct Logger {
    error_log: Mutex<()>,
    corrupted_log: Mutex<()>,
}

impl Logger {
    pub fn new() -> Self {
        Self {
            error_log: Mutex::new(()),
            corrupted_log: Mutex::new(()),
        }
    }

    /// 确保日志目录存在
    pub fn ensure_dirs(&self) {
        let cfg = config::config();
        if let Some(parent) = cfg.error_log.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Some(parent) = cfg.corrupted_log.parent() {
            let _ = fs::create_dir_all(parent);
        }
    }

    /// 记录操作日志
    pub fn log(&self, message: &str, level: &str) {
        let _lock = self.error_log.lock().unwrap_or_else(|e| e.into_inner());
        self.ensure_dirs();

        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let entry = format!("[{}] [{}] {}\n", timestamp, level, message);

        if let Err(e) = append_file(&config::config().error_log, &entry) {
            eprintln!("日志写入失败: {}", e);
        }
    }

    /// 记录损坏文件（预留）
    #[allow(dead_code)]
    pub fn log_corrupted(&self, path: &str, error_type: &str, error_msg: &str) {
        let _lock = self.corrupted_log.lock().unwrap_or_else(|e| e.into_inner());
        self.ensure_dirs();

        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        let entry = format!("{}|{}|{}|{}\n", timestamp, path, error_type, error_msg);

        if let Err(e) = append_file(&config::config().corrupted_log, &entry) {
            eprintln!("损坏文件日志写入失败: {}", e);
            self.log(&format!("无法记录损坏文件: {}, 错误: {}", path, e), "ERROR");
        }
    }

    /// 保存操作摘要（JSON 格式）
    #[allow(clippy::too_many_arguments)]
    pub fn save_summary(
        &self,
        scanned: u64,
        processed: u64,
        corrupted: u64,
        large: u64,
        medium: u64,
        small: u64,
        speed: f64,
        duration_secs: f64,
    ) {
        let _lock = self.error_log.lock().unwrap_or_else(|e| e.into_inner());
        self.ensure_dirs();

        let timestamp = Local::now().format("%Y-%m-%dT%H:%M:%S");
        let summary = serde_json::json!({
            "timestamp": timestamp.to_string(),
            "duration_seconds": (duration_secs * 100.0).round() / 100.0,
            "stats": {
                "scanned": scanned,
                "processed": processed,
                "corrupted": corrupted,
                "large": large,
                "medium": medium,
                "small": small,
                "final_speed": (speed * 100.0).round() / 100.0,
            }
        });

        let entry = summary.to_string() + "\n";
        if let Err(e) = append_file(&config::config().log_file, &entry) {
            eprintln!("摘要保存失败: {}", e);
        }
    }
}

fn append_file(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(content.as_bytes())?;
    file.flush()?;
    Ok(())
}

use std::sync::OnceLock;
static LOGGER: OnceLock<Logger> = OnceLock::new();

pub fn logger() -> &'static Logger {
    LOGGER.get_or_init(|| {
        let log = Logger::new();
        log.ensure_dirs();
        log
    })
}
