use std::path::Path;

use crate::config::format_size;
use crate::file_op;
use crate::log::logger;
use crate::platform;

/// 全盘刷新管理器 — 实现完整的全盘刷新业务流程
pub struct FullRefresh;

impl FullRefresh {
    /// 执行全盘刷新业务流程
    /// dir_size: 目录本身占用的空间（始终填充这部分）
    /// fill_free: 是否额外填充空闲空间
    pub fn execute(directory: &str, keep_files: bool, fill_free: bool,
                   dir_size: u64, unit_size_gb: u64, write_buf_kb: u64) {
        logger().log("开始全盘刷新业务流程", "INFO");

        let unit_size = unit_size_gb * 1024u64.pow(3);
        let dir_path = Path::new(directory);

        // 1. 获取磁盘统计信息
        let (_total, used, free) = platform::get_disk_space(dir_path);
        println!("磁盘信息:");
        println!("  已使用: {}", format_size(used));
        println!("  可用: {}", format_size(free));

        // 2. 备份文件（如果需要）
        let backup_dir = if keep_files {
            // 备份必须存到与目标不同的盘，否则覆写时会破坏备份
            let backup = match find_backup_drive(directory) {
                Some(drive) => drive.join("$aspnmytools"),
                None => {
                    println!("❌ 未找到可用的备份盘（需要与目标不同的硬盘），终止全盘刷新");
                    return;
                }
            };

            if let Err(e) = std::fs::create_dir_all(&backup) {
                println!("❌ 创建备份目录失败: {}，终止全盘刷新以保护数据", e);
                return;
            }
            println!("\n正在备份文件到 {}...", backup.display());

            if backup_files(directory, &backup) {
                println!("文件备份成功");
                Some(backup)
            } else {
                println!("❌ 文件备份失败，终止全盘刷新以保护数据");
                return;
            }
        } else {
            println!("\n用户选择不保留文件，数据将无法恢复");
            None
        };

        // 3. 已备份，删除目标目录中的原始文件
        println!("\n正在删除目标目录中的原始文件...");
        match delete_directory_contents(dir_path) {
            Err(e) => {
                println!("❌ 删除失败: {}", e);
                println!("   请手动删除目标目录中的文件后再执行全盘刷新");
                return;
            }
            Ok((success, failure)) => {
                if success > 0 || failure > 0 {
                    println!("删除结果: 成功 {} 个, 失败 {} 个", success, failure);
                    if failure > 0 {
                        println!("⚠️  部分文件删除失败，可能影响填充效果");
                    }
                } else {
                    println!("目录为空，无需删除");
                }
                println!("原始文件已删除，空间已释放");
            }
        }

        // 4. 填充空间（覆写原文件释放的空间 + 可选填充空闲空间）
        println!("\n开始填充空间，目录大小: {}，空闲空间填充: {}",
                 format_size(dir_size), if fill_free { "是" } else { "否" });
        let result = fill_available_space(directory, unit_size, dir_size, fill_free, write_buf_kb);
        let (cumulative, max_speed) = result;

        // 4.5 删除填充文件，释放空间用于恢复
        let work_dir = dir_path.join("$aspnmytools");
        if work_dir.exists() {
            println!("\n正在删除填充文件...");
            match std::fs::remove_dir_all(&work_dir) {
                Ok(()) => println!("填充文件已删除"),
                Err(e) => println!("⚠️  删除填充文件失败: {}", e),
            }
        }

        // 5. 恢复文件（如果需要）
        if let Some(ref backup) = backup_dir {
            if backup.exists() {
                println!("\n正在恢复文件...");
                if restore_files(backup, directory) {
                    println!("文件恢复成功");
                    let _ = std::fs::remove_dir_all(backup);
                } else {
                    println!("文件恢复失败，请手动从 {} 恢复", backup.display());
                }
            }
        } else if keep_files {
            println!("\n⚠️  未备份文件，无法恢复");
        }

        // 6. 执行最终 TRIM
        println!("\n开始执行最终 TRIM 优化...");
        if let Some(device) = platform::resolve_device_name(directory) {
            platform::trim_volume(&device);
        }

        println!("\n全盘刷新完成!");
        println!("累积写入容量: {}", format_size(cumulative));
        println!("最高写入速度: {:.2} MB/s", max_speed);
    }
}

/// 备份目录中的所有文件到目标路径（保留原始时间戳）
fn backup_files(source: &str, destination: &Path) -> bool {
    use std::fs;
    use filetime::FileTime;
    let src = Path::new(source);

    for entry in walkdir::WalkDir::new(src)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path == src {
            continue;
        }

        // 跳过系统保护目录
        let p_str = path.to_string_lossy().to_uppercase();
        if p_str.contains("$RECYCLE.BIN") || p_str.contains("SYSTEM VOLUME INFORMATION") {
            continue;
        }

        let relative = match path.strip_prefix(src) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let dest = destination.join(relative);

        if entry.file_type().is_dir() {
            let _ = fs::create_dir_all(&dest);
        } else {
            if let Some(parent) = dest.parent() {
                let _ = fs::create_dir_all(parent);
            }

            // 先获取源文件时间戳
            let src_meta = fs::metadata(path).ok();

            if let Err(e) = fs::copy(path, &dest) {
                logger().log(
                    &format!("备份文件失败 {}: {}", path.display(), e),
                    "WARNING",
                );
                return false;
            }

            // 保留时间戳
            if let Some(meta) = src_meta {
                if let Ok(mtime) = meta.modified() {
                    let atime = meta.accessed().unwrap_or(mtime);
                    let _ = filetime::set_file_times(
                        &dest,
                        FileTime::from_system_time(atime),
                        FileTime::from_system_time(mtime),
                    );
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    let ctime_raw = meta.creation_time();
                    if ctime_raw > 0 {
                        let _ = set_file_creation_time_win(&dest, ctime_raw);
                    }
                }
            }
        }
    }
    true
}

/// 从备份目录恢复所有文件到目标路径（保留原始时间戳）
fn restore_files(source: &Path, target: &str) -> bool {
    use std::fs;
    use filetime::FileTime;
    let dst = Path::new(target);

    for entry in walkdir::WalkDir::new(source)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path == source {
            continue;
        }

        let relative = match path.strip_prefix(source) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let dest = dst.join(relative);

        if entry.file_type().is_dir() {
            let _ = fs::create_dir_all(&dest);
            // 目录时间戳设为当前最新时间
            let now = std::time::SystemTime::now();
            let _ = filetime::set_file_times(
                &dest,
                FileTime::from_system_time(now),
                FileTime::from_system_time(now),
            );
        } else {
            if let Some(parent) = dest.parent() {
                let _ = fs::create_dir_all(parent);
            }

            // 复制文件内容
            if let Err(e) = fs::copy(path, &dest) {
                logger().log(
                    &format!("恢复文件失败 {}: {}", path.display(), e),
                    "WARNING",
                );
                return false;
            }

            // 恢复时间戳：设为当前最新时间（冷数据刷新目的）
            let now = std::time::SystemTime::now();
            let _ = filetime::set_file_times(
                &dest,
                FileTime::from_system_time(now),
                FileTime::from_system_time(now),
            );
        }
    }
    true
}

/// 删除目录下所有内容（不删除根目录本身）
/// 返回 (成功数, 失败数)
fn delete_directory_contents(dir: &Path) -> Result<(u64, u64), String> {
    let mut success = 0u64;
    let mut failure = 0u64;

    // 验证目录存在
    if !dir.exists() {
        return Ok((0, 0));
    }

    // 使用 walkdir 递归遍历所有条目（从子到父顺序，确保先删文件再删目录）
    let entries: Vec<_> = walkdir::WalkDir::new(dir)
        .contents_first(true) // 先处理子条目，再处理目录自身
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path() != dir) // 跳过根目录本身
        .collect();

    for entry in entries {
        let path = entry.path().to_path_buf();

        // 跳过系统保护目录
        let p_str = path.to_string_lossy().to_uppercase();
        if p_str.contains("$RECYCLE.BIN") || p_str.contains("SYSTEM VOLUME INFORMATION") {
            continue;
        }

        // 最多重试 3 次
        let mut last_err = String::new();
        let mut deleted = false;
        for _attempt in 0..3 {
            // Windows: 先移除只读属性
            remove_readonly(&path);

            let result = if entry.file_type().is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };

            match result {
                Ok(()) => {
                    deleted = true;
                    break;
                }
                Err(e) => {
                    last_err = e.to_string();
                    // 短暂等待后重试（处理临时占用）
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }

        if deleted {
            success += 1;
        } else {
            failure += 1;
            logger().log(
                &format!("删除失败(重试3次后) {}: {}", path.display(), last_err),
                "WARNING",
            );
        }
    }

    if failure > 0 && success == 0 {
        Err(format!(
            "所有文件删除失败（共 {} 个条目），请检查权限或手动删除",
            failure
        ))
    } else if failure > 0 {
        // 部分成功，报告警告但不中断流程
        logger().log(
            &format!(
                "目录删除部分完成：成功 {} 个，失败 {} 个",
                success, failure
            ),
            "WARNING",
        );
        Ok((success, failure))
    } else {
        Ok((success, failure))
    }
}

/// 移除文件的只读属性（Windows 专用，其他平台无操作）
#[cfg(windows)]
fn remove_readonly(path: &Path) {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_READONLY: u32 = 0x0001;
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.file_attributes() & FILE_ATTRIBUTE_READONLY != 0 {
            // 使用 attrib 命令移除只读属性
            let _ = std::process::Command::new("attrib")
                .args(["-R", &path.to_string_lossy()])
                .output();
        }
    }
}

/// 非 Windows 平台无操作
#[cfg(not(windows))]
fn remove_readonly(_path: &Path) {}

/// 填充空间
/// dir_size: 目录本身的已用空间（始终填充）
/// fill_free: 填充完目录空间后是否继续填充空闲空间
/// unit_size: 填充空闲空间时的每文件写入单位
fn fill_available_space(directory: &str, unit_size: u64, dir_size: u64,
                        fill_free: bool, buf_kb: u64) -> (u64, f64) {
    let work_dir = Path::new(directory).join("$aspnmytools");
    let _ = std::fs::create_dir_all(&work_dir);

    let mut cumulative = 0u64;
    let mut max_speed = 0.0f64;
    let mut file_count = 0u64;

    // 每个填充文件的大小：填充空闲空间时用 unit_size，仅填充目录时用 dir_size 或 50GB
    let file_target = if fill_free {
        unit_size
    } else {
        (50u64 * 1024u64.pow(3)).min(dir_size.max(1))
    };

    loop {
        let (_total, _used, free) = platform::get_disk_space(Path::new(directory));

        // 判断是否停止填充
        if fill_free {
            // 填充空闲空间模式：直到磁盘剩余空间 < unit_size
            if free < unit_size {
                break;
            }
        } else {
            // 仅填充目录空间模式：填充够 dir_size 就停
            if cumulative >= dir_size {
                break;
            }
        }

        let file_path = work_dir.join(format!("refresh_{}.dat", file_count));

        match file_op::continuous_full_refresh(&file_path, file_target, buf_kb) {
            Ok((written, speed)) => {
                cumulative += written;
                if speed > max_speed {
                    max_speed = speed;
                }
                file_count += 1;
                print!(
                    "\r已创建 {} 个文件, 累积写入: {}",
                    file_count,
                    format_size(cumulative)
                );
            }
            Err(e) => {
                println!("\n写入失败: {}", e);
                break;
            }
        }
    }

    println!();
    (cumulative, max_speed)
}

/// 查找可用于备份的盘符（与目标盘不同，优先 D:）
fn find_backup_drive(target_path: &str) -> Option<std::path::PathBuf> {
    let target_drive = if target_path.len() >= 2 && target_path.as_bytes()[1] == b':' {
        target_path[..1].to_uppercase()
    } else {
        return None; // Linux 下暂不支持自动查找
    };

    let mut candidates: Vec<String> = Vec::new();

    // 扫描 A:-Z: 所有盘符
    for letter in 'A'..='Z' {
        let drive = letter.to_string();
        if drive == target_drive {
            continue; // 跳过目标盘
        }

        let path = format!("{}:\\", drive);
        if std::fs::metadata(&path).is_ok() {
            candidates.push(drive);
        }
    }

    // 优先选 D:，否则取第一个可用盘
    if candidates.contains(&"D".to_string()) {
        Some(std::path::PathBuf::from(r"D:\"))
    } else {
        candidates.first().map(|d| std::path::PathBuf::from(format!("{}:\\", d)))
    }
}

/// Windows 下使用 Win32 API 设置文件创建时间
#[cfg(windows)]
fn set_file_creation_time_win(path: &std::path::Path, creation_time_raw: u64) -> Result<(), String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::SetFileTime;

    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|e| format!("无法打开文件 {}: {}", path.display(), e))?;

    let handle = file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
    let ft: *const windows_sys::Win32::Foundation::FILETIME =
        &creation_time_raw as *const u64 as *const windows_sys::Win32::Foundation::FILETIME;

    let result = unsafe { SetFileTime(handle, ft, std::ptr::null(), std::ptr::null()) };
    if result == 0 {
        Err(format!("设置创建时间失败: {}", path.display()))
    } else {
        Ok(())
    }
}
