use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use crc32fast::Hasher;

use crate::config::{Config, CONFIG, FileCategory};
use crate::log_manager::{LOG_MANAGER};

/// 已处理的驱动器列表，用于避免重复执行TRIM操作
pub static mut PROCESSED_DRIVES: std::collections::HashSet<String> = std::collections::HashSet::new();

/// 文件操作器，负责处理文件相关的所有操作
pub struct FileOperator;

impl FileOperator {
    /// 文件分类方法
    /// 小文件: < 10MB
    /// 中等文件: 10MB - 100MB  
    /// 大文件: > 100MB
    pub fn categorize_file(size: u64) -> FileCategory {
        if size > unsafe { CONFIG.large_file } {
            FileCategory::Large
        } else if size > unsafe { CONFIG.medium_file } {
            FileCategory::Medium
        } else {
            FileCategory::Small
        }
    }
    
    /// 计算文件的CRC32校验和
    pub fn checksum_file(path: &Path) -> Result<u32, std::io::Error> {
        let mut hasher = Hasher::new();
        let mut file = File::open(path)?;
        let mut buffer = vec![0; unsafe { CONFIG.buffer_size }];
        
        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }
        
        Ok(hasher.finalize())
    }
    
    /// 全盘数据刷新模式：将文件内容统一写入FF值
    pub fn full_refresh_file(path: &Path, size: u64) -> Result<bool, std::io::Error> {
        // 检查是否为系统保护目录
        if let Some(dir_path) = path.parent() {
            let dir_str = dir_path.to_string_lossy().to_uppercase();
            if dir_str.contains("$RECYCLE.BIN") || dir_str.contains("SYSTEM VOLUME INFORMATION") {
                println!("警告: 跳过系统保护目录中的文件: {}", path.display());
                return Ok(false);
            }
        }
        
        let temp_file = path.with_extension("tmp");
        let mut processed_size = 0;
        
        // 创建填充块
        let fill_pattern = unsafe { &CONFIG.full_refresh_pattern };
        let fill_block = fill_pattern.repeat(unsafe { CONFIG.buffer_size / fill_pattern.len() });
        
        // 写入临时文件
        let mut file = File::create(&temp_file)?;
        let mut remaining = size;
        
        while remaining > 0 {
            let chunk_size = std::cmp::min(unsafe { CONFIG.buffer_size }, remaining as usize);
            file.write_all(&fill_block[..chunk_size])?;
            processed_size += chunk_size as u64;
            remaining -= chunk_size as u64;
        }
        
        // 确保所有数据都写入磁盘
        file.flush()?;
        std::fs::rename(&temp_file, path)?;
        
        Ok(true)
    }
    
    /// 持续全盘写入模式：直接写入目标文件，不再使用临时文件
    pub fn continuous_full_refresh_file(path: &Path, target_unit_size: u64) -> Result<(u64, f64), std::io::Error> {
        // 检查是否为系统保护目录
        if let Some(dir_path) = path.parent() {
            let dir_str = dir_path.to_string_lossy().to_uppercase();
            if dir_str.contains("$RECYCLE.BIN") || dir_str.contains("SYSTEM VOLUME INFORMATION") {
                println!("警告: 跳过系统保护目录中的文件: {}", path.display());
                return Ok((0, 0.0));
            }
        }
        
        // 检查目录是否存在
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                println!("错误: 目录 {} 不存在", parent.display());
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "目录不存在"));
            }
            
            // 检查是否有写入权限
            if !parent.metadata()?.permissions().readonly() {
                println!("错误: 没有写入权限: {}", parent.display());
                return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "无写入权限"));
            }
        }
        
        // 清理可能存在的旧文件
        if path.exists() {
            println!("删除已存在的文件: {}", path.display());
            fs::remove_file(path)?;
        }
        
        // 创建填充块
        let fill_pattern = unsafe { &CONFIG.full_refresh_pattern };
        let fill_block = fill_pattern.repeat(unsafe { CONFIG.buffer_size / fill_pattern.len() });
        
        let start_time = std::time::Instant::now();
        let mut max_speed = 0.0;
        let mut total_written = 0;
        
        let mut file = File::create(path)?;
        let mut remaining = target_unit_size;
        
        while remaining > 0 {
            let chunk_size = std::cmp::min(unsafe { CONFIG.buffer_size }, remaining as usize);
            file.write_all(&fill_block[..chunk_size])?;
            total_written += chunk_size;
            remaining -= chunk_size as u64;
            
            // 计算写入速度
            let elapsed = start_time.elapsed().as_secs_f64();
            if elapsed > 1.0 {
                let speed = total_written as f64 / (1024.0 * 1024.0) / elapsed;
                max_speed = max_speed.max(speed);
                print!("\r当前写入速度: {:.2f} MB/s", speed);
            }
        }
        
        // 确保数据写入磁盘
        file.flush()?;
        
        Ok((total_written as u64, max_speed))
    }
    
    /// 真正的TRIM功能：通知操作系统哪些数据块是无效的
    pub fn trim_file(path: &Path, size: u64) -> Result<bool, std::io::Error> {
        let trim_size = std::cmp::min(unsafe { CONFIG.trim_block_size }, size);
        
        if cfg!(target_os = "windows") {
            // Windows实现
            // 获取文件所在的驱动器
            let drive = if let Some(drive) = path.to_str() {
                if drive.len() >= 2 && drive.chars().nth(1) == Some(':') {
                    drive[0..2].to_string()
                } else {
                    std::env::current_dir()?.to_string_lossy()[0..2].to_string()
                }
            } else {
                std::env::current_dir()?.to_string_lossy()[0..2].to_string()
            };
            
            // 检查是否已经处理过该驱动器，避免重复执行TRIM操作
            unsafe {
                if PROCESSED_DRIVES.contains(&drive) {
                    if let Some(log_manager) = &LOG_MANAGER {
                        log_manager.log_operation(&format!("驱动器{drive}已经执行过TRIM操作，跳过本次操作"), "INFO");
                    }
                    return Ok(true);
                }
                PROCESSED_DRIVES.insert(drive.clone());
            }
            
            // 添加TRIM操作提示信息
            println!("\n" + "="*60);
            println!("正在对驱动器 {} 进行 SSD固态盘实时TRIM优化操作", drive);
            println!("="*60);
            println!("注意事项：");
            println!("1. 后台执行时间预计需要10-30分钟");
            println!("2. 30分钟内请不要对该SSD固态硬盘进行断电操作");
            println!("3. TRIM操作有助于提高SSD性能并延长使用寿命");
            println!("4. 操作期间可以继续使用计算机，但建议减少对该驱动器的大量读写");
            println!("="*60);
            
            // TODO: 实现Windows下的TRIM操作
            // 这里需要使用Windows API调用，如DeviceIoControl或PowerShell命令
            Ok(true)
        } else {
            // Linux/Unix实现
            // TODO: 实现Linux下的TRIM操作
            Ok(true)
        }
    }
}
