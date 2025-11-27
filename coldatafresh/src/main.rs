mod config;
mod file_operator;
mod full_refresh_manager;
mod log_manager;
mod terminal_manager;
mod dashboard;
mod application_controller;

use clap::Parser;
use log::info;
use env_logger::Env;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, help = "运行基准测试模式")]
    benchmark: bool,
    
    #[arg(long, default_value = "./benchmark_test", help = "基准测试文件目录")]
    test_dir: String,
    
    #[arg(long, default_value = "3", help = "基准测试迭代次数")]
    iterations: u32,
    
    #[arg(long, help = "创建测试文件用于基准测试")]
    create_test_files: bool,
    
    #[arg(long, help = "使用全盘数据刷新模式")]
    full_refresh: bool,
    
    #[arg(long, help = "使用TRIM功能")]
    trim_mode: bool,
    
    #[arg(long, help = "测试文件写入功能")]
    test: bool,
    
    #[arg(long, default_value = "H:", help = "测试目录")]
    dir: String,
    
    #[arg(long, default_value = "50", help = "测试文件大小(GB)")]
    size: u32,
}

fn main() {
    // 初始化日志系统
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    
    info!("冷数据维护工具 v4.7.0 启动");
    
    // 解析命令行参数
    let args = Args::parse();
    
    // 运行应用程序控制器
    let mut controller = application_controller::ApplicationController::new();
    controller.execute(args.full_refresh, args.trim_mode);
}
