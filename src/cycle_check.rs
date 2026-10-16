//! 跑圈校验业务流程：写入(1GB 随机数据 + 校验码) → 校验 → 保存报告 → 删除 → TRIM = 一圈。
//!
//! 与全盘刷新模式的区别：全盘刷新只做写入（填充/刷新）；跑圈校验在写入时**留下校验码**，
//! 写满后**按校验码逐块回读校验**，用于定位具体哪个 1GB 区块出错（Nano 颗粒坏块排查）。

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::config::format_size;
use crate::log::logger;
use crate::platform;

//// ============================ 跑圈校验 (cycle check) ============================
/// 单个区块的校验结果
#[derive(Debug, Clone)]
pub struct BlockResult {
    /// 区块序号（从 1 开始）
    pub index: u64,
    /// 区块文件路径
    pub path: String,
    /// 写入字节数
    pub bytes: u64,
    /// 期望校验码（写入时计算）
    pub expected: u32,
    /// 实际校验码（回读时计算）
    pub actual: u32,
    /// 该区块是否校验通过
    pub ok: bool,
}

/// 单圈统计
#[derive(Debug, Clone, Default)]
pub struct CycleStat {
    /// 圈序号（从 1 开始）
    pub cycle: u64,
    /// 写入区块总数
    pub blocks: u64,
    /// 写入总字节数
    pub written_bytes: u64,
    /// 校验通过区块数
    pub passed: u64,
    /// 校验失败区块数
    pub failed: u64,
    /// 写入耗时（秒）
    pub write_secs: f64,
    /// 校验耗时（秒）
    pub verify_secs: f64,
    /// 校验失败的区块明细
    pub bad_blocks: Vec<BlockResult>,
}

/// 跑圈校验管理器
pub struct CycleCheck;

impl CycleCheck {
    /// 执行跑圈校验：共 cycles 圈，每圈 = 写入 → 校验 → 保存报告 → 删除 → TRIM。
    /// 返回全部圈的统计；空 vec 表示首圈即失败（调用方据此定退出码）。
    pub fn execute(directory: &str, cycles: u64, unit_gb: u64, write_buf_kb: u64) -> Vec<CycleStat> {
        logger().log(
            &format!(
                "开始跑圈校验模式: 路径='{}', 圈数={}, 单元={}GB, 缓冲区={}KB",
                directory, cycles, unit_gb, write_buf_kb
            ),
            "INFO",
        );

        let dir_path = Path::new(directory);
        let unit_bytes = unit_gb * 1024u64.pow(3);
        let mut stats: Vec<CycleStat> = Vec::new();

        // ── 块设备直写后端（issues #3/#4：绕过文件系统，避免元数据 / 页缓存导致卡死） ──
        // 若目标路径能解析出块设备且通过安全闸门，则整圈走直写 + 直读校验。
        if let Some(dev) = platform::resolve_block_device(directory) {
            match crate::block_dev::BlockDevice::open(&dev) {
                Ok(bd) => {
                    println!(
                        "直写后端 (direct-write backend): {} (扇区 {}B, 容量 {})",
                        dev,
                        bd.alignment(),
                        crate::config::format_size(bd.info().capacity)
                    );
                    for cycle in 1..=cycles {
                        println!("\n{}", "=".repeat(50));
                        println!("          第 {}/{} 圈 (Cycle {}/{})", cycle, cycles, cycle, cycles);
                        println!("{}", "=".repeat(50));

                        // ① 直写
                        let t0 = std::time::Instant::now();
                        let (units, written) = match crate::block_dev::direct_fill(
                            &bd, 0, bd.info().capacity, unit_bytes, write_buf_kb,
                        ) {
                            Ok(v) => v,
                            Err(e) => {
                                println!("\n❌ 直写失败 (direct fill failed): {}", e);
                                stats.push(CycleStat {
                                    cycle,
                                    blocks: 0,
                                    written_bytes: 0,
                                    passed: 0,
                                    failed: 1,
                                    write_secs: t0.elapsed().as_secs_f64(),
                                    verify_secs: 0.0,
                                    bad_blocks: Vec::new(),
                                });
                                break;
                            }
                        };
                        let write_secs = t0.elapsed().as_secs_f64();
                        println!(
                            "\n[圈{}][写入] 直写完成: {} 个单元, 累积 {} ({} 秒)",
                            cycle, units, crate::config::format_size(written), write_secs as u64
                        );

                        // ② 直读校验
                        let t1 = std::time::Instant::now();
                        let verify_result = crate::block_dev::direct_verify(
                            &bd, 0, bd.info().capacity, write_buf_kb,
                        );
                        let verify_secs = t1.elapsed().as_secs_f64();
                        let (passed, failed) = match verify_result {
                            Ok(read) => {
                                println!(
                                    "[圈{}][校验] 直读 {} 完成 ({} 秒)",
                                    cycle, crate::config::format_size(read), verify_secs as u64
                                );
                                (units, 0u64)
                            }
                            Err(e) => {
                                println!("[圈{}][校验] 直读失败 (direct read failed): {}", cycle, e);
                                (0u64, 1u64)
                            }
                        };

                        let stat = CycleStat {
                            cycle,
                            blocks: units,
                            written_bytes: written,
                            passed,
                            failed,
                            write_secs,
                            verify_secs,
                            bad_blocks: Vec::new(),
                        };
                        let report = save_cycle_report(directory, &stat);
                        println!("[圈{}][报告] 已保存 (report saved): {}", cycle, report);
                        stats.push(stat);

                        // ④ 删除：直写后端不落文件，只需块层 discard 回收
                        // ⑤ TRIM：块层直下（issue #5）
                        println!("[圈{}][TRIM] 块层直下 (BLKDISCARD)...", cycle);
                        match bd.discard_all() {
                            Ok(()) => println!("[圈{}][TRIM] 完成", cycle),
                            Err(e) => println!("⚠️  [圈{}][TRIM] 失败 (discard failed): {}", cycle, e),
                        }
                    }

                    // 直写后端完成 ⇒ 直接汇总返回
                    Self::summarize(&stats);
                    return stats;
                }
                Err(e) => {
                    println!("直写不可用 (direct-write unavailable): {}，回退文件系统路径 (fallback)", e);
                }
            }
        }

        // ── 文件系统路径（回退，保持原行为） ──
        println!("使用文件系统路径 (filesystem path)");
        for cycle in 1..=cycles {
            println!("\n{}", "=".repeat(50));
            println!(
                "          第 {}/{} 圈 (Cycle {}/{})",
                cycle, cycles, cycle, cycles
            );
            println!("{}", "=".repeat(50));

            // ---- ① 写入：1GB 随机数据 + 计算校验码 ----
            let write_start = std::time::Instant::now();
            let write_outcome = write_blocks(dir_path, unit_bytes, write_buf_kb);
            let write_secs = write_start.elapsed().as_secs_f64();

            let (blocks, written_bytes, block_results) = match write_outcome {
                Ok(v) => v,
                Err(e) => {
                    println!("\n❌ 写入失败 (write failed): {}", e);
                    logger().log(&format!("跑圈校验写入失败: {}", e), "ERROR");
                    // 写入阶段失败：记录本圈后终止后续圈（无法继续校验）
                    // 失败前已写入的区块数/字节数由 write_blocks 内部统计，此处仅记失败标记
                    stats.push(CycleStat {
                        cycle,
                        blocks: 0,
                        written_bytes: 0,
                        passed: 0,
                        failed: 1,
                        write_secs,
                        verify_secs: 0.0,
                        bad_blocks: Vec::new(),
                    });
                    break;
                }
            };
            if blocks == 0 {
                println!("\n⚠️  空间不足，无法写入任何区块 (no space for any block)，终止");
                stats.push(CycleStat {
                    cycle,
                    blocks: 0,
                    written_bytes: 0,
                    passed: 0,
                    failed: 0,
                    write_secs,
                    verify_secs: 0.0,
                    bad_blocks: Vec::new(),
                });
                break;
            }
            println!(
                "\n[圈{}][写入] 完成: {} 个区块, 累积 {} ({} 秒)",
                cycle,
                blocks,
                format_size(written_bytes),
                write_secs as u64
            );

            // ---- ② 校验：按校验码逐块回读比对 ----
            let verify_start = std::time::Instant::now();
            let (passed, failed, bad_blocks) = verify_blocks(&block_results, write_buf_kb);
            let verify_secs = verify_start.elapsed().as_secs_f64();
            println!(
                "[圈{}][校验] 完成: 通过 {} 个, 失败 {} 个 ({} 秒)",
                cycle,
                passed,
                failed,
                verify_secs as u64
            );

            let mut stat = CycleStat {
                cycle,
                blocks,
                written_bytes,
                passed,
                failed,
                write_secs,
                verify_secs,
                bad_blocks,
            };

            // ---- ③ 保存校验报告到程序主体 logs/ 下 ----
            let report = save_cycle_report(directory, &stat);
            println!("[圈{}][报告] 已保存 (report saved): {}", cycle, report);
            stat.bad_blocks = stat.bad_blocks.clone();
            stats.push(stat.clone());

            // ---- ④ 删除 ----
            println!("[圈{}][删除] 开始 (deleting)...", cycle);
            match delete_blocks(dir_path) {
                Ok((success, failure)) => {
                    println!(
                        "[圈{}][删除] 完成: 成功 {} 个, 失败 {} 个",
                        cycle, success, failure
                    );
                }
                Err(e) => {
                    println!("⚠️  [圈{}][删除] 失败 (delete failed): {}", cycle, e);
                }
            }

            // ---- ⑤ TRIM ----
            println!("[圈{}][TRIM] 开始 (running fstrim)...", cycle);
            if let Some(device) = platform::resolve_device_name(directory) {
                platform::trim_volume(&device);
            }
            println!("[圈{}][TRIM] 完成", cycle);
        }

        // 汇总
        Self::summarize(&stats);
        stats
    }

    /// 打印跑圈校验汇总（直写后端与文件系统路径共用）
    fn summarize(stats: &[CycleStat]) {
        println!("\n{}", "=".repeat(50));
        println!("          跑圈校验完成 (Cycle Check Completed)");
        println!("{}", "=".repeat(50));
        for s in stats {
            println!(
                "圈{} (Cycle {}): 区块 {} 个, 写入 {}, 通过 {}, 失败 {}",
                s.cycle,
                s.cycle,
                s.blocks,
                format_size(s.written_bytes),
                s.passed,
                s.failed
            );
        }
        let total_failed: u64 = stats.iter().map(|s| s.failed).sum();
        if total_failed > 0 {
            println!("\n⚠️  发现校验失败区块 (checksum mismatches found): {}", total_failed);
            for s in stats {
                for b in &s.bad_blocks {
                    println!(
                        "   [圈{}] 区块 {} (block {}): {} — 期望 {:08X}, 实际 {:08X}",
                        s.cycle, b.index, b.index, b.path, b.expected, b.actual
                    );
                }
            }
        }
        println!("{}", "=".repeat(50));
    }
}

/// 写入区块：每个区块写 unit_bytes 的随机数据，同时计算 CRC32 校验码。
/// 返回 (区块数, 写入总字节, 各区块结果)
fn write_blocks(
    dir_path: &Path,
    unit_bytes: u64,
    buf_kb: u64,
) -> Result<(u64, u64, Vec<BlockResult>), String> {
    use std::fs;

    let work_dir = dir_path.join("$aspnmytools");
    fs::create_dir_all(&work_dir).map_err(|e| format!("创建写入目录失败: {}", e))?;

    let buf_size = (buf_kb * 1024).max(64 * 1024) as usize;
    let mut buf = vec![0u8; buf_size];
    let mut results: Vec<BlockResult> = Vec::new();
    let mut index = 0u64;
    let mut written_total = 0u64;

    loop {
        // 空间不足一个单元即停（写满为止）
        let (_total, _used, free) = platform::get_disk_space(dir_path);
        if free < unit_bytes {
            break;
        }

        index += 1;
        let file_path = work_dir.join(format!("cycle_{:05}.dat", index));
        let path_str = file_path.display().to_string();

        let mut file = fs::File::create(&file_path)
            .map_err(|e| format!("创建区块文件失败 {}: {}", path_str, e))?;

        // 逐缓冲区写入随机数据，边写边算 CRC32
        let mut hasher = crc32fast::Hasher::new();
        let mut remaining = unit_bytes;
        while remaining > 0 {
            let n = buf_size.min(remaining as usize);
            // 随机数据源：/dev/urandom（每次填充取新随机）
            fill_random(&mut buf[..n])?;
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])
                .map_err(|e| format!("写入失败 {}: {}", path_str, e))?;
            remaining -= n as u64;
        }
        file.flush()
            .map_err(|e| format!("flush 失败 {}: {}", path_str, e))?;
        // 落盘，确保后续回读的是真实介质内容（排查"写入未落盘"类错误）
        file.sync_all()
            .map_err(|e| format!("sync 失败 {}: {}", path_str, e))?;
        drop(file);

        let expected = hasher.finalize();
        written_total += unit_bytes;
        results.push(BlockResult {
            index,
            path: path_str,
            bytes: unit_bytes,
            expected,
            actual: 0,
            ok: false,
        });

        if index.is_multiple_of(10) {
            println!(
                "  已写入 {} 个区块, 累积 {}",
                index,
                format_size(written_total)
            );
        }
    }

    Ok((index, written_total, results))
}

/// 校验区块：按写入时记录的校验码逐块回读比对。
/// 返回 (通过数, 失败数, 失败明细)
pub fn verify_blocks(blocks: &[BlockResult], buf_kb: u64) -> (u64, u64, Vec<BlockResult>) {
    use std::fs;

    let buf_size = (buf_kb * 1024).max(64 * 1024) as usize;
    let mut buf = vec![0u8; buf_size];
    let mut passed = 0u64;
    let mut failed = 0u64;
    let mut bad: Vec<BlockResult> = Vec::new();

    for b in blocks {
        let mut hasher = crc32fast::Hasher::new();
        let mut read_ok = true;

        match fs::File::open(&b.path) {
            Ok(mut file) => {
                let mut remaining = b.bytes;
                while remaining > 0 {
                    let n = buf_size.min(remaining as usize);
                    match file.read_exact(&mut buf[..n]) {
                        Ok(()) => {
                            hasher.update(&buf[..n]);
                            remaining -= n as u64;
                        }
                        Err(e) => {
                            logger().log(
                                &format!("回读失败 {}: {}", b.path, e),
                                "WARNING",
                            );
                            read_ok = false;
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                logger().log(&format!("打开区块失败 {}: {}", b.path, e), "WARNING");
                read_ok = false;
            }
        }

        let actual = if read_ok { hasher.finalize() } else { 0 };
        let ok = read_ok && actual == b.expected;
        let mut item = b.clone();
        item.actual = actual;
        item.ok = ok;

        if ok {
            passed += 1;
        } else {
            failed += 1;
            println!(
                "  ❌ 区块 {} 校验失败 (block {} mismatch): {} — 期望 {:08X}, 实际 {:08X}",
                b.index, b.index, b.path, b.expected, actual
            );
            bad.push(item);
        }
    }

    (passed, failed, bad)
}

/// 保存单圈校验报告到程序主体 logs/ 目录下。
/// 返回报告文件路径。
fn save_cycle_report(directory: &str, stat: &CycleStat) -> String {
    use std::fs::{self, OpenOptions};

    let cfg = crate::config::config();
    let log_dir = cfg
        .error_log
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("logs"));
    let _ = fs::create_dir_all(&log_dir);

    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let report_path = log_dir.join(format!("cycle-check-{}-cycle{}.json", ts, stat.cycle));
    let path_str = report_path.display().to_string();

    // 坏块明细（定位到具体 1GB 区块）
    let bad: Vec<serde_json::Value> = stat
        .bad_blocks
        .iter()
        .map(|b| {
            serde_json::json!({
                "index": b.index,
                "path": b.path,
                "bytes": b.bytes,
                "expected_crc32": format!("{:08X}", b.expected),
                "actual_crc32": format!("{:08X}", b.actual),
            })
        })
        .collect();

    let doc = serde_json::json!({
        "timestamp": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
        "mode": "cycle-check",
        "target": directory,
        "cycle": stat.cycle,
        "blocks": stat.blocks,
        "written_bytes": stat.written_bytes,
        "written_human": format_size(stat.written_bytes),
        "passed": stat.passed,
        "failed": stat.failed,
        "write_seconds": (stat.write_secs * 100.0).round() / 100.0,
        "verify_seconds": (stat.verify_secs * 100.0).round() / 100.0,
        "bad_blocks": bad,
        "verdict": if stat.failed == 0 { "all blocks ok" } else { "bad blocks found" },
    });

    match OpenOptions::new().create(true).append(true).open(&report_path) {
        Ok(mut f) => {
            if let Err(e) = writeln!(f, "{}", doc) {
                logger().log(&format!("写入校验报告失败 {}: {}", path_str, e), "WARNING");
            }
        }
        Err(e) => {
            logger().log(&format!("创建校验报告失败 {}: {}", path_str, e), "WARNING");
        }
    }

    logger().log(
        &format!(
            "跑圈校验报告: 圈={}, 区块={}, 通过={}, 失败={}, 报告={}",
            stat.cycle, stat.blocks, stat.passed, stat.failed, path_str
        ),
        "INFO",
    );

    path_str
}

/// 删除本模式写入的全部区块文件（含工作目录）
fn delete_blocks(dir_path: &Path) -> Result<(u64, u64), String> {
    let work_dir = dir_path.join("$aspnmytools");
    if !work_dir.exists() {
        return Ok((0, 0));
    }

    let mut success = 0u64;
    let mut failure = 0u64;

    let entries: Vec<_> = walkdir::WalkDir::new(&work_dir)
        .contents_first(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .collect();

    for entry in entries {
        let path = entry.path();
        let result = if entry.file_type().is_dir() {
            std::fs::remove_dir(path)
        } else {
            std::fs::remove_file(path)
        };
        match result {
            Ok(()) => success += 1,
            Err(e) => {
                failure += 1;
                logger().log(&format!("删除区块失败 {}: {}", path.display(), e), "WARNING");
            }
        }
    }

    Ok((success, failure))
}

/// 填充随机数据（优先 /dev/urandom；不可用时退回基于时间的伪随机）
fn fill_random(buf: &mut [u8]) -> Result<(), String> {
    use std::fs::File;
    match File::open("/dev/urandom") {
        Ok(mut f) => {
            f.read_exact(buf).map_err(|e| format!("读取 /dev/urandom 失败: {}", e))?;
            Ok(())
        }
        Err(_) => {
            // 非 Unix 或 /dev/urandom 不可用：用简单 LCG 生成伪随机（仍保证逐次不同）
            let mut seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9E3779B97F4A7C15)
                | 1;
            for chunk in buf.chunks_mut(8) {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let bytes = seed.to_le_bytes();
                let n = chunk.len();
                chunk.copy_from_slice(&bytes[..n]);
            }
            Ok(())
        }
    }
}

//// ==============================================================================

/// 供外部（如按偏移定点复检）使用的读取辅助 —— 保留占位，当前未接线
#[allow(dead_code)]
pub fn reread_at(path: &str, offset: u64, len: usize) -> std::io::Result<Vec<u8>> {
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; len];
    f.read_exact(&mut buf)?;
    Ok(buf)
}
