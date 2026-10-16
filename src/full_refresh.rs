//! 全盘刷新业务流程：备份 → 删除 → 填充 → 恢复 → TRIM。

use std::path::Path;

use crate::config::format_size;
use crate::file_op;
use crate::log::logger;
use crate::platform;

/// 全盘刷新参数（交互与 CLI 共用的输入结构）
#[derive(Debug, Clone)]
pub struct FullRefreshParams {
    /// 工作目录（**必填**，issue #14）——备份落点与填充落点两者共用；
    /// ⛔ 不提供兜底默认值：各系统临时目录不同，内置默认值会写错位置
    pub work_dir: String,
    pub keep_files: bool,
    pub fill_free: bool,
    pub unit_gb: u64,
    pub write_buf_kb: u64,
}

/// 全盘刷新管理器 — 实现完整的全盘刷新业务流程
pub struct FullRefresh;

impl FullRefresh {
    /// 执行全盘刷新业务流程
    /// dir_size: 目录本身占用的空间（始终填充这部分）
    /// fill_free: 是否额外填充空闲空间
    /// 返回 true = 全流程成功；false = 中途失败（备份/删除/备份盘缺失），调用方据此定退出码。
    /// `work_dir`：**必填**工作目录（issue #14）——备份落点与填充文件临时落点**两者共用**。
    /// ⛔ 不再内置任何兜底路径（原 `$aspnmytools` 相对目录 + Windows 盘符探测
    /// `find_backup_drive` 在 Linux 上恒返 None）⇒ 缺该参数由调用方直接报错，不进入本函数。
    pub fn execute(directory: &str, work_dir: &str, keep_files: bool, fill_free: bool,
                   dir_size: u64, unit_size_gb: u64, write_buf_kb: u64) -> bool {
        logger().log("开始全盘刷新业务流程", "INFO");

        let unit_size = unit_size_gb * 1024u64.pow(3);
        let dir_path = Path::new(directory);
        // 统一入口（issue #14）：工作目录的解析/准备走 platform，⛔ 各模式不再各写一遍
        let work_path = match platform::resolve_work_dir(Some(work_dir)) {
            Ok(p) => p,
            Err(e) => {
                println!("❌ {}", e);
                return false;
            }
        };

        // 1. 获取磁盘统计信息
        let (_total, used, free) = platform::get_disk_space(dir_path);
        println!("磁盘信息 (Disk info):");
        println!("  已使用 (used): {}", format_size(used));
        println!("  可用 (free): {}", format_size(free));

        // 2. 备份文件（如果需要）
        let backup_dir = if keep_files {
            // issue #14：备份落点统一交 platform 准备（⛔ 不再自动探测盘符 ——
            // 原 find_backup_drive 是 Windows 专有逻辑，Linux 上恒返 None ⇒ 必挂）
            let backup = match platform::prepare_work_dir(&work_path, Some(platform::WorkSubDir::Backup)) {
                Ok(p) => p,
                Err(e) => {
                    println!("❌ {}，终止全盘刷新 (aborting to protect data)", e);
                    return false;
                }
            };
            println!("\n正在备份文件到 (Backing up to) {}...", backup.display());

            if backup_files(directory, &backup) {
                println!("文件备份成功 (Backup succeeded)");
                Some(backup)
            } else {
                println!("❌ 文件备份失败 (backup failed)，终止全盘刷新 (aborting to protect data)");
                return false;
            }
        } else {
            println!("\n用户选择不保留文件，数据将无法恢复 (not keeping files, data unrecoverable)");
            None
        };

        // 3. 已备份，删除目标目录中的原始文件
        println!("\n正在删除目标目录中的原始文件... (Deleting original files...)");
        match delete_directory_contents(dir_path) {
            Err(e) => {
                println!("❌ 删除失败 (delete failed): {}", e);
                println!("   请手动删除目标目录中的文件后再执行全盘刷新 (delete manually, then retry)");
                return false;
            }
            Ok((success, failure)) => {
                if success > 0 || failure > 0 {
                    println!("删除结果 (Delete result): 成功/ok {} 个, 失败/failed {} 个", success, failure);
                    if failure > 0 {
                        println!("⚠️  部分文件删除失败 (partial delete failure)，可能影响填充效果");
                    }
                } else {
                    println!("目录为空，无需删除 (directory empty, nothing to delete)");
                }
                println!("原始文件已删除，空间已释放 (originals deleted, space freed)");
            }
        }

        // 4. 填充空间（覆写原文件释放的空间 + 可选填充空闲空间）
        println!("\n开始填充空间 (Filling)，目录大小 (dir size): {}，空闲空间填充 (fill free): {}",
                 format_size(dir_size), if fill_free { "是" } else { "否" });
        // 填充落点统一交 platform 准备（issue #14：⛔ 不再写死 $aspnmytools）
        let fill_dir = match platform::prepare_work_dir(&work_path, Some(platform::WorkSubDir::Fill)) {
            Ok(p) => p,
            Err(e) => {
                println!("❌ {}", e);
                return false;
            }
        };
        let fill_dir_str = fill_dir.to_string_lossy().to_string();
        let result = fill_available_space(directory, &fill_dir_str, unit_size, dir_size, fill_free, write_buf_kb);
        let (cumulative, max_speed) = result;

        // 4.5 删除填充文件，释放空间用于恢复
        let work_dir = fill_dir;
        if work_dir.exists() {
            println!("\n正在删除填充文件... (Deleting fill files...)");
            match std::fs::remove_dir_all(&work_dir) {
                Ok(()) => println!("填充文件已删除 (fill files deleted)"),
                Err(e) => println!("⚠️  删除填充文件失败 (delete fill files failed): {}", e),
            }
        }

        // 5. 恢复文件（如果需要）
        if let Some(ref backup) = backup_dir {
            if backup.exists() {
                println!("\n正在恢复文件... (Restoring files...)");
                if restore_files(backup, directory) {
                    println!("文件恢复成功 (Restore succeeded)");
                    let _ = std::fs::remove_dir_all(backup);
                } else {
                    println!("文件恢复失败 (restore failed)，请手动从 (manual restore from) {} 恢复", backup.display());
                }
            }
        } else if keep_files {
            println!("\n⚠️  未备份文件，无法恢复 (no backup, cannot restore)");
        }

        // 6. 执行最终 TRIM（底层直下，issue #5）
        println!("\n开始执行最终 TRIM 优化... (Running final TRIM...)");
        if let Some(device) = platform::resolve_device_name(directory) {
            platform::trim_volume_direct(&device);
        }

        println!("\n全盘刷新完成! (Full refresh completed)");
        println!("累积写入容量 (Total written): {}", format_size(cumulative));
        println!("最高写入速度 (Max speed): {:.2} MB/s", max_speed);

        true
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
                // 平台专有的创建时间保留（Windows 用 Win32 SetFileTime，其他平台空操作）
                platform::preserve_creation_time(&dest);
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
            // 平台专有的只读属性清除（Windows 用 attrib -R，其他平台空操作）
            platform::clear_readonly(&path);

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

/// 填充空间
/// dir_size: 目录本身的已用空间（始终填充）
/// fill_free: 填充完目录空间后是否继续填充空闲空间
/// unit_size: 填充空闲空间时的每文件写入单位
///
/// 优先走**块设备直写**（issues #3/#4：绕过文件系统，避免元数据 / 页缓存拖垮导致卡死）；
/// 无法解析块设备或安全闸门拒绝时，回退原有文件系统写入路径。
fn fill_available_space(directory: &str, work_dir: &str, unit_size: u64, dir_size: u64,
                        fill_free: bool, buf_kb: u64) -> (u64, f64) {
    // ── 块设备直写后端（优先） ──
    if let Some(dev) = platform::resolve_block_device(directory) {
        match crate::block_dev::BlockDevice::open(&dev) {
            Ok(bd) => {
                println!(
                    "直写后端 (direct-write backend): {} (扇区 {}B, 容量 {})",
                    dev,
                    bd.alignment(),
                    format_size(bd.info().capacity)
                );
                // 目标范围：整个设备（fill_free）或按 dir_size 限定
                let total = if fill_free { bd.info().capacity } else { dir_size };
                let start = std::time::Instant::now();
                match crate::block_dev::direct_fill(&bd, 0, total, unit_size, buf_kb) {
                    Ok((units, written)) => {
                        let secs = start.elapsed().as_secs_f64().max(0.001);
                        let speed = written as f64 / secs / 1_048_576.0;
                        println!(
                            "\n直写完成 (direct fill done): {} 个单元, 累积写入 {}",
                            units,
                            format_size(written)
                        );
                        return (written, speed);
                    }
                    Err(e) => {
                        println!("⚠️  直写失败 (direct fill failed): {}，回退文件系统写入 (fallback)", e);
                        logger().log(&format!("块设备直写失败，回退: {}", e), "WARNING");
                    }
                }
            }
            Err(e) => {
                println!("直写不可用 (direct-write unavailable): {}，回退文件系统写入 (fallback)", e);
            }
        }
    }

    // ── 文件系统写入路径（回退，保持原行为） ──
    println!("使用文件系统写入路径 (filesystem write path)");
    // issue #14：填充落点统一交 platform 准备（⛔ 各模式不再各写一遍拼路径）
    let work_dir = match platform::prepare_work_dir(Path::new(work_dir), None) {
        Ok(p) => p,
        Err(e) => {
            println!("❌ {}", e);
            return (0, 0.0);
        }
    };

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
                println!("\n写入失败 (write failed): {}", e);
                break;
            }
        }
    }

    println!();
    (cumulative, max_speed)
}
