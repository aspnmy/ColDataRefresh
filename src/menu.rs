//! 交互界面层 —— 菜单、确认、提问（issue #9：与 CLI 业务流程分离管理）。
//!
//! 本文件**只放界面**：无业务逻辑、不依赖 `app`。
//! 调用方向单一 = `app.rs` → `menu.rs`（app 决定何时问、拿回值后执行）。
//! ⛔ `-cli` 路径**不得**调用本文件的任何函数（CLI 参数一律来自命令行，不读 stdin）。

use std::io::{self, Write};

use crate::full_refresh::FullRefreshParams;
use crate::log::logger;
use crate::platform;

//// ============================ 界面层 (UI layer) ============================

/// 模式选择结果（替代原先的 `(bool, bool, bool)` 元组 —— 加第 5 项后可读性差）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeChoice {
    /// 智能模式
    Smart,
    /// 全盘刷新
    FullRefresh,
    /// TRIM 优化
    Trim,
    /// 跑圈校验
    CycleCheck,
    /// LBA 热力图
    Heatmap,
}

/// 热力图交互参数
#[derive(Debug, Clone, Copy)]
pub struct HeatmapParams {
    /// 分桶粒度（MiB）
    pub bucket_mib: u64,
    /// 热桶阈值（访问次数）
    pub hot_threshold: u64,
    /// 温桶阈值（访问次数）
    pub warm_threshold: u64,
}

impl Default for HeatmapParams {
    fn default() -> Self {
        Self {
            bucket_mib: 64,
            hot_threshold: 100,
            warm_threshold: 10,
        }
    }
}

/// 打印启动横幅
pub fn show_startup_banner() {
    let admin = if platform::is_admin() {
        " [管理员/Admin]"
    } else {
        ""
    };
    let os_info = platform::get_os_display();
    let eq = "=".repeat(50);

    println!("{}", eq);
    println!("SSD掉速激活-冷数据维护系统 v5.0.3{}", admin);
    println!("运行平台 (Platform): {}", os_info);
    println!("作者 (Author): support@e2bank.cn  QQ群 (Group): 115405294");
    println!("GitHub: https://github.com/aspnmy/ColDataRefresh");
    println!("{}", eq);
}

/// 模式选择菜单 —— 返回用户选择的模式
pub fn show_mode_menu() -> ModeChoice {
    crate::terminal::Terminal::clear();
    let admin = if platform::is_admin() { " [管理员/Admin]" } else { "" };
    let os_info = platform::get_os_display();
    let eq = "=".repeat(50);

    println!("{}", eq);
    println!("SSD掉速激活-冷数据维护系统 v5.0.3{}", admin);
    println!("运行平台 (Platform): {}  |  作者 (Author): support@e2bank.cn  QQ群 (Group): 115405294", os_info);
    println!("GitHub: https://github.com/aspnmy/ColDataRefresh");
    println!("{}", eq);

    println!("\n{}", eq);
    println!("          冷数据维护工具 - 操作模式选择 (Cold Data Tool - Mode Selection)");
    println!("{}", eq);
    println!("1. 智能模式 (Smart Mode) (推荐/Recommended) - 保留原文件内容，仅激活冷数据 (keep contents, refresh cold data only)");
    println!("2. 全盘刷新模式 (Full Refresh Mode) (所有文件全部丢失无法找回/ALL DATA LOST, UNRECOVERABLE) - 将文件内容替换为 FF 值 (overwrite with 0xFF)");
    println!("3. TRIM优化模式 (TRIM Optimization Mode) (如需找回数据不要使用/CANNOT recover deleted data) - 通知SSD哪些数据块无效 (notify SSD which blocks are invalid)");
    println!("4. 跑圈校验模式 (Cycle Check Mode) (所有文件全部丢失无法找回/ALL DATA LOST, UNRECOVERABLE) - 1GB随机数据+校验码，写满后回读校验以定位坏块 (locate bad blocks)");
    println!("5. LBA热力图模式 (LBA Heat Map Mode) (只读采样/READ-ONLY) - 抓取块层 IO 按 LBA 分桶统计冷热，用于定向刷新冷块 (sample LBA hotness)");
    println!("{}", eq);

    loop {
        print!("请选择操作模式 (Select mode) [1/2/3/4/5]: ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        match input.trim() {
            "1" => {
                logger().log("用户选择操作模式: 智能模式", "INFO");
                return ModeChoice::Smart;
            }
            "2" => {
                logger().log("用户选择操作模式: 全盘刷新模式", "INFO");
                return ModeChoice::FullRefresh;
            }
            "3" => {
                logger().log("用户选择操作模式: TRIM模式", "INFO");
                return ModeChoice::Trim;
            }
            "4" => {
                logger().log("用户选择操作模式: 跑圈校验模式", "INFO");
                return ModeChoice::CycleCheck;
            }
            "5" => {
                logger().log("用户选择操作模式: LBA 热力图模式", "INFO");
                return ModeChoice::Heatmap;
            }
            _ => println!("无效的选择，请输入 1、2、3、4 或 5 (Invalid choice, enter 1, 2, 3, 4 or 5)"),
        }
    }
}

/// 全盘刷新确认 —— 两次 `yes`
pub fn confirm_full_refresh() -> bool {
    println!("\n⚠️  警告 (WARNING): 正在使用全盘数据刷新模式 (Full Refresh Mode)！");
    println!("   此模式将完全擦除SSD数据，所有文件内容将丢失且无法找回 (erases all data, unrecoverable)！");
    println!();

    print!("请输入 'yes' 确认执行全盘刷新操作 (第一次) (type 'yes' to confirm, 1st): ");
    io::stdout().flush().ok();
    let mut c1 = String::new();
    io::stdin().read_line(&mut c1).ok();
    if c1.trim().to_lowercase() != "yes" {
        println!("操作已取消 (Cancelled)");
        return false;
    }

    print!("请再次输入 'yes' 确认执行全盘刷新操作 (第二次) (type 'yes' again, 2nd): ");
    io::stdout().flush().ok();
    let mut c2 = String::new();
    io::stdin().read_line(&mut c2).ok();
    if c2.trim().to_lowercase() != "yes" {
        println!("操作已取消 (Cancelled)");
        return false;
    }

    logger().log("用户已确认两次，开始执行全盘刷新操作", "INFO");
    true
}

/// TRIM 确认
pub fn confirm_trim() -> bool {
    println!("\n⚠️  警告 (WARNING): 正在使用TRIM优化模式 (TRIM Optimization)！");
    println!("   如需找回SSD中删除的数据请勿使用此模式 (do NOT use if you need to recover data)！");
    println!();

    print!("请输入 'yes' 确认 (type 'yes' to confirm): ");
    io::stdout().flush().ok();
    let mut c = String::new();
    io::stdin().read_line(&mut c).ok();
    if c.trim().to_lowercase() != "yes" {
        println!("操作已取消 (Cancelled)");
        return false;
    }

    logger().log("用户已确认，开始执行TRIM优化操作", "INFO");
    true
}

/// 跑圈校验确认 —— 破坏性操作
pub fn confirm_cycle_check() -> bool {
    println!("\n⚠️  警告 (WARNING): 正在使用跑圈校验模式 (Cycle Check Mode)！");
    println!("   此模式会写入 1GB 随机数据并覆写整盘空间，所有文件内容将丢失且无法找回");
    println!("   (writes 1GB random blocks over free space; ALL DATA LOST, UNRECOVERABLE)！");
    println!();

    print!("请输入 'yes' 确认执行跑圈校验操作 (type 'yes' to confirm): ");
    io::stdout().flush().ok();
    let mut c = String::new();
    io::stdin().read_line(&mut c).ok();
    if c.trim().to_lowercase() != "yes" {
        println!("操作已取消 (Cancelled)");
        return false;
    }

    logger().log("用户已确认，开始执行跑圈校验操作", "INFO");
    true
}

/// 询问扫描目录
pub fn prompt_directory() -> String {
    print!("扫描目录 (Scan directory): ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
    let dir = input.trim().trim_matches('"').to_string();

    // 补齐路径分隔符
    if !dir.ends_with(&['\\', '/'][..]) {
        dir + "\\"
    } else {
        dir
    }
}

/// 询问数据时效（天）
pub fn prompt_age() -> u32 {
    print!("数据时效/天 (Data age in days): ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
    input.trim().parse().unwrap_or(0)
}

/// 询问是否跳过小文件
pub fn prompt_skip_small() -> bool {
    print!("跳过小文件? (Skip small files?) (y/n): ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
    input.trim().to_lowercase() == "y"
}

/// 询问缓冲区大小（MB）
pub fn prompt_buffer_size() -> u32 {
    print!("处理缓冲区大小/MB (Buffer size, default 512, max 2048): ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
    let val: u32 = input.trim().parse().unwrap_or(512);
    val.clamp(64, 2048)
}

/// 询问热力图参数（分桶粒度 + 热/温阈值）
///
/// ⚠️ 采样是**只读**操作 ⇒ ⛔ 不需要 `yes` 确认（与全盘刷新 / TRIM / 跑圈校验不同）。
pub fn prompt_heatmap_params() -> HeatmapParams {
    let d = HeatmapParams::default();

    print!(
        "\n分桶粒度/MiB (Bucket size in MiB, default {}): ",
        d.bucket_mib
    );
    io::stdout().flush().ok();
    let mut b = String::new();
    io::stdin().read_line(&mut b).ok();
    let bucket_mib = b.trim().parse::<u64>().unwrap_or(d.bucket_mib).clamp(1, 1024);

    print!(
        "热桶阈值/次数 (Hot threshold, >=此值为 hot, default {}): ",
        d.hot_threshold
    );
    io::stdout().flush().ok();
    let mut h = String::new();
    io::stdin().read_line(&mut h).ok();
    let hot_threshold = h
        .trim()
        .parse::<u64>()
        .unwrap_or(d.hot_threshold)
        .max(1);

    print!(
        "温桶阈值/次数 (Warm threshold, >=此值为 warm, default {}): ",
        d.warm_threshold
    );
    io::stdout().flush().ok();
    let mut w = String::new();
    io::stdin().read_line(&mut w).ok();
    let warm_threshold = w
        .trim()
        .parse::<u64>()
        .unwrap_or(d.warm_threshold)
        .max(1);

    HeatmapParams {
        bucket_mib,
        hot_threshold,
        warm_threshold,
    }
}

/// 询问是否返回菜单
pub fn ask_return_to_menu() -> bool {
    crate::terminal::Terminal::clear();
    println!("\n{}", "=".repeat(50));
    println!("1. 返回 - 回到交互界面 (Back to menu)");
    println!("2. 退出 - 关闭程序 (Exit)");
    println!("{}", "=".repeat(50));

    loop {
        print!("请选择操作 (Select action) [1/2]: ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        match input.trim() {
            "1" => return true,
            "2" => {
                println!("感谢使用冷数据维护工具，再见！(Thanks for using, goodbye!)");
                return false;
            }
            _ => println!("无效的选择，请输入 1 或 2 (Invalid choice, enter 1 or 2)"),
        }
    }
}

/// 全盘刷新参数询问（交互模式用；CLI 路径**不调用**本函数）
pub fn prompt_full_refresh_params() -> FullRefreshParams {
    // 询问是否保留文件
    print!("\n是否保留已使用空间中的文件? (Keep existing files?) (Y/N, 默认/default Y): ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
    let keep_files = !input.trim().to_lowercase().starts_with('n');

    // 询问是否额外填充空闲空间
    print!("\n是否同时填充空闲空间? (Also fill free space?) (Y/N, 默认/default N): ");
    io::stdout().flush().ok();
    let mut fill_input = String::new();
    io::stdin().read_line(&mut fill_input).ok();
    let want_fill = fill_input.trim().to_lowercase().starts_with('y');

    // 填充属不可恢复操作，需二次确认
    let fill_free = if want_fill {
        print!("\n⚠️  填充空闲空间将覆写所有未分配空间，数据无法还原 (unrecoverable)！\n是否确认执行? (Confirm?) (Y/N, 默认/default N): ");
        io::stdout().flush().ok();
        let mut confirm = String::new();
        io::stdin().read_line(&mut confirm).ok();
        confirm.trim().to_lowercase().starts_with('y')
    } else {
        false
    };

    // 写入参数仅在填充空闲空间时需要
    let (unit_gb, write_buf_kb) = if fill_free {
        print!("\n请输入每个文件的写入容量 (Write size per file) (1-100GB, 默认/default 50GB): ");
        io::stdout().flush().ok();
        let mut cap = String::new();
        io::stdin().read_line(&mut cap).ok();
        let val: u64 = cap.trim().parse().unwrap_or(50);
        let unit_gb = val.clamp(1, 100);

        print!("\n请输入写入缓冲区大小 (Write buffer size) (64KB~1GB, 默认/default 512MB): ");
        io::stdout().flush().ok();
        let mut buf_input = String::new();
        io::stdin().read_line(&mut buf_input).ok();
        let buf_val: u64 = buf_input.trim().parse().unwrap_or(512);
        let write_buf_kb = buf_val.clamp(1, 1024) * 1024;

        (unit_gb, write_buf_kb)
    } else {
        (0, 64)
    };

    FullRefreshParams {
        keep_files,
        fill_free,
        unit_gb,
        write_buf_kb,
    }
}

//// ========================== 执行中动画 (spinner) ==========================
/// 在后台线程执行 `work`，主线程显示旋转动画，返回 `work` 的真实结果。
///
/// 抽自原 `run_trim_mode` 的样板（`Arc<AtomicBool>` + `Mutex`），供后续长任务复用。
pub fn run_with_spinner<F>(label: &str, work: F) -> bool
where
    F: FnOnce() -> bool + Send + 'static,
{
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    let done = Arc::new(AtomicBool::new(false));
    let outcome = Arc::new(Mutex::new(false));

    let done_clone = done.clone();
    let outcome_clone = outcome.clone();
    let handle = std::thread::spawn(move || {
        let ok = work();
        if let Ok(mut slot) = outcome_clone.lock() {
            *slot = ok;
        }
        done_clone.store(true, Ordering::Relaxed);
    });

    let spinner = ['|', '/', '-', '\\'];
    let mut i = 0usize;
    while !done.load(Ordering::Relaxed) {
        print!(
            "\r{} {} 请耐心等待 (please wait)...",
            label,
            spinner[i % 4]
        );
        io::stdout().flush().ok();
        std::thread::sleep(std::time::Duration::from_millis(500));
        i += 1;
    }
    let _ = handle.join();

    let ok = *outcome.lock().unwrap_or_else(|e| e.into_inner());
    println!(
        "\r{} {}                    ",
        label,
        if ok { "✅ 完成 (Done)" } else { "❌ 失败 (Failed)" }
    );
    ok
}

//// ============================================================================
