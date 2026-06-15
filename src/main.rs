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
    version = "5.0.0",
    about = "冷数据维护工具 - 优化SSD性能，延长使用寿命"
)]
struct Args {
    /// 目标目录路径
    #[arg(short = 'p', long, default_value = ".")]
    path: String,

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
}

fn main() {
    // 解析命令行参数（先解析，再副作用）
    let args = Args::parse();

    // 初始化日志系统
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();

    // 设置窗口标题
    terminal::Terminal::set_window_title("冷数据维护工具 v5.0.0");

    // 初始化日志器
    log::logger();

    // 启动应用程序
    let mut app = app::App::new();
    app.run(args.full_refresh, args.trim, args.age, args.skip_smaller);
}
