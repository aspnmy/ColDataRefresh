use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::{CONFIG};
use crate::file_operator::FileOperator;

/// 全盘刷新功能管理器，负责实现完整的全盘刷新业务流程
pub struct FullRefreshManager;

impl FullRefreshManager {
    /// 判断指定路径是否为整个盘符
    pub fn is_drive(directory: &Path) -> bool {
        if cfg!(target_os = "windows") {
            // Windows系统：判断是否为盘符格式（如 C: 或 C:\）
            if let Some(dir_str) = directory.to_str() {
                return dir_str.matches(':').count() == 1 && (dir_str.ends_with(':') || dir_str.ends_with(":\\"));
            }
        } else {
            // Linux系统：判断是否为根目录或挂载点
            return directory == Path::new("/") || std::fs::metadata(directory).map(|md| md.is_dir()).unwrap_or(false);
        }
        false
    }
    
    /// 获取指定目录的总容量和已使用容量
    pub fn get_directory_stats(directory: &Path) -> (u64, u64, u64) {
        if cfg!(target_os = "windows") {
            // Windows系统：使用Windows API获取磁盘空间
            // TODO: 实现Windows下的磁盘空间获取
            (0, 0, 0)
        } else {
            // Linux/Mac OS 使用statvfs
            // TODO: 实现Linux下的磁盘空间获取
            (0, 0, 0)
        }
    }
    
    /// 格式化指定盘符
    pub fn format_drive(drive: &str) -> bool {
        if cfg!(target_os = "windows") {
            // Windows系统：使用diskpart命令格式化盘符
            // TODO: 实现Windows下的盘符格式化
            false
        } else {
            // Linux系统：使用mkfs命令格式化
            // TODO: 实现Linux下的盘符格式化
            false
        }
    }
    
    /// 备份文件到临时目录
    pub fn backup_files(source_dir: &Path, backup_dir: &Path) -> bool {
        // 创建备份目录
        if let Err(e) = fs::create_dir_all(backup_dir) {
            println!("创建备份目录失败: {}", e);
            return false;
        }
        
        // 遍历源目录，备份所有文件
        if let Err(e) = Self::copy_directory(source_dir, backup_dir) {
            println!("备份文件失败: {}", e);
            return false;
        }
        
        true
    }
    
    /// 从临时目录恢复文件
    pub fn restore_files(backup_dir: &Path, target_dir: &Path) -> bool {
        // 遍历备份目录，恢复所有文件
        if let Err(e) = Self::copy_directory(backup_dir, target_dir) {
            println!("恢复文件失败: {}", e);
            return false;
        }
        
        true
    }
    
    /// 复制目录内容
    fn copy_directory(source: &Path, destination: &Path) -> std::io::Result<()> {
        // 确保目标目录存在
        fs::create_dir_all(destination)?;
        
        // 遍历源目录
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let source_path = entry.path();
            let dest_path = destination.join(entry.file_name());
            
            if source_path.is_dir() {
                // 跳过系统保护目录
                let dir_str = source_path.to_string_lossy().to_uppercase();
                if dir_str.contains("$RECYCLE.BIN") || dir_str.contains("SYSTEM VOLUME INFORMATION") {
                    continue;
                }
                
                // 递归复制子目录
                Self::copy_directory(&source_path, &dest_path)?;
            } else {
                // 复制文件
                fs::copy(&source_path, &dest_path)?;
            }
        }
        
        Ok(())
    }
    
    /// 计算需要创建的文件数量
    pub fn calculate_max_files(total_capacity: u64, unit_size: u64) -> u64 {
        if unit_size <= 0 {
            0
        } else {
            total_capacity / unit_size
        }
    }
    
    /// 填满可用空间
    pub fn fill_available_space(directory: &Path, unit_size: u64) -> (u64, f64) {
        // 创建工作目录
        let work_dir = directory.join("$aspnmytools");
        if let Err(e) = fs::create_dir_all(&work_dir) {
            println!("创建工作目录失败: {}", e);
            return (0, 0.0);
        }
        
        let mut cumulative_capacity = 0;
        let mut max_write_speed = 0.0;
        let mut file_count = 0;
        
        // 计算需要创建的文件数量
        let (total_bytes, _, _) = Self::get_directory_stats(directory);
        let max_files = Self::calculate_max_files(total_bytes, unit_size);
        println!("预计需要创建 {} 个文件", max_files);
        
        // 持续写入文件，直到填满可用空间
        loop {
            // 检查可用空间
            let (_, _, free_bytes) = Self::get_directory_stats(directory);
            if free_bytes < unit_size {
                break;
            }
            
            // 创建文件路径
            let file_path = work_dir.join(format!("refresh_{}.dat", file_count));
            
            // 写入文件
            match FileOperator::continuous_full_refresh_file(&file_path, unit_size) {
                Ok((written, speed)) => {
                    cumulative_capacity += written;
                    if speed > max_write_speed {
                        max_write_speed = speed;
                    }
                    
                    file_count += 1;
                    
                    // 显示进度
                    let progress = if max_files > 0 {
                        file_count as f64 / max_files as f64
                    } else {
                        0.0
                    };
                    println!("已创建 {} 个文件，进度: {:.1}%", file_count, progress * 100.0);
                }
                Err(e) => {
                    println!("写入文件失败: {}", e);
                    break;
                }
            }
        }
        
        (cumulative_capacity, max_write_speed)
    }
    
    /// 清理临时文件和目录
    pub fn cleanup(work_dir: &Path, keep_backup: bool) -> bool {
        // 清理工作目录
        if let Err(e) = fs::remove_dir_all(work_dir) {
            println!("清理工作目录失败: {}", e);
            return false;
        }
        
        true
    }
}
