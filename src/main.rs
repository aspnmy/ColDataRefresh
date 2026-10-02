//! ColDataRefresh — SSD 冷数据刷新工具入口。
//! CLI 参数解析（-cli 非交互 / 无参数交互）与运行参数构造。

// `////` 用作文档阅读用的区域分隔标记（让 docs 中的条目不被打断），
// 它并非文档注释，故会触发 clippy 的 four_forward_slashes 提示 —— 属有意为之，抑制之。
#![allow(clippy::four_forward_slashes)]

use clap::Parser;

mod app;
mod config;
mod dashboard;
mod file_op;
mod full_refresh;
mod log;
mod platform;
mod terminal;

#[derive(Parser, Debug)]
#[command(
    name = "coldatafresh",
    version = "5.0.3",
    about = "冷数据维护工具 - 优化SSD性能，延长使用寿命"
)]
//// ============================ CLI 模式参数 (CLI mode options) ============================
struct Args {
    /// 进入命令行（非交互）模式；必须同时指定 -p
    #[arg(long = "cli")]
    cli: bool,

    /// 目标目录路径（-cli 模式下必填；交互模式下忽略）
    #[arg(short = 'p', long)]
    path: Option<String>,

    /// 数据年龄阈值（天），超过此值的文件将被刷新
    #[arg(short = 'a', long)]
    age: Option<u32>,

    /// 是否跳过小于指定大小的文件（MB）
    #[arg(short = 's', long)]
    skip_smaller: Option<u64>,

    /// 是否执行全盘刷新
    #[arg(short = 'f', long)]
    full_refresh: bool,

    /// 是否执行TRIM操作
    #[arg(short = 't', long)]
    trim: bool,

    /// 是否启用详细日志
    #[arg(short = 'v', long)]
    verbose: bool,

    /// 非交互模式：确认执行破坏性操作（全盘刷新 / TRIM），无人值守必备
    #[arg(short = 'y', long)]
    yes: bool,

    /// 非交互模式：处理缓冲区大小（MB），仅在 CLI 模式下生效
    #[arg(short = 'b', long)]
    buffer_size: Option<u32>,

    /// 全盘刷新：保留已使用空间中的文件（备份→删除→填充→恢复）
    #[arg(long, conflicts_with = "no_keep_files")]
    keep_files: bool,

    /// 全盘刷新：不保留文件（数据不可恢复）
    #[arg(long)]
    no_keep_files: bool,

    /// 全盘刷新：额外填充空闲空间（覆写未分配空间，不可恢复）
    #[arg(long)]
    fill_free: bool,

    /// 全盘刷新：填充空闲空间时每个文件的写入容量（GB，1-100）
    #[arg(long, value_name = "GB", default_value_t = 50)]
    unit_gb: u64,

    /// 全盘刷新：写入缓冲区大小（KB，64~1048576）
    #[arg(long, value_name = "KB", default_value_t = 512)]
    write_buf_kb: u64,
}
//// ========================================================================================

fn main() {
    // 解析命令行参数（先解析，再副作用）
    let args = Args::parse();

    // 初始化日志系统（-v 提升到 debug 级）
    let default_level = if args.verbose { "debug" } else { "info" };
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(default_level),
    )
    .try_init();

    // 设置窗口标题
    terminal::Terminal::set_window_title("冷数据维护工具 v5.0.3");

    // 初始化日志器
    log::logger();

    //// ---------------------------- 运行参数构造 (build run options) ----------------------------
    // 构造运行参数
    let opts = app::RunOptions {
        cli: args.cli,
        path: args.path,
        full_refresh: args.full_refresh,
        trim: args.trim,
        age: args.age,
        skip_smaller: args.skip_smaller,
        yes: args.yes,
        buffer_size_mb: args.buffer_size,
        keep_files: args.keep_files,
        no_keep_files: args.no_keep_files,
        fill_free: args.fill_free,
        unit_gb: args.unit_gb,
        write_buf_kb: args.write_buf_kb,
    };

    //// ----------------------------------------------------------------------------------------
    // 启动应用程序
    let mut app = app::App::new();
    let code = app.run(opts);
    std::process::exit(code);
}
