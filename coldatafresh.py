#!/usr/bin/env python3
# coldatafresh.py - 冷数据维护专业工具
# 终极版：解决日志保存问题，增强进度显示

import os
import sys
import time
import zlib
import ctypes
import signal
import threading
import concurrent.futures
from dataclasses import dataclass
from typing import TypedDict, List, Optional
from datetime import datetime
from types import FrameType
from enum import Enum, auto
import json
import platform

# 尝试导入requests模块，如果不可用则设置标志
try:
    import requests
    HAS_REQUESTS = True
except ImportError:
    HAS_REQUESTS = False

# 读取版本号
VERSION_FILE = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'version.txt')
def get_version():
    """
    从version.txt文件读取当前版本号
    如果文件不存在或读取失败，返回默认版本号
    """
    try:
        with open(VERSION_FILE, 'r', encoding='utf-8') as f:
            return f.read().strip()
    except Exception:
        return '4.3.0'  # 默认版本号

# 获取当前版本
CURRENT_VERSION = get_version()



# 定义TRIM相关的常量和结构
if os.name == 'nt':  # Windows系统
    # Windows API常量
    FSCTL_TRIM_FILES = 0x000900c4
    GENERIC_READ = 0x80000000
    GENERIC_WRITE = 0x40000000
    OPEN_EXISTING = 3
    FILE_SHARE_READ = 0x00000001
    FILE_SHARE_WRITE = 0x00000002
    
    # 定义FILE_ZERO_DATA_INFORMATION_EX结构
    class FILE_ZERO_DATA_INFORMATION_EX(ctypes.Structure):
        _fields_ = [
            ('FileOffset', ctypes.c_ulonglong),
            ('BeyondFinalZero', ctypes.c_ulonglong),
            ('Flags', ctypes.c_ulonglong)
        ]
    
    # 加载Windows API
    kernel32 = ctypes.windll.kernel32
    kernel32.CreateFileW.argtypes = [ctypes.c_wchar_p, ctypes.c_ulonglong, ctypes.c_ulonglong,
                                    ctypes.c_void_p, ctypes.c_ulonglong, ctypes.c_ulonglong, ctypes.c_void_p]
    kernel32.CreateFileW.restype = ctypes.c_void_p
    kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
    kernel32.CloseHandle.restype = ctypes.c_bool
    kernel32.DeviceIoControl.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_void_p,
                                        ctypes.c_ulong, ctypes.c_void_p, ctypes.c_ulong,
                                        ctypes.POINTER(ctypes.c_ulong), ctypes.c_void_p]
    kernel32.DeviceIoControl.restype = ctypes.c_bool
    
elif os.name == 'posix':  # Linux/Unix系统
    # 尝试导入fcntl模块用于ioctl调用
    try:
        import fcntl
        # Linux BLKDISCARD常量
        BLKDISCARD = 0x1277
        BLKDISCARDZEROES = 0x127c
    except ImportError:
        fcntl = None

def set_window_title(title: str = None) -> None:
    """设置控制台窗口标题"""
    if title is None:
        title = f"冷数据维护工具 v{CURRENT_VERSION}"
    if os.name == 'nt':
        ctypes.windll.kernel32.SetConsoleTitleW(title)

# ============================== 系统配置模块 ==============================
@dataclass(frozen=True)
class Config:
    # 获取脚本所在目录
    SCRIPT_DIR: str = os.path.dirname(os.path.abspath(__file__))
    # 日志文件保存在脚本同级目录下
    LOG_FILE: str = os.path.join(SCRIPT_DIR, "refresh_log.json")
    CORRUPTED_LOG: str = os.path.join(SCRIPT_DIR, "corrupted_files.log")
    ERROR_LOG: str = os.path.join(SCRIPT_DIR, "error.log")
    BUFFER_SIZE: int = 4 * 1024
    MAX_RETRIES: int = 3
    LARGE_FILE: int = 100 * 1024**2      # 100MB以上为大文件
    MEDIUM_FILE: int = 10 * 1024**2       # 10MB-100MB为中等文件
    REPORT_INTERVAL: float = 0.2
    SKIP_SMALL: int = 1 * 1024**2        # 1MB以下为小文件（可跳过）
    MAX_WORKERS: int = 4                  # 最大线程数
    MEMORY_LIMIT_MB: int = 512            # 内存限制(MB)
    FULL_REFRESH_MODE: bool = False       # 全盘数据刷新模式标志
    FULL_REFRESH_PATTERN: bytes = b'\xFF'  # 全盘刷新时写入的填充值（FF值）
    TRIM_MODE: bool = False               # TRIM模式标志
    TRIM_BLOCK_SIZE: int = 1 * 1024**2    # TRIM操作的块大小（1MB）

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

# ============================== 日志管理模块 ==============================
class LogManager:
    """日志管理器，负责确保日志目录存在并提供统一的日志记录功能"""
    
    @staticmethod
    def ensure_log_directory():
        """确保日志目录存在，如果不存在则创建"""
        log_dir = os.path.dirname(config.LOG_FILE)
        if log_dir and not os.path.exists(log_dir):
            try:
                os.makedirs(log_dir, exist_ok=True)
                print(f"日志目录已创建: {log_dir}")
            except Exception as e:
                print(f"警告: 无法创建日志目录: {e}")
    
    @staticmethod
    def log_operation(message: str, level: str = "INFO"):
        """记录操作日志到统一的错误日志文件"""
        try:
            LogManager.ensure_log_directory()
            timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S.%f")[:-3]
            log_entry = f"[{timestamp}] [{level}] {message}\n"
            
            with open(config.ERROR_LOG, 'a', encoding='utf-8', errors='replace') as f:
                f.write(log_entry)
                f.flush()
                os.fsync(f.fileno())  # 确保写入到磁盘
        except Exception as e:
            print(f"日志记录失败: {e}")
    
    @staticmethod
    def log_corrupted_file(path: str, error_type: str, error_message: str):
        """记录损坏文件信息"""
        try:
            LogManager.ensure_log_directory()
            log_entry = f"{datetime.now():%Y-%m-%d %H:%M:%S}|{path}|{error_type}|{error_message}\n"
            
            with open(config.CORRUPTED_LOG, 'a', encoding='utf-8', errors='replace') as f:
                f.write(log_entry)
                f.flush()
                os.fsync(f.fileno())  # 确保写入到磁盘
        except Exception as e:
            print(f"损坏文件日志记录失败: {e}")
            LogManager.log_operation(f"无法记录损坏文件: {path}, 错误: {e}", "ERROR")
    
    @staticmethod
    def save_operation_summary(stats: OperationStats, duration: float):
        """保存操作摘要到JSON日志文件"""
        try:
            LogManager.ensure_log_directory()
            
            # 读取现有日志或创建新的
            try:
                if os.path.exists(config.LOG_FILE):
                    with open(config.LOG_FILE, 'r', encoding='utf-8') as f:
                        log_data = json.load(f)
                else:
                    log_data = {
                        "operations": [],
                        "total_scanned": 0,
                        "total_processed": 0,
                        "total_corrupted": 0
                    }
            except Exception as e:
                log_data = {
                    "operations": [],
                    "total_scanned": 0,
                    "total_processed": 0,
                    "total_corrupted": 0
                }
                LogManager.log_operation(f"读取现有日志失败，创建新日志: {e}", "WARNING")
            
            # 添加新操作记录
            operation_record = {
                "timestamp": datetime.now().isoformat(),
                "duration_seconds": round(duration, 2),
                "stats": {
                    "scanned": stats.scanned,
                    "processed": stats.processed,
                    "large": stats.large,
                    "medium": stats.medium,
                    "small": stats.small,
                    "corrupted": stats.corrupted,
                    "final_speed": round(stats.speed, 2)
                }
            }
            
            log_data["operations"].append(operation_record)
            
            # 更新累计统计
            log_data["total_scanned"] += stats.scanned
            log_data["total_processed"] += stats.processed
            log_data["total_corrupted"] += stats.corrupted
            
            # 只保留最近100条操作记录
            if len(log_data["operations"]) > 100:
                log_data["operations"] = log_data["operations"][-100:]
            
            # 写入文件
            with open(config.LOG_FILE, 'w', encoding='utf-8') as f:
                json.dump(log_data, f, ensure_ascii=False, indent=2)
                f.flush()
                os.fsync(f.fileno())  # 确保写入到磁盘
                
            return config.LOG_FILE
        except Exception as e:
            error_msg = f"保存操作摘要失败: {e}"
            print(error_msg)
            LogManager.log_operation(error_msg, "ERROR")
            return None

# 初始化日志管理器，确保日志目录存在
LogManager.ensure_log_directory()

# 从GitHub获取网站信息
def getWebSite():
    """
    从GitHub仓库获取网站信息（QQ群和URL）的更新流程：
    1. 尝试从远程获取并更新本地WebSite.json文件
    2. 从本地WebSite.json文件读取信息
    3. 如果上述步骤失败，返回默认值
    """
    # 定义本地WebSite.json文件路径
    local_website_file = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'WebSite.json')
    remote_url = "https://raw.githubusercontent.com/aspnmy/ColDataRefresh/refs/heads/master/WebSite.json"
    
    # 步骤1：尝试从远程获取并更新本地文件
    if HAS_REQUESTS:
        try:
            response = requests.get(remote_url, timeout=5)
            response.raise_for_status()
            
            # 将远程内容保存到本地文件
            with open(local_website_file, 'w', encoding='utf-8') as f:
                f.write(response.text)
            LogManager.log_operation(f"已更新本地WebSite.json文件", "INFO")
        except Exception as e:
            # 远程获取失败时记录日志但不中断流程
            LogManager.log_operation(f"更新本地WebSite.json失败: {e}", "WARNING")
    else:
        LogManager.log_operation("未安装requests模块，跳过远程更新", "WARNING")
    
    # 步骤2：尝试从本地文件读取信息
    try:
        if os.path.exists(local_website_file):
            with open(local_website_file, 'r', encoding='utf-8') as f:
                data = json.load(f)
                LogManager.log_operation(f"从本地WebSite.json读取信息成功", "INFO")
                return data.get('qqun', '115405294'), data.get('url', 'https://github.com/aspnmy/ColDataRefresh')
    except Exception as e:
        LogManager.log_operation(f"读取本地WebSite.json失败: {e}", "ERROR")
    
    # 步骤3：如果所有尝试都失败，返回默认值
    LogManager.log_operation("无法获取网站信息，使用默认值", "WARNING")
    return "115405294", "https://github.com/aspnmy/ColDataRefresh"

def check_latest_version():
    """
    检查GitHub上的最新版本号
    返回：最新版本号字符串，如果检查失败返回None
    """
    # 检查是否有requests模块
    if not HAS_REQUESTS:
        LogManager.log_operation("未安装requests模块，跳过版本检查", "WARNING")
        return None
        
    try:
        releases_url = "https://api.github.com/repos/aspnmy/ColDataRefresh/releases/latest"
        response = requests.get(releases_url, timeout=5)
        response.raise_for_status()
        
        data = response.json()
        latest_version = data.get('tag_name', '')
        # 清理版本号，移除可能的'v'前缀
        if latest_version.startswith('v'):
            latest_version = latest_version[1:]
        
        LogManager.log_operation(f"获取到最新版本: {latest_version}", "INFO")
        return latest_version
    except Exception as e:
        LogManager.log_operation(f"检查最新版本失败: {e}", "WARNING")
        return None

def compare_versions(current: str, latest: str) -> bool:
    """
    比较版本号，判断是否有新版本
    返回：True表示有新版本，False表示已是最新版本
    """
    try:
        # 分割版本号并转换为整数列表
        current_parts = [int(part) for part in current.split('.')]
        latest_parts = [int(part) for part in latest.split('.')]
        
        # 补齐长度以便比较
        max_len = max(len(current_parts), len(latest_parts))
        current_parts.extend([0] * (max_len - len(current_parts)))
        latest_parts.extend([0] * (max_len - len(latest_parts)))
        
        # 从左到右比较每个部分
        for i in range(max_len):
            if latest_parts[i] > current_parts[i]:
                return True
            elif latest_parts[i] < current_parts[i]:
                return False
        
        return False  # 版本相同
    except Exception as e:
        LogManager.log_operation(f"版本比较失败: {e}", "WARNING")
        return False

# 获取QQ群和URL常量
qqun, url = getWebSite()

# 检查是否有新版本
LATEST_VERSION = check_latest_version()
HAS_NEW_VERSION = LATEST_VERSION and compare_versions(CURRENT_VERSION, LATEST_VERSION)

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
        self.working_directory = ""  # 当前工作路径
        self.full_refresh = False  # 全盘刷新模式标志
        self.trim_mode = False  # TRIM模式标志
        self.min_days = 0  # 数据时效
        self.skip_small = False  # 是否跳过小文件

    def _safe_print(self, text: str) -> str:
        return text.encode(sys.stdout.encoding, errors='replace').decode(sys.stdout.encoding)

    def _render_header(self) -> None:
        border = self._BORDER_MAP[self.terminal.safe_mode()]
        h_line = border['horizontal'] * 70
        header = self.terminal.colored_text(f" SSD掉速激活-冷数据维护系统 v{CURRENT_VERSION} 作者:support@e2bank.cn By Python3.12.3 QQ群：{qqun} Url: {url}", bg=44)
        
        # 打印主标题
        print(self._safe_print(f"\n{h_line}\n{header:^70}"))
        
        # 如果有新版本，显示通知
        if 'HAS_NEW_VERSION' in globals() and HAS_NEW_VERSION and 'LATEST_VERSION' in globals():
            update_notice = self.terminal.colored_text(
                f"⚠️  发现新版本 v{LATEST_VERSION}，请及时更新！", 
                fg=33, bg=41  # 黄色文字，红色背景
            )
            print(self._safe_print(f"{update_notice:^70}"))
        
        print(self._safe_print(f"{h_line}"))

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
        
        # 获取操作模式显示文本
        mode_text = "常规模式" 
        if self.full_refresh:
            mode_text = "全盘刷新模式"
        elif self.trim_mode:
            mode_text = "TRIM模式"
        
        info_lines = [
            f"智能检测固态硬盘的冷数据并解决冷数据掉速问题。",
            f"工作路径: {self.working_directory or os.getcwd()}  ",
            f"操作模式: {self.terminal.colored_text(mode_text, fg=32)}  ",
            f"数据时效: {self.min_days} 天, 跳过小文件: {'是' if self.skip_small else '否'}  ",
            f"当前时间: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}",
            f"运行阶段: {self.terminal.colored_text(phase.ljust(12), fg=33)} 耗时: {elapsed:.1f}s",
            f"处理进度: [{process_bar}] {stats.progress:.1%}",
            f"{scan_info}",
            f"扫描速度: {scan_speed:.1f} MB/s, 处理速度: {stats.speed:.1f} MB/s ",
            f"文件分类: 大（>100MB）({stats.large}) 中（10-100MB）({stats.medium}) 小（<10MB）({stats.small})",
            f"损坏的文件: {self.terminal.colored_text(str(stats.corrupted), fg=31)}",
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
        """计算文件的CRC32校验和"""
        crc = 0
        try:
            with open(path, 'rb') as f:
                while chunk := f.read(config.BUFFER_SIZE):
                    crc = zlib.crc32(chunk, crc)
        except IOError as e:
            raise RuntimeError(f"文件读取失败: {str(e)}")
        return crc
    
    @staticmethod
    def full_refresh_file(path: str, size: int) -> bool:
        """全盘数据刷新模式：将文件内容统一写入FF值
        
        Args:
            path: 文件路径
            size: 文件大小
            
        Returns:
            bool: 操作是否成功
        """
        try:
            temp_file = f"{path}.tmp"
            processed_size = 0
            
            # 创建填充块
            fill_block = config.FULL_REFRESH_PATTERN * config.BUFFER_SIZE
            
            # 写入临时文件
            with open(temp_file, 'wb') as f:
                remaining = size
                while remaining > 0:
                    chunk_size = min(config.BUFFER_SIZE, remaining)
                    f.write(fill_block[:chunk_size])
                    processed_size += chunk_size
                    remaining -= chunk_size
            
            # 替换原文件
            os.replace(temp_file, path)
            return True
        except (IOError, OSError) as e:
            if 'temp_file' in locals() and os.path.exists(temp_file):
                os.remove(temp_file)
            raise RuntimeError(f"全盘刷新失败: {str(e)}")
    
    @staticmethod
    def trim_file(path: str, size: int) -> bool:
        """真正的TRIM功能：通知操作系统哪些数据块是无效的
        
        Args:
            path: 文件路径
            size: 文件大小
            
        Returns:
            bool: 操作是否成功
        """
        try:
            # 定义要TRIM的区域大小
            trim_size = min(config.TRIM_BLOCK_SIZE, size)
            
            if os.name == 'nt':  # Windows实现
                # 打开文件获取句柄
                hFile = kernel32.CreateFileW(
                    path,
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None,
                    OPEN_EXISTING,
                    0,
                    None
                )
                
                if hFile == -1 or hFile is None:
                    raise OSError(f"无法打开文件: {path}")
                
                try:
                    # 设置要TRIM的范围
                    zero_data = FILE_ZERO_DATA_INFORMATION_EX()
                    zero_data.FileOffset = 0  # 从文件开头开始
                    zero_data.BeyondFinalZero = trim_size  # TRIM的结束位置
                    zero_data.Flags = 0  # 使用默认标志
                    
                    # 调用DeviceIoControl执行TRIM操作
                    bytes_returned = ctypes.c_ulong()
                    success = kernel32.DeviceIoControl(
                        hFile,
                        FSCTL_TRIM_FILES,
                        ctypes.byref(zero_data),
                        ctypes.sizeof(zero_data),
                        None,
                        0,
                        ctypes.byref(bytes_returned),
                        None
                    )
                    
                    if not success:
                        raise OSError(f"TRIM操作失败")
                    
                    return True
                finally:
                    # 关闭文件句柄
                    kernel32.CloseHandle(hFile)
                    
            else:  # 其他平台的实现
                # 对于不支持直接TRIM API的平台，我们使用文件覆盖方法
                # 并尽可能使用系统提示机制
                temp_file = f"{path}.tmp"
                
                try:
                    # 复制文件，前部分写入零
                    with open(path, 'rb') as f_orig, open(temp_file, 'wb') as f_temp:
                        # 写入零数据
                        zero_block = b'\x00' * config.BUFFER_SIZE
                        remaining = trim_size
                        while remaining > 0:
                            chunk_size = min(config.BUFFER_SIZE, remaining)
                            f_temp.write(zero_block[:chunk_size])
                            remaining -= chunk_size
                        
                        # 复制剩余内容
                        f_orig.seek(trim_size)
                        while chunk := f_orig.read(config.BUFFER_SIZE):
                            f_temp.write(chunk)
                    
                    os.replace(temp_file, path)
                    
                    # 尝试调用posix_fadvise提示系统该部分数据不再需要（如果可用）
                    try:
                        import posix_fadvise
                        with open(path, 'r') as f:
                            posix_fadvise.fadvise(f.fileno(), 0, trim_size, posix_fadvise.POSIX_FADV_DONTNEED)
                    except (ImportError, AttributeError):
                        pass  # 忽略不可用的情况
                    
                    return True
                except Exception as e:
                    if os.path.exists(temp_file):
                        os.remove(temp_file)
                    raise
            
        except (IOError, OSError) as e:
            if 'temp_file' in locals() and os.path.exists(temp_file):
                os.remove(temp_file)
            raise RuntimeError(f"TRIM操作失败: {str(e)}")

    @classmethod
    def refresh_file(cls, path: str, stats: OperationStats, dashboard: Dashboard, full_refresh: bool = False, trim_mode: bool = False) -> dict:
        """处理单个文件并返回统计信息（线程安全版本）
        
        Args:
            path: 文件路径
            stats: 操作统计对象
            dashboard: 仪表盘对象
            full_refresh: 是否使用全盘数据刷新模式
            
        Returns:
            dict: 处理结果统计
        """
        temp_file = f"{path}.tmp"
        error_type = "UNKNOWN"
        result = {
            'small': 0,
            'large': 0,
            'medium': 0,
            'corrupted': 0,
            'speed': 0.0
        }
        
        try:
            start_time = time.time()
            
            # 文件分类处理
            size = os.path.getsize(path)
            if size < config.SKIP_SMALL:
                result['small'] = 1
                return result

            category = cls.categorize_file(size)
            result[category.name.lower()] = 1
            
            # 根据模式选择处理逻辑
            if full_refresh:
                # 全盘数据刷新模式
                for attempt in range(config.MAX_RETRIES + 1):
                    try:
                        # 使用FF值填充文件
                        cls.full_refresh_file(path, size)
                        
                        # 计算处理速度
                        process_time = time.time() - start_time
                        if process_time > 0:
                            result['speed'] = size / process_time / 1024**2
                        return result
                    except Exception as e:
                        error_type = type(e).__name__
                    
                    print(f"尝试重试 ({attempt+1}/{config.MAX_RETRIES})...")
            elif trim_mode:
                # TRIM模式：通知SSD哪些数据块是无效的，提高写入性能、减少耗损
                # 针对文件前 {config.TRIM_BLOCK_SIZE/1024**2:.1f}MB 区域执行TRIM操作
                # 支持固态硬盘及叠瓦式机械硬盘，可延长设备寿命
                for attempt in range(config.MAX_RETRIES + 1):
                    try:
                        # 执行真正的TRIM操作
                        cls.trim_file(path, size)
                        
                        # 计算处理速度
                        process_time = time.time() - start_time
                        if process_time > 0:
                            trim_size = min(config.TRIM_BLOCK_SIZE, size)
                            result['speed'] = trim_size / process_time / 1024**2
                        return result
                    except Exception as e:
                        error_type = type(e).__name__
                        print(f"TRIM尝试 {attempt+1} 失败: {str(e)}")
                    
                    print(f"尝试重试 ({attempt+1}/{config.MAX_RETRIES})...")
            else:
                # 常规刷新模式（保持原数据）
                src_crc = cls.checksum_file(path)
                for attempt in range(config.MAX_RETRIES + 1):
                    try:
                        dest_crc = 0
                        processed_size = 0
                        with open(path, 'rb') as src, open(temp_file, 'wb') as dest:
                            while chunk := src.read(config.BUFFER_SIZE):
                                dest.write(chunk)
                                dest_crc = zlib.crc32(chunk, dest_crc)
                                processed_size += len(chunk)
                        
                        # 计算整个文件的平均处理速度
                        process_time = time.time() - start_time
                        if process_time > 0:
                            result['speed'] = processed_size / process_time / 1024**2
                        
                        if src_crc == dest_crc:
                            os.replace(temp_file, path)
                            return result
                        error_type = "CHECKSUM_ERROR"
                    except (IOError, OSError) as e:
                        error_type = type(e).__name__
                    finally:
                        if os.path.exists(temp_file):
                            os.remove(temp_file)
                    
                    print(f"尝试重试 ({attempt+1}/{config.MAX_RETRIES})...")

            result['corrupted'] = 1
            raise RuntimeError(f"操作失败: {error_type}")
            
        except Exception as e:
            result['corrupted'] = 1
            # 直接报错信息
            print(f"❌ 无法读取文件: {path}")
            print(f"   错误类型: {error_type}")
            print(f"   错误信息: {str(e)}")
            # 使用日志管理器记录损坏文件
            LogManager.log_corrupted_file(path, error_type, str(e))
            LogManager.log_operation(f"文件处理失败 (超过最大重试次数): {path}, 错误: {str(e)}", "ERROR")
            dashboard.update_display(stats, "错误处理")
        finally:
            return result

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
                        
                        # 在扫描阶段就进行文件分类统计
                        try:
                            size = os.path.getsize(path)
                            if size < config.SKIP_SMALL:
                                self.stats.small += 1
                            else:
                                category = FileOperator.categorize_file(size)
                                self.stats.__dict__[category.name.lower()] += 1
                        except (OSError, FileNotFoundError):
                            pass  # 忽略无法获取大小的文件
                        
                        # 实时刷新界面 (每秒最多10次)
                        if time.time() - self.dashboard.last_update > 0.1:
                            self.dashboard.update_display(self.stats, "扫描中")
                except FileNotFoundError:
                    continue  # 忽略临时删除的文件
                except Exception as e:
                    print(f"扫描异常: {os.path.join(root, name) if 'root' in locals() and 'name' in locals() else 'unknown'} - {str(e)}")
        
        return file_list

    def execute(self, full_refresh: bool = False, trim_mode: bool = False) -> None:
        """执行主程序流程
        
        Args:
            full_refresh: 是否使用全盘数据刷新模式
            trim_mode: 是否使用TRIM模式
        """
        signal.signal(signal.SIGINT, self._handle_interrupt)
        
        # 将模式参数保存到dashboard中
        self.dashboard.full_refresh = full_refresh
        self.dashboard.trim_mode = trim_mode
        
        # 用户配置阶段
        self.dashboard.update_display(self.stats, "初始化")
        LogManager.log_operation("程序进入执行阶段")
        
        # 显示菜单让用户选择操作模式
        if not full_refresh and not trim_mode:  # 只有在非命令行指定的情况下才显示菜单
            print("\n" + "="*50)
            print("          冷数据维护工具 - 操作模式选择")
            print("="*50)
            print("1. 智能模式 (推荐) - 保留原文件内容，仅激活冷数据")
            print("2. 全盘激活冷数据模式 (所有文件全部丢失无法找回) - 将文件内容替换为 66 值")
            print("3. TRIM优化模式 (清理/如需找回数据不要使用这个模式) - 操作系统API来通知SSD哪些数据块是无效的，提高性能并延长寿命")
            print("="*50)
            
            while True:
                choice = input("请选择操作模式 [1/2/3]: ").strip()
                if choice == '1':
                    full_refresh = False
                    trim_mode = False
                    mode_name = "智能模式"
                    break
                elif choice == '2':
                    full_refresh = True
                    trim_mode = False
                    mode_name = "全盘刷新模式"
                    break
                elif choice == '3':
                    full_refresh = False
                    trim_mode = True
                    mode_name = "TRIM模式"
                    break
                else:
                    print("无效的选择，请输入 1、2 或 3")
            
            LogManager.log_operation(f"用户选择操作模式: {mode_name}")
        
        # 根据不同模式显示相应的警告
        if full_refresh:
            print("⚠️  警告: 正在使用全盘数据刷新模式！")
            print(f"   所有文件内容将被替换为 {config.FULL_REFRESH_PATTERN.hex().upper()} 值")
            print("   此操作不可撤销，请确保您了解操作后果！")
            # 要求用户确认
            confirm = input("请输入 'YES' 确认执行全盘刷新操作: ")
            if confirm != 'YES':
                print("操作已取消")
                return
        elif trim_mode:
            print("ℹ️  信息: 正在使用TRIM模式")
            print(f"   通知SSD哪些数据块是无效的，提高写入性能、减少耗损")
            print(f"   针对文件前 {config.TRIM_BLOCK_SIZE/1024**2:.1f}MB 区域执行TRIM操作")
            print(f"   支持固态硬盘及叠瓦式机械硬盘，可延长设备寿命")
        
        directory = input("扫描目录: ").strip('"').replace('：', ':')  # 中文冒号转英文冒号
        # 自动添加反斜杠如果用户没有输入
        if directory and not directory.endswith(('\\', '/')):
            directory += '\\'
        
        min_days_input = input("数据时效(天): ").replace('：', ':').replace('，', ',')  # 中文标点转英文
        min_days = int(min_days_input) if min_days_input else 0
        
        skip_small_input = input("跳过小文件? (y/n): ").replace('：', ':').replace('，', ',')  # 中文标点转英文
        skip_small = skip_small_input.lower() == 'y'
        
        # 将用户输入的参数保存到dashboard中
        self.dashboard.working_directory = directory
        self.dashboard.min_days = min_days
        self.dashboard.skip_small = skip_small
        
        # 记录用户配置
        LogManager.log_operation(f"用户配置: 目录='{directory}', 数据时效={min_days}天, 跳过小文件={skip_small}")

        # 文件扫描阶段（实时显示进度）
        self.dashboard.update_display(self.stats, "扫描中")
        LogManager.log_operation(f"开始扫描目录: {directory}, 最小天数: {min_days}")
        
        try:
            target_files = self._collect_files(directory, min_days)
            total_files = len(target_files)
            self.stats.progress = 0.1  # 进入处理阶段初始进度
            
            LogManager.log_operation(f"扫描完成，发现 {total_files} 个目标文件")
        except Exception as e:
            LogManager.log_operation(f"扫描目录失败: {str(e)}", "ERROR")
            raise

        # 文件处理阶段 - 多线程优化
        start_time = time.time()
        
        # 内存优化：分批处理文件，避免内存溢出
        batch_size = max(1, len(target_files) // (config.MAX_WORKERS * 2))
        processed_count = 0
        
        with concurrent.futures.ThreadPoolExecutor(max_workers=config.MAX_WORKERS) as executor:
            # 分批提交任务
            futures = []
            for path in target_files:
                if skip_small and os.path.getsize(path) < config.SKIP_SMALL:
                    self.stats.small += 1
                    processed_count += 1
                    continue
                
                # 提交任务到线程池
                future = executor.submit(FileOperator.refresh_file, path, self.stats, self.dashboard, full_refresh, trim_mode)
                futures.append(future)
                
                # 内存控制：限制同时运行的任务数量
                if len(futures) >= config.MAX_WORKERS * 2:
                    # 等待部分任务完成
                    for future in concurrent.futures.as_completed(futures[:config.MAX_WORKERS]):
                        try:
                            future.result()
                        except Exception as e:
                            print(f"\n处理失败: {str(e)}")
                        processed_count += 1
                        self.stats.processed = processed_count
                        self.stats.progress = processed_count / total_files if total_files else 0
                        self.dashboard.update_display(self.stats, "处理中")
                    
                    futures = futures[config.MAX_WORKERS:]
            
            # 等待剩余任务完成
            for future in concurrent.futures.as_completed(futures):
                try:
                    future.result()
                except Exception as e:
                    error_msg = str(e)
                    print(f"\n处理失败: {error_msg}")
                    LogManager.log_operation(f"任务执行异常: {error_msg}", "WARNING")
                processed_count += 1
                self.stats.processed = processed_count
                self.stats.progress = processed_count / total_files if total_files else 0
                self.dashboard.update_display(self.stats, "处理中")

        # 结束阶段
        elapsed_time = time.time() - start_time
        self.dashboard.update_display(self.stats, "完成")
        
        # 保存操作摘要到日志
        log_file_path = LogManager.save_operation_summary(self.stats, elapsed_time)
        
        print(f"\n操作总结: 处理文件 {self.stats.processed} 个 (共发现 {self.stats.scanned} 个)")
        print(f"总耗时: {elapsed_time:.2f} 秒")
        print(f"平均速度: {self.stats.speed:.2f} MB/秒")
        print(f"错误记录: {config.CORRUPTED_LOG}")
        if full_refresh:
            print("⚠️  全盘数据刷新模式已完成")
        
        if log_file_path:
            print(f"操作摘要: {log_file_path}")
        
        # 记录操作完成
            if full_refresh:
                mode = "全盘刷新"
            elif trim_mode:
                mode = "TRIM模式"
            else:
                mode = "常规刷新"
            LogManager.log_operation(f"操作完成 ({mode}): 扫描{self.stats.scanned}个文件, 处理{self.stats.processed}个, 损坏{self.stats.corrupted}个, 耗时{elapsed_time:.2f}秒")

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
                    FileOperator.refresh_file(path, stats, dashboard, False)
                except Exception as e:
                    print(f"处理失败: {path} - {str(e)}")
                
                dashboard.update_display(stats, "基准测试中")
            
            end_time = time.time()
            duration = end_time - start_time
            stats.speed = stats.speed if stats.speed > 0 else 0.0  # 确保速度有有效值
            
            # 保存操作摘要到日志文件
            LogManager.save_operation_summary(stats, duration)
            
            # 记录结果
            result = {
                "iteration": i + 1,
                "total_files": total_files,
                "total_time": duration,
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
    set_window_title(f"冷数据维护工具 v{CURRENT_VERSION} - SSD冷数据刷新与基准测试")
    
    # 添加全局错误处理，将错误写入日志文件以便调试
    try:
        f"""冷数据维护工具 v{CURRENT_VERSION} - 主要功能和使用说明
        
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
        """)
        parser.add_argument('--benchmark', action='store_true', help='运行基准测试模式')
        parser.add_argument('--test-dir', type=str, default='./benchmark_test', 
                           help='基准测试文件目录 (默认: ./benchmark_test)')
        parser.add_argument('--iterations', type=int, default=3, 
                           help='基准测试迭代次数 (默认: 3)')
        parser.add_argument('--create-test-files', action='store_true',
                           help='创建测试文件用于基准测试')
        parser.add_argument('--full-refresh', action='store_true',
                           help='使用全盘数据刷新模式（将文件内容统一写入66值）')
        parser.add_argument('--trim-mode', action='store_true',
                           help='使用真正的TRIM功能（通知SSD哪些数据块无效，提高写入性能）')
        
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
            # 确保不同时启用两种模式
            if args.full_refresh and args.trim_mode:
                print("错误: 不能同时启用全盘刷新模式和TRIM模式")
                return
            
            ApplicationController().execute(args.full_refresh, args.trim_mode)
    except Exception as e:
        # 使用日志管理器记录错误
        error_type = type(e).__name__
        error_message = str(e)
        stack_trace = traceback.format_exc()
        
        # 记录详细错误信息
        LogManager.log_operation(f"主程序错误: {error_type} - {error_message}\n{stack_trace}", "ERROR")
        
        # 重新抛出异常，让外部处理
        raise


if __name__ == "__main__":
    # 确保日志管理器已初始化
    LogManager.ensure_log_directory()
    LogManager.log_operation("程序启动")
    
    # 添加全局异常处理，捕获所有未处理的异常并写入日志
    try:
        main()
    except Exception as e:
        # 使用日志管理器记录错误
        error_type = type(e).__name__
        error_message = str(e)
        stack_trace = traceback.format_exc()
        
        # 记录到所有可用的日志文件
        LogManager.log_operation(f"未捕获异常: {error_type} - {error_message}\n{stack_trace}", "ERROR")
        
        # 在控制台显示错误信息
        print(f"\n程序遇到错误，请查看日志文件获取详细信息: {config.ERROR_LOG}")
        print(f"错误类型: {error_type}")
        print(f"错误信息: {error_message}")
        
        # 在Windows下保持窗口打开
        if os.name == 'nt':
            print("\n按Enter键退出...")
            try:
                input()
            except:
                pass
        
        # 重新抛出异常以保持原始行为
        raise
