use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crc32fast::Hasher;
use walkdir::WalkDir;

use crate::config::config as get_config;

/// 全局中断标志
pub static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// 获取或创建线程本地缓冲区
#[allow(clippy::missing_const_for_thread_local)]
fn with_buffer<F, R>(f: F) -> R
where
    F: FnOnce(&mut Vec<u8>) -> R,
{
    std::thread_local! {
        static BUF: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    BUF.with(|cell| {
        let mut buf = cell.borrow_mut();
        buf.resize(get_config().buffer_size, 0);
        f(&mut buf)
    })
}

/// 扫描目录，收集符合条件的冷数据文件
/// skip_small: true 时跳过 < 1MB 的文件
/// progress: 可选进度回调，每处理约 100 个文件调用一次
pub fn collect_files(
    directory: &str,
    min_days: u32,
    skip_small: bool,
    mut progress: impl FnMut(u64),
) -> Vec<String> {
    let cutoff = if min_days > 0 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        (now as i64) - (min_days as i64 * 86400)
    } else {
        0 // 0 表示不过滤
    };

    let mut files = Vec::new();
    let cfg = get_config();

    let mut scan_count = 0u64;
    for entry in WalkDir::new(directory)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if INTERRUPTED.load(Ordering::Relaxed) {
            break;
        }

        // 每扫描 100 个条目回调一次，保持界面子进度条运动
        scan_count += 1;
        if scan_count.is_multiple_of(100) {
            progress(scan_count);
        }

        let path = entry.path();

        // 跳过系统保护目录
        let p_str = path.to_string_lossy().to_uppercase();
        if p_str.contains("$RECYCLE.BIN") || p_str.contains("SYSTEM VOLUME INFORMATION") {
            continue;
        }

        if !entry.file_type().is_file() {
            continue;
        }

        // 跳过小文件（如果配置了）
        if let Ok(meta) = fs::metadata(path) {
            let file_size = meta.len();
            if skip_small && file_size < cfg.skip_small_threshold {
                continue;
            }

            // 按修改时间过滤
            if cutoff > 0 {
                if let Ok(mtime) = meta.modified() {
                    if let Ok(duration) = mtime.duration_since(std::time::UNIX_EPOCH) {
                        if (duration.as_secs() as i64) >= cutoff {
                            continue;
                        }
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            files.push(path.to_string_lossy().to_string());
        }
    }

    files
}

/// 计算文件的 CRC32 校验和（预留，当前未使用）
#[allow(dead_code)]
pub fn checksum_file(path: &Path) -> Result<u32, std::io::Error> {
    let mut hasher = Hasher::new();
    let mut file = File::open(path)?;
    with_buffer(|buffer| loop {
        let bytes_read = file.read(buffer)?;
        if bytes_read == 0 {
            break Ok(hasher.finalize());
        }
        hasher.update(&buffer[..bytes_read]);
    })
}

/// 刷新单个文件（智能模式 — 保持原内容，通过重写激活）
/// 小文件（≤ 缓冲区容量）整片读入→整片写回→整片读回校验
/// 大文件（> 缓冲区容量）分块原地覆写 + 分块读回校验
pub fn refresh_file(path: &str) -> Result<u64, String> {
    let p = Path::new(path);

    // 安全检查：跳过系统目录
    let p_upper = p.to_string_lossy().to_uppercase();
    if p_upper.contains("$RECYCLE.BIN") || p_upper.contains("SYSTEM VOLUME INFORMATION") {
        return Err("跳过系统保护目录".into());
    }

    let meta = fs::metadata(path)
        .map_err(|e| format!("获取文件信息失败: {}", e))?;
    let file_len = meta.len();
    if file_len == 0 {
        return Ok(0);
    }

    let start = Instant::now();
    let mem_limit = (get_config().memory_limit_mb as u64) * 1024 * 1024;

    if file_len <= mem_limit {
        // ── 小文件路径：整片读入 → 整片写回 → 整片读回验证 ──
        refresh_file_whole(path, p, file_len, &start)
    } else {
        // ── 大文件路径：分块原地覆写 + 分块读回验证 ──
        refresh_file_chunked(path, p, file_len, &start)
    }
}

/// 整片读写（小文件，≤ 缓冲区大小）
/// I/O：1 次 read_to_end + 1 次 write_all + 1 次 read_to_end 校验
fn refresh_file_whole(
    _path: &str, p: &Path, file_len: u64, start: &Instant,
) -> Result<u64, String> {
    // 第一阶段：整片读入 + 计算 CRC
    let data = std::fs::read(p)
        .map_err(|e| format!("读取文件失败: {}", e))?;
    let write_crc = crc32fast::hash(&data);
    let processed = file_len;

    // 第二阶段：整片写回
    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(p)
        .map_err(|e| format!("打开文件写入失败: {}", e))?;
    file.write_all(&data)
        .map_err(|e| format!("写入文件失败: {}", e))?;
    file.flush()
        .map_err(|e| format!("刷新文件失败: {}", e))?;
    drop(file);

    // 第三阶段：整片读回验证
    let verify_data = std::fs::read(p)
        .map_err(|e| format!("读取文件验证失败: {}", e))?;
    let verify_crc = crc32fast::hash(&verify_data);

    if write_crc != verify_crc {
        return Err("CRC 校验不匹配：文件可能损坏，建议从备份恢复".into());
    }

    let elapsed = start.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 {
        processed as f64 / (1024.0 * 1024.0) / elapsed
    } else {
        0.0
    };
    Ok((speed * 100.0).round() as u64)
}

/// 分块原地覆写（大文件，> 缓冲区大小）
/// I/O：分块 read → seek → write + 分块 read 校验
fn refresh_file_chunked(
    _path: &str, p: &Path, file_len: u64, start: &Instant,
) -> Result<u64, String> {
    let mut processed: u64 = 0;

    // 第一阶段：分块原地覆写
    let write_crc = {
        let mut hasher = Hasher::new();
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(p)
            .map_err(|e| format!("打开文件失败: {}", e))?;

        with_buffer(|buf| -> Result<(), String> {
            loop {
                let pos = file.stream_position()
                    .map_err(|e| format!("获取文件位置失败: {}", e))?;
                let n = file.read(buf)
                    .map_err(|e| format!("读取失败: {}", e))?;
                if n == 0 {
                    break Ok(());
                }
                hasher.update(&buf[..n]);
                file.seek(SeekFrom::Start(pos))
                    .map_err(|e| format!("定位失败: {}", e))?;
                file.write_all(&buf[..n])
                    .map_err(|e| format!("写入失败: {}", e))?;
                processed += n as u64;
            }
        })?;

        file.flush().map_err(|e| format!("刷新失败: {}", e))?;
        if file_len > 0 {
            file.set_len(file_len)
                .map_err(|e| format!("截断文件失败: {}", e))?;
        }

        hasher.finalize()
    };

    // 第二阶段：分块读回验证
    let verify_crc = {
        let mut file = File::open(p)
            .map_err(|e| format!("打开文件验证失败: {}", e))?;
        let mut hasher = Hasher::new();
        with_buffer(|buf| -> u32 {
            loop {
                match file.read(buf) {
                    Ok(0) => break hasher.finalize(),
                    Ok(n) => hasher.update(&buf[..n]),
                    Err(_e) => return hasher.finalize(), // 忽略读取错误，继续尝试
                }
            }
        })
    };

    if write_crc != verify_crc {
        return Err("CRC 校验不匹配：写入后数据不一致，文件可能损坏".into());
    }

    let elapsed = start.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 {
        processed as f64 / (1024.0 * 1024.0) / elapsed
    } else {
        0.0
    };
    Ok((speed * 100.0).round() as u64)
}

/// 全盘刷新文件（写入 FF 值） — 预留
#[allow(dead_code)]
pub fn full_refresh_file(path: &Path, size: u64) -> Result<(), String> {
    // 安全检查
    let p_upper = path.to_string_lossy().to_uppercase();
    if p_upper.contains("$RECYCLE.BIN") || p_upper.contains("SYSTEM VOLUME INFORMATION") {
        return Err("跳过系统保护目录".into());
    }

    if let Some(parent) = path.parent() {
        if !parent.exists() {
            return Err(format!("目录不存在: {}", parent.display()));
        }
    }

    let cfg = get_config();
    let temp_path = path.with_extension("tmp");

    let fill_block = cfg
        .full_refresh_pattern
        .repeat(cfg.buffer_size / cfg.full_refresh_pattern.len().max(1));

    {
        let mut file = File::create(&temp_path).map_err(|e| format!("创建临时文件失败: {}", e))?;
        let mut remaining = size;

        while remaining > 0 {
            let chunk = std::cmp::min(cfg.buffer_size as u64, remaining) as usize;
            file.write_all(&fill_block[..chunk])
                .map_err(|e| format!("写入失败: {}", e))?;
            remaining -= chunk as u64;
        }

        file.flush().map_err(|e| format!("刷新失败: {}", e))?;
    }

    fs::rename(&temp_path, path).map_err(|e| format!("替换文件失败: {}", e))?;
    Ok(())
}

/// 持续全盘写入模式（用于填充可用空间）
/// buf_kb: 写入缓冲区大小，单位 KB
pub fn continuous_full_refresh(path: &Path, target_size: u64, buf_kb: u64) -> Result<(u64, f64), String> {
    let p_upper = path.to_string_lossy().to_uppercase();
    if p_upper.contains("$RECYCLE.BIN") || p_upper.contains("SYSTEM VOLUME INFORMATION") {
        return Err("跳过系统保护目录".into());
    }

    if path.exists() {
        fs::remove_file(path).map_err(|e| format!("删除旧文件失败: {}", e))?;
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }

    let cfg = get_config();
    let start = Instant::now();
    let mut max_speed = 0.0_f64;
    let mut total_written = 0u64;
    let mut last_report = Instant::now();

    let mut file = File::create(path).map_err(|e| format!("创建文件失败: {}", e))?;
    let mut remaining = target_size;
    let buf_size = (buf_kb as usize) * 1024;

    // 使用用户指定大小的缓冲区，预填 0xFF
    let buffer = vec![cfg.full_refresh_pattern[0]; buf_size];

    while remaining > 0 {
        if INTERRUPTED.load(Ordering::Relaxed) {
            file.flush().ok();
            break;
        }

        let chunk = std::cmp::min(buf_size as u64, remaining) as usize;
        file.write_all(&buffer[..chunk])
            .map_err(|e| format!("写入失败: {}", e))?;
        total_written += chunk as u64;
        remaining -= chunk as u64;

            // 每秒报告速度
            if last_report.elapsed().as_secs_f64() >= 1.0 {
                let elapsed = start.elapsed().as_secs_f64();
                if elapsed > 0.0 {
                    let speed = total_written as f64 / (1024.0 * 1024.0) / elapsed;
                    max_speed = max_speed.max(speed);
                }
                last_report = Instant::now();
            }
        }

    file.flush().map_err(|e| format!("最终刷新失败: {}", e))?;
    let elapsed = start.elapsed().as_secs_f64();
    let final_speed = if elapsed > 0.0 {
        total_written as f64 / (1024.0 * 1024.0) / elapsed
    } else {
        0.0
    };
    max_speed = max_speed.max(final_speed);

    Ok((total_written, max_speed))
}
