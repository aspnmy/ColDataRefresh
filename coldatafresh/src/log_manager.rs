use std::fs::{self, OpenOptions};
use std::io::{Write, BufWriter};
use std::path::Path;
use std::sync::Mutex;
use chrono::Local;
use serde::{Deserialize, Serialize};

use crate::config::{Config, CONFIG};

/// 日志数据结构
#[derive(Debug, Deserialize, Serialize)]
pub struct LogData {
    pub pending: Vec<String>,
    pub completed: Vec<String>,
    pub corrupted: Vec<String>,
}

/// 日志管理器，负责确保日志目录存在并提供统一的日志记录功能
pub struct LogManager {
    /// 日志文件的互斥锁，确保线程安全
    mutex: Mutex<()>,
}

impl LogManager {
    /// 创建新的日志管理器实例
    pub fn new() -> Self {
        Self {
            mutex: Mutex::new(()),
        }
    }
    
    /// 确保日志目录存在，如果不存在则创建
    pub fn ensure_log_directory(&self) {
        let _guard = self.mutex.lock().expect("无法获取日志管理器锁");
        
        // 确保日志目录存在
        let log_dir = unsafe { CONFIG.log_file.parent() };
        if let Some(dir) = log_dir {
            if !dir.exists() {
                match fs::create_dir_all(dir) {
                    Ok(_) => println!("日志目录已创建: {}", dir.display()),
                    Err(e) => println!("警告: 无法创建日志目录: {}", e),
                }
            }
        }
    }
    
    /// 记录操作日志到统一的错误日志文件
    pub fn log_operation(&self, message: &str, level: &str) {
        let _guard = self.mutex.lock().expect("无法获取日志管理器锁");
        
        self.ensure_log_directory();
        
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S.%3f");
        let log_entry = format!("[{timestamp}] [{level}] {message}\n");
        
        // 写入错误日志文件
        if let Err(e) = self.write_to_file(unsafe { &CONFIG.error_log }, &log_entry) {
            println!("日志记录失败: {}", e);
        }
    }
    
    /// 记录损坏文件信息
    pub fn log_corrupted_file(&self, path: &str, error_type: &str, error_message: &str) {
        let _guard = self.mutex.lock().expect("无法获取日志管理器锁");
        
        self.ensure_log_directory();
        
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        let log_entry = format!("{timestamp}|{path}|{error_type}|{error_message}\n");
        
        // 写入损坏文件日志
        if let Err(e) = self.write_to_file(unsafe { &CONFIG.corrupted_log }, &log_entry) {
            println!("损坏文件日志记录失败: {}", e);
            self.log_operation(&format!("无法记录损坏文件: {}, 错误: {}", path, e), "ERROR");
        }
    }
    
    /// 将日志条目写入文件
    fn write_to_file(&self, path: &Path, content: &str) -> std::io::Result<()> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        
        let mut writer = BufWriter::new(file);
        writer.write_all(content.as_bytes())?;
        writer.flush()?;
        Ok(())
    }
}

/// 全局日志管理器实例
pub static mut LOG_MANAGER: Option<LogManager> = None;

/// 初始化全局日志管理器
pub fn init_log_manager() {
    unsafe {
        LOG_MANAGER = Some(LogManager::new());
        LOG_MANAGER.as_ref().unwrap().ensure_log_directory();
    }
}
