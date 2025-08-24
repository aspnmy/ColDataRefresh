#!/usr/bin/env python3
# coldatafresh.py - 冷数据维护专业工具 v4.3
# 终极版：解决日志保存问题，增强进度显示

import os
import sys
import time
import zlib
import ctypes
import signal
from dataclasses import dataclass
from typing import TypedDict
from datetime import datetime
from types import FrameType
from enum import Enum, auto
import json

def set_window_title(title: str = "冷数据维护工具 v4.3") -> None:
    """设置控制台窗口标题"""
    if os.name == 'nt':
        ctypes.windll.kernel32.SetConsoleTitleW(title)

# ============================== 系统配置模块 ==============================
@dataclass(frozen=True)
class Config:
    LOG_FILE: str = "refresh_log.json"
    CORRUPTED_LOG: str = "corrupted_files.log"
    BUFFER_SIZE: int = 4 * 1024
    MAX_RETRIES: int = 3
    LARGE_FILE: int = 100 * 1024**2      # 100MB以上为大文件
    MEDIUM_FILE: int = 10 * 1024**2       # 10MB-100MB为中等文件
    REPORT_INTERVAL: float = 0.2
    SKIP_SMALL: int = 1 * 1024**2        # 1MB以下为小文件（可跳过）

class FileCategory(Enum):
    SMALL = auto()
    MEDIUM = auto()
    LARGE = auto()

config = Config()

# ============================== 数据模型模块 ==============================
class LogData(TypedDict):
    pending: list[str]
    completed: list[str]
    corrupted: list[str]

@dataclass
class OperationStats:
    scanned: int = 0       # 已扫描文件总数
    processed: int = 0    # 已处理文件数
    large: int = 0
    medium: int = 0
    small: int = 0
    corrupted: int = 0
    speed: float = 0.0
    progress: float = 0.0  # 总体进度百分比

# ============================== 终端控制模块 ==============================
class TerminalManager:
    _instance = None
    _safe_mode = False
    
    def __new__(cls):
        if not cls._instance:
            cls._instance = super().__new__(cls)
            cls._setup_terminal()
        return cls._instance

    @classmethod
    def _setup_terminal(cls):
        cls._safe_mode = False
        
        if os.name == 'nt':
            kernel32 = ctypes.windll.kernel32
            console_mode = ctypes.c_uint32()
            handle = kernel32.GetStdHandle(-11)
            kernel32.GetConsoleMode(handle, ctypes.byref(console_mode))
            kernel32.SetConsoleMode(handle, console_mode.value | 0x0004)
            
            os.environ["PYTHONIOENCODING"] = "utf-8"
            sys.stdout = open(sys.stdout.fileno(), 'w', 
                            encoding='utf-8', 
                            errors='replace',
                            buffering=1)
            sys.stderr = open(sys.stderr.fileno(), 'w',
                            encoding='utf-8',
                            errors='replace',
                            buffering=1)

        try:
            '▓░║═'.encode(sys.stdout.encoding, errors='strict')
        except (UnicodeEncodeError, AttributeError):
            cls._safe_mode = True

    @classmethod
    def safe_mode(cls) -> bool:
        return cls._safe_mode

    @classmethod
    def clear(cls) -> None:
        if not cls._safe_mode:
            sys.stdout.write('\033[2J\033[H')

    @classmethod
    def colored_text(cls, text: str, fg: int = 37, bg: int = 44) -> str:
        return text if cls._safe_mode else f'\033[{fg};{bg}m{text}\033[0m'

# ============================== 界面渲染模块 ==============================
class Dashboard:
    _BORDER_MAP = {
        True: {'horizontal': '=', 'vertical': '|'},
        False: {'horizontal': '═', 'vertical': '│'}
    }
    
    def __init__(self):
        self.terminal = TerminalManager()
        self.start_time = time.time()
        self.last_update = 0.0
        self.last_scanned = 0  # 用于扫描速度计算

    def _safe_print(self, text: str) -> str:
        return text.encode(sys.stdout.encoding, errors='replace').decode(sys.stdout.encoding)

    def _render_header(self) -> None:
        border = self._BORDER_MAP[self.terminal.safe_mode()]
        h_line = border['horizontal'] * 70
        header = self.terminal.colored_text(" SSD冷数据维护系统 v4.3 作者:aspnmy By Python3.12.3 ", bg=44)
        print(self._safe_print(f"\n{h_line}\n{header:^70}\n{h_line}"))

    def _render_stats(self, stats: OperationStats, phase: str) -> None:
        border = self._BORDER_MAP[self.terminal.safe_mode()]
        elapsed = time.time() - self.start_time
        fill, empty = ('#', '-') if self.terminal.safe_mode() else ('▓', '░')
        
        # 计算扫描速度
        scan_speed = (stats.scanned - self.last_scanned) / max(elapsed - self.last_update, 0.001)
        self.last_scanned = stats.scanned
        
        # 构建双重进度信息
        scan_info = f"发现文件: {stats.scanned}" if phase == "扫描中" else ""
        process_bar = fill * int(50 * stats.progress) + empty * (50 - int(50 * stats.progress))
        
        info_lines = [
            f"智能检测固态硬盘的冷数据并解决冷数据掉速问题。",
            f"当前路径: {os.getcwd()}请复制在机械硬盘中运行",
            f"当前时间: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}",
            f"请不要关闭窗口，否则扫描会中断。 ",
            f"请用管理员权限运行，不然有些文件扫描不全 ",
            f"运行阶段: {self.terminal.colored_text(phase.ljust(12), fg=33)} 耗时: {elapsed:.1f}s",
            f"处理进度: [{process_bar}] {stats.progress:.1%}",
            f"{scan_info}",
            f"处理速度: {stats.speed:.1f} MB/s 扫描速度: {scan_speed:.1f} 文件/秒" if stats.speed > 0 else f"扫描速度: {scan_speed:.1f} 文件/秒",
            f"文件分类: 大（大于100MB）({stats.large}) 中（10MB - 100MB）({stats.medium}) 小（小于10MB）({stats.small})",
            f"损坏的文件: {self.terminal.colored_text(str(stats.corrupted), fg=31)}",
            f"本项目地址:https://github.com/aspnmy/ColDataRefresh.git ",
            f"感谢原作者:https://github.com/infrost/ColDataRefresh.git ",
            f"按Ctrl+C退出程序"
            
        ]
        
        # 清理空行并渲染 - 右侧竖线完全不显示
        v = border['vertical']
        for line in filter(None, info_lines):
            print(self._safe_print(f"{v} {line.ljust(68)}"))
        print(self._safe_print(f"{v}{border['horizontal']*68}"))

    def update_display(self, stats: OperationStats, phase: str) -> None:
        if time.time() - self.last_update < config.REPORT_INTERVAL:
            return

        self.terminal.clear()
        self._render_header()
        self._render_stats(stats, phase)
        sys.stdout.flush()
        
        self.last_update = time.time()

# ============================== 文件处理模块 ==============================
class FileOperator:
    @staticmethod
    def categorize_file(size: int) -> FileCategory:
        """文件分类方法
        小文件: < 10MB
        中等文件: 10MB - 100MB  
        大文件: > 100MB
        """
        if size > config.LARGE_FILE:
            return FileCategory.LARGE
        return FileCategory.MEDIUM if size > config.MEDIUM_FILE else FileCategory.SMALL

    @staticmethod
    def checksum_file(path: str) -> int:
        crc = 0
        try:
            with open(path, 'rb') as f:
                while chunk := f.read(config.BUFFER_SIZE):
                    crc = zlib.crc32(chunk, crc)
        except IOError as e:
            raise RuntimeError(f"文件读取失败: {str(e)}")
        return crc

    @classmethod
    def refresh_file(cls, path: str, stats: OperationStats, dashboard: Dashboard) -> None:
        temp_file = f"{path}.tmp"
        error_type = "UNKNOWN"
        
        try:
            start_time = time.time()
            
            # 文件分类处理
            if (size := os.path.getsize(path)) < config.SKIP_SMALL:
                stats.small += 1
                return

            category = cls.categorize_file(size)
            stats.__dict__[category.name.lower()] += 1
            
            # 主体处理逻辑
            src_crc = cls.checksum_file(path)
            for attempt in range(config.MAX_RETRIES + 1):
                try:
                    dest_crc = 0
                    with open(path, 'rb') as src, open(temp_file, 'wb') as dest:
                        while chunk := src.read(config.BUFFER_SIZE):
                            dest.write(chunk)
                            dest_crc = zlib.crc32(chunk, dest_crc)
                            # 实时计算处理速度
                            stats.speed = len(chunk) / (time.time() - start_time + 0.001) / 1024**2
                    
                    if src_crc == dest_crc:
                        os.replace(temp_file, path)
                        return
                    error_type = "CHECKSUM_ERROR"
                except (IOError, OSError) as e:
                    error_type = type(e).__name__
                finally:
                    if os.path.exists(temp_file):
                        os.remove(temp_file)
                
                print(f"尝试重试 ({attempt+1}/{config.MAX_RETRIES})...")

            stats.corrupted += 1
            raise RuntimeError(f"操作失败: {error_type}")
            
        except Exception as e:
            stats.corrupted += 1
            # 直接报错信息
            print(f"❌ 无法读取文件: {path}")
            print(f"   错误类型: {error_type}")
            print(f"   错误信息: {str(e)}")
            # 增强的日志记录
            try:
                log_entry = f"{datetime.now():%Y-%m-%d %H:%M}|{path}|{error_type}|{str(e)}\n"
                with open(config.CORRUPTED_LOG, 'a', encoding='utf-8', errors='replace') as f:
                    f.write(log_entry)
                    f.flush()  # 强制写入磁盘
            except Exception as log_error:
                print(f"⚠️ 日志记录失败: {str(log_error)}")
            dashboard.update_display(stats, "错误处理")

# ============================== 主控流程模块 ==============================
class ApplicationController:
    def __init__(self):
        self.dashboard = Dashboard()
        self.stats = OperationStats()

    def _handle_interrupt(self, _: int, __: FrameType | None) -> None:
        self.dashboard.update_display(self.stats, "用户中止")
        sys.exit(1)

    def _collect_files(self, directory: str, min_days: int) -> list[str]:
        """实时显示扫描进度"""
        cutoff = datetime.now().timestamp() - (min_days * 86400)
        file_list = []
        
        for root, _, files in os.walk(directory):
            for name in files:
                try:
                    path = os.path.join(root, name)
                    if os.path.getmtime(path) < cutoff:
                        file_list.append(path)
                        self.stats.scanned += 1
                        
                        # 实时刷新界面 (每秒最多10次)
                        if time.time() - self.dashboard.last_update > 0.1:
                            self.dashboard.update_display(self.stats, "扫描中")
                except FileNotFoundError:
                    continue  # 忽略临时删除的文件
                except Exception as e:
                    print(f"扫描异常: {os.path.join(root, name) if 'root' in locals() and 'name' in locals() else 'unknown'} - {str(e)}")
        
        return file_list

    def execute(self) -> None:
        signal.signal(signal.SIGINT, self._handle_interrupt)
        
        # 用户配置阶段
        self.dashboard.update_display(self.stats, "初始化")
        directory = input("扫描目录: ").strip('"').replace('：', ':')  # 中文冒号转英文冒号
        # 自动添加反斜杠如果用户没有输入
        if directory and not directory.endswith(('\\', '/')):
            directory += '\\'
        min_days_input = input("数据时效(天): ").replace('：', ':').replace('，', ',')  # 中文标点转英文
        min_days = int(min_days_input) if min_days_input else 0
        skip_small_input = input("跳过小文件? (y/n): ").replace('：', ':').replace('，', ',')  # 中文标点转英文
        skip_small = skip_small_input.lower() == 'y'

        # 文件扫描阶段（实时显示进度）
        self.dashboard.update_display(self.stats, "扫描中")
        target_files = self._collect_files(directory, min_days)
        total_files = len(target_files)
        self.stats.progress = 0.1  # 进入处理阶段初始进度

        # 文件处理阶段
        start_time = time.time()
        for idx, path in enumerate(target_files, 1):
            self.stats.progress = idx / total_files if total_files else 0
            self.stats.processed = idx
            
            if skip_small and os.path.getsize(path) < config.SKIP_SMALL:
                self.stats.small += 1
                continue
                
            try:
                FileOperator.refresh_file(path, self.stats, self.dashboard)
            except Exception as e:
                print(f"\n处理失败: {path}\n原因: {str(e)}")
            finally:
                self.dashboard.update_display(self.stats, "处理中")

        # 结束阶段
        self.dashboard.update_display(self.stats, "完成")
        print(f"\n操作总结: 处理文件 {self.stats.processed} 个 (共发现 {self.stats.scanned} 个)")
        print(f"错误记录: {config.CORRUPTED_LOG}")

# ============================== 基准测试模块 ==============================
class Benchmark:
    """性能基准测试工具"""
    
    @staticmethod
    def create_test_files(directory: str, file_count: int = 10, sizes_mb: list[int] = [1, 10, 100]) -> None:
        """创建测试文件用于基准测试"""
        import random
        
        if not os.path.exists(directory):
            os.makedirs(directory)
        
        print(f"正在创建 {file_count} 个测试文件...")
        
        for i in range(file_count):
            size_mb = random.choice(sizes_mb)
            file_path = os.path.join(directory, f"test_file_{i+1}_{size_mb}MB.dat")
            
            # 创建文件内容（随机数据）
            chunk_size = 1024 * 1024  # 1MB
            with open(file_path, 'wb') as f:
                for _ in range(size_mb):
                    f.write(os.urandom(chunk_size))
            
            # 设置文件修改时间为过去（模拟冷数据）
            old_time = time.time() - (365 * 86400)  # 1年前
            os.utime(file_path, (old_time, old_time))
        
        print(f"测试文件创建完成，目录: {directory}")

    @staticmethod
    def run_benchmark(test_dir: str, iterations: int = 3) -> dict:
        """运行性能基准测试"""
        results = []
        
        for i in range(iterations):
            print(f"\n=== 基准测试第 {i+1}/{iterations} 轮 ===")
            
            # 重置统计
            stats = OperationStats()
            dashboard = Dashboard()
            controller = ApplicationController()
            
            # 运行测试
            start_time = time.time()
            
            # 模拟用户输入
            controller.directory = test_dir
            controller.min_days = 0  # 处理所有文件
            controller.skip_small = False
            
            # 收集文件
            target_files = controller._collect_files(test_dir, 0)
            total_files = len(target_files)
            
            # 处理文件
            for idx, path in enumerate(target_files, 1):
                stats.progress = idx / total_files if total_files else 0
                stats.processed = idx
                
                try:
                    FileOperator.refresh_file(path, stats, dashboard)
                except Exception as e:
                    print(f"处理失败: {path} - {str(e)}")
                
                dashboard.update_display(stats, "基准测试中")
            
            end_time = time.time()
            
            # 记录结果
            result = {
                "iteration": i + 1,
                "total_files": total_files,
                "total_time": end_time - start_time,
                "avg_speed_mb_s": stats.speed,
                "files_processed": stats.processed,
                "corrupted_files": stats.corrupted,
                "file_categories": {
                    "large": stats.large,
                    "medium": stats.medium,
                    "small": stats.small
                }
            }
            results.append(result)
            
            print(f"第 {i+1} 轮完成: {result['total_time']:.2f} 秒, "
                  f"平均速度: {result['avg_speed_mb_s']:.2f} MB/s")
        
        return {
            "benchmark_results": results,
            "summary": {
                "avg_time": sum(r["total_time"] for r in results) / iterations,
                "avg_speed": sum(r["avg_speed_mb_s"] for r in results) / iterations,
                "total_iterations": iterations
            }
        }

    @staticmethod
    def save_results(results: dict, filename: str = "benchmark_results.json") -> None:
        """保存基准测试结果到JSON文件"""
        with open(filename, 'w', encoding='utf-8') as f:
            json.dump(results, f, ensure_ascii=False, indent=2)
        print(f"基准测试结果已保存到: {filename}")


def main():
    # 设置控制台窗口标题
    set_window_title("冷数据维护工具 v4.3 - SSD冷数据刷新与基准测试")
    
    """冷数据维护工具 v4.3 - 主要功能和使用说明
    
    主要功能:
    1. 检测和刷新固态硬盘中的冷数据
    2. 支持数据校验确保文件安全
    3. 实时进度显示和性能监控
    4. 基准测试模式评估性能
    
    文件分类标准:
    - 小文件: < 10MB (可配置跳过)
    - 中等文件: 10MB - 100MB
    - 大文件: > 100MB
    
    使用示例:
    正常模式: python coldatafresh.py
    基准测试: python coldatafresh.py --benchmark --test-dir ./test_data --iterations 3
    创建测试文件: python coldatafresh.py --create-test-files --test-dir ./test_data
    
    注意: 建议以管理员权限运行以确保文件访问权限
    """
    import argparse
    
    parser = argparse.ArgumentParser(
        description='冷数据维护工具 - 检测和刷新固态硬盘冷数据',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
使用示例:
  正常模式: python coldatafresh.py
  基准测试: python coldatafresh.py --benchmark --test-dir ./test_data --iterations 3
  创建测试文件: python coldatafresh.py --create-test-files --test-dir ./test_data

文件分类说明:
  • 小文件: < 10MB (可使用 --skip-small 跳过)
  • 中等文件: 10MB - 100MB  
  • 大文件: > 100MB

注意事项:
  • 建议以管理员权限运行
  • 操作前请确保有数据备份
  • 支持进度保存和恢复功能
        """
    )
    parser.add_argument('--benchmark', action='store_true', help='运行基准测试模式')
    parser.add_argument('--test-dir', type=str, default='./benchmark_test', 
                       help='基准测试文件目录 (默认: ./benchmark_test)')
    parser.add_argument('--iterations', type=int, default=3, 
                       help='基准测试迭代次数 (默认: 3)')
    parser.add_argument('--create-test-files', action='store_true',
                       help='创建测试文件用于基准测试')
    
    args = parser.parse_args()
    
    if args.create_test_files:
        Benchmark.create_test_files(args.test_dir)
        return
    
    if args.benchmark:
        # 确保测试目录存在
        if not os.path.exists(args.test_dir):
            print(f"测试目录不存在: {args.test_dir}")
            print("请先使用 --create-test-files 创建测试文件")
            return
        
        print("开始性能基准测试...")
        results = Benchmark.run_benchmark(args.test_dir, args.iterations)
        Benchmark.save_results(results)
        
        # 打印摘要
        summary = results["summary"]
        print(f"\n=== 基准测试摘要 ===")
        print(f"平均耗时: {summary['avg_time']:.2f} 秒")
        print(f"平均速度: {summary['avg_speed']:.2f} MB/s")
        print(f"测试轮数: {summary['total_iterations']}")
        
    else:
        # 正常模式
        ApplicationController().execute()


if __name__ == "__main__":
    main()
