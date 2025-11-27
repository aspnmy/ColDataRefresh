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
import traceback
import concurrent.futures
from dataclasses import dataclass
from typing import TypedDict, List, Optional
from datetime import datetime
from types import FrameType
from enum import Enum, auto
import json
import platform
import argparse

# 尝试导入requests模块，如果不可用则设置标志
try:
    import requests
    # 禁用InsecureRequestWarning警告
    import urllib3
    urllib3.disable_warnings(urllib3.exceptions.InsecureRequestWarning)
    HAS_REQUESTS = True
except ImportError:
    HAS_REQUESTS = False

def get_disk_space(drive):
    """获取磁盘空间信息
    
    Args:
        drive: 磁盘驱动器路径（如 'H:'）
        
    Returns:
        tuple: (总容量字节, 可用容量字节)，如果出错则返回(0, 0)
    """
    try:
        if platform.system() == 'Windows':
            free_bytes = ctypes.c_ulonglong(0)
            total_bytes = ctypes.c_ulonglong(0)
            
            # 使用Windows API获取磁盘空间
            ctypes.windll.kernel32.GetDiskFreeSpaceExW(
                ctypes.c_wchar_p(drive),
                None,  # 可用字节
                ctypes.pointer(total_bytes),
                ctypes.pointer(free_bytes)
            )
            
            return total_bytes.value, free_bytes.value
        else:
            # Linux/Mac OS 使用statvfs
            stat = os.statvfs(drive)
            total_bytes = stat.f_blocks * stat.f_frsize
            free_bytes = stat.f_bavail * stat.f_frsize
            return total_bytes, free_bytes
    except Exception as e:
        print(f"获取磁盘空间失败: {str(e)}")
        return 0, 0

def format_size(size_bytes):
    """格式化字节大小为可读格式
    
    Args:
        size_bytes: 字节大小
        
    Returns:
        str: 格式化后的大小字符串
    """
    if size_bytes < 1024:
        return f"{size_bytes} B"
    elif size_bytes < 1024**2:
        return f"{size_bytes / 1024:.2f} KB"
    elif size_bytes < 1024**3:
        return f"{size_bytes / (1024**2):.2f} MB"
    elif size_bytes < 1024**4:
        return f"{size_bytes / (1024**3):.2f} GB"
    else:
        return f"{size_bytes / (1024**4):.2f} TB"

def test_write_functionality(target_dir="H:", test_size_gb=50):
    """测试文件写入功能
    
    Args:
        target_dir: 目标测试目录，默认为H盘
        test_size_gb: 测试文件大小（GB），默认为50GB
    """
    print("\n" + "="*60)
    print("         测试文件写入功能")
    print("="*60)
    
    test_files = []
    temp_dir = None
    
    try:
        # 检查目标目录
        if not os.path.exists(target_dir):
            print(f"错误: 目标目录不存在: {target_dir}")
            return
        
        if not os.access(target_dir, os.W_OK):
            print(f"错误: 没有写入权限: {target_dir}")
            return
        
        # 创建临时测试目录 $aspnmytools
        temp_dir = os.path.join(target_dir, "$aspnmytools")
        if not os.path.exists(temp_dir):
            try:
                os.makedirs(temp_dir)
                print(f"创建临时测试目录: {temp_dir}")
            except Exception as e:
                print(f"创建临时目录失败: {str(e)}")
                return
        
        # 获取磁盘容量信息
        total_bytes, free_bytes = get_disk_space(target_dir)
        if total_bytes > 0:
            print(f"磁盘信息:")
            print(f"  总容量: {format_size(total_bytes)}")
            print(f"  可用容量: {format_size(free_bytes)}")
            print(f"  已用容量: {format_size(total_bytes - free_bytes)}")
            
            # 检查是否有足够空间
            required_bytes = test_size_gb * 1024**3
            if free_bytes < required_bytes:
                print(f"\n警告: 可用空间不足! 需要 {test_size_gb} GB, 但只有 {free_bytes / (1024**3):.2f} GB 可用")
                # 调整为可用空间的80%
                test_size_gb = int(free_bytes * 0.8 / (1024**3))
                print(f"自动调整写入大小为: {test_size_gb} GB")
        
        # 创建测试文件路径（使用临时目录）
        test_file = os.path.join(temp_dir, "test_write_functionality.tmp")
        test_files.append(test_file)
        
        target_size = test_size_gb * 1024**3  # 转换为字节
        
        print(f"\n测试设置:")
        print(f"  测试目录: {target_dir}")
        print(f"  测试文件: {test_file}")
        print(f"  目标写入大小: {test_size_gb} GB")
        print(f"  目标字节数: {target_size} bytes")
        print(f"\n开始测试写入...")
        
        # 调用连续写入函数
        stats = FileOperator.continuous_full_refresh_file(test_file, target_size)
        
        print("\n" + "="*60)
        print("测试完成！")
        print(f"实际写入容量: {stats['total_written'] / (1024**3):.2f} GB")
        print(f"最高写入速度: {stats['max_speed_mbs']:.2f} MB/s")
        print("="*60)
        
        # 删除测试文件
        print(f"\n正在清理测试文件...")
        for file_path in test_files:
            if os.path.exists(file_path):
                try:
                    os.remove(file_path)
                    print(f"  删除文件: {file_path}")
                except Exception as e:
                    print(f"  删除文件失败 {file_path}: {str(e)}")
        
        # 清理临时测试目录
        if temp_dir and os.path.exists(temp_dir):
            try:
                os.rmdir(temp_dir)
                print(f"  删除临时测试目录: {temp_dir}")
            except Exception as e:
                print(f"  清理临时目录失败: {temp_dir} - {str(e)}")
        print("清理完成！")
        
    except Exception as e:
        print(f"测试失败: {str(e)}")
        print(traceback.format_exc())
        
        # 发生异常时也尝试清理文件
        if test_files:
            print(f"\n尝试清理已创建的测试文件...")
            for file_path in test_files:
                if os.path.exists(file_path):
                    try:
                        os.remove(file_path)
                        print(f"  删除文件: {file_path}")
                    except Exception:
                        pass

# 读取版本号
def get_version():
    """
    从version.txt文件读取当前版本号
    支持PyInstaller打包后的环境
    如果文件不存在或读取失败，返回默认版本号
    """
    try:
        # 处理PyInstaller打包后的情况
        if hasattr(sys, '_MEIPASS'):
            # 在打包后的环境中，文件位于_MEIPASS目录下
            VERSION_FILE = os.path.join(sys._MEIPASS, 'version.txt')
        else:
            # 在正常Python环境中
            VERSION_FILE = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'version.txt')
        
        with open(VERSION_FILE, 'r', encoding='utf-8') as f:
            return f.read().strip()
    except Exception:
        return '4.5.0'  # 更新默认版本号以匹配make.bat中的DEFAULT_VERSION

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
    
    def is_windows_10_or_higher() -> bool:
        """
        检测当前Windows系统是否为Windows 10或更高版本
        
        Returns:
            bool: 如果是Windows 10或更高版本返回True，否则返回False
        """
        try:
            # Windows 10的主要版本号是10.0
            major, minor = sys.getwindowsversion().major, sys.getwindowsversion().minor
            return (major, minor) >= (10, 0)
        except Exception:
            return False
    
    # 检测当前Windows版本
    IS_WINDOWS_10_OR_HIGHER = is_windows_10_or_higher()


# 定义Windows 10+系统上使用PowerShell的Optimize-Volume命令的工具函数
def optimize_volume_retrim(drive: str) -> bool:
    """
    使用PowerShell的Optimize-Volume命令执行ReTrim操作
    
    Args:
        drive: 驱动器路径（如 "C:"）
        
    Returns:
        bool: 操作是否成功
    """
    try:
        import subprocess
        
        # 构建PowerShell命令
        powershell_command = f"Optimize-Volume -DriveLetter {drive[0]} -ReTrim -Verbose"
        LogManager.log_operation(f"执行PowerShell命令: {powershell_command}")
        
        # 执行PowerShell命令
        result = subprocess.run(
            ['powershell', '-Command', powershell_command],
            capture_output=True,
            text=True,
            timeout=300  # 设置5分钟超时
        )
        
        if result.returncode == 0:
            LogManager.log_operation(f"ReTrim操作成功: {result.stdout}")
            return True
        else:
            LogManager.log_operation(f"ReTrim操作失败: {result.stderr}", "WARNING")
            return False
    except Exception as e:
        LogManager.log_operation(f"执行ReTrim操作时出错: {str(e)}", "ERROR")
        return False

def optimize_volume_slab_consolidate(drive: str) -> bool:
    """
    使用PowerShell的Optimize-Volume命令执行SlabConsolidate操作
    
    Args:
        drive: 驱动器路径（如 "C:"）
        
    Returns:
        bool: 操作是否成功
    """
    try:
        import subprocess
        
        # 构建PowerShell命令
        powershell_command = f"Optimize-Volume -DriveLetter {drive[0]} -SlabConsolidate -Verbose"
        LogManager.log_operation(f"执行PowerShell命令: {powershell_command}")
        
        # 执行PowerShell命令
        result = subprocess.run(
            ['powershell', '-Command', powershell_command],
            capture_output=True,
            text=True,
            timeout=300  # 设置5分钟超时
        )
        
        if result.returncode == 0:
            LogManager.log_operation(f"SlabConsolidate操作成功: {result.stdout}")
            return True
        else:
            LogManager.log_operation(f"SlabConsolidate操作失败: {result.stderr}", "WARNING")
            return False
    except Exception as e:
        LogManager.log_operation(f"执行SlabConsolidate操作时出错: {str(e)}", "ERROR")
        return False

def optimize_volume_retrim_again(drive: str) -> bool:
    """
    使用PowerShell的Optimize-Volume命令再次执行ReTrim操作
    作为完整优化流程的最后一步
    
    Args:
        drive: 驱动器路径（如 "C:"）
        
    Returns:
        bool: 操作是否成功
    """
    try:
        import subprocess
        
        # 构建PowerShell命令
        powershell_command = f"Optimize-Volume -DriveLetter {drive[0]} -ReTrim -Verbose"
        LogManager.log_operation(f"执行最终ReTrim操作PowerShell命令: {powershell_command}")
        
        # 执行PowerShell命令
        result = subprocess.run(
            ['powershell', '-Command', powershell_command],
            capture_output=True,
            text=True,
            timeout=300  # 设置5分钟超时
        )
        
        if result.returncode == 0:
            LogManager.log_operation(f"最终ReTrim操作成功: {result.stdout}")
            return True
        else:
            LogManager.log_operation(f"最终ReTrim操作失败: {result.stderr}", "WARNING")
            return False
    except Exception as e:
        LogManager.log_operation(f"执行最终ReTrim操作时出错: {str(e)}", "ERROR")
        return False


if os.name == 'posix':  # Linux/Unix系统
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

# 确定程序运行目录
# 对于PyInstaller打包后的程序，获取可执行文件所在目录
# 对于正常Python环境，获取脚本所在目录
def get_program_dir():
    """
    获取程序的实际运行目录
    
    Returns:
        str: 程序运行目录
    """
    if hasattr(sys, '_MEIPASS'):
        # PyInstaller打包后的环境
        # 获取可执行文件所在目录
        if sys.platform == 'win32':
            return os.path.dirname(os.path.abspath(sys.executable))
        else:
            return os.path.dirname(os.path.abspath(sys.executable))
    else:
        # 正常Python环境
        return os.path.dirname(os.path.abspath(__file__))

@dataclass(frozen=True)
class Config:
    # 获取程序运行目录
    SCRIPT_DIR: str = get_program_dir()
    # 日志文件保存在程序运行目录下
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
        """记录损坏文件信息
        
        Args:
            path: 文件路径
            error_type: 错误类型
            error_message: 错误信息
        """
        try:
            LogManager.ensure_log_directory()
            log_entry = f"{datetime.now():%Y-%m-%d %H:%M:%S}|{path}|{error_type}|{error_message}\n"
            
            # 确保日志文件写入到脚本同级目录
            log_path = os.path.join(config.SCRIPT_DIR, "corrupted_files.log")
            
            with open(log_path, 'a', encoding='utf-8', errors='replace') as f:
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

# 全局变量：已处理的驱动器列表，用于避免重复执行TRIM操作
processed_drives = set()

# 定义Windows版本检查函数
def get_windows_version() -> tuple:
    """
    获取Windows系统版本号
    
    Returns:
        tuple: (major, minor, build) - 系统版本号
    """
    try:
        import sys
        if sys.platform != 'win32':
            return (0, 0, 0)
        
        # 获取Windows版本信息
        version_info = sys.getwindowsversion()
        return (version_info.major, version_info.minor, version_info.build)
    except Exception as e:
        LogManager.log_operation(f"获取Windows版本失败: {str(e)}", "WARNING")
        return (0, 0, 0)

# 检查是否为Windows 10及以上版本
def is_windows_10_or_higher() -> bool:
    """
    检测当前Windows系统是否为Windows 10或更高版本
    
    Returns:
        bool: 如果是Windows 10或更高版本返回True，否则返回False
    """
    try:
        major, minor, build = get_windows_version()
        # Windows 10的主要版本号是10.0
        return (major, minor) >= (10, 0)
    except Exception as e:
        LogManager.log_operation(f"检测Windows版本失败: {str(e)}", "WARNING")
        return False

# 检查是否为Windows 11及以上版本
def is_windows_11_or_higher() -> bool:
    """
    检测当前Windows系统是否为Windows 11或更高版本
    
    Returns:
        bool: 如果是Windows 11或更高版本返回True，否则返回False
    """
    try:
        major, minor, build = get_windows_version()
        # Windows 11的主要版本号是10.0，内部版本号是22000或更高
        return (major, minor) >= (10, 0) and build >= 22000
    except Exception as e:
        LogManager.log_operation(f"检测Windows版本失败: {str(e)}", "WARNING")
        return False

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
            # 禁用SSL证书验证以解决证书验证失败问题
            response = requests.get(remote_url, timeout=5, verify=False)
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
        # 禁用SSL证书验证以解决证书验证失败问题
        response = requests.get(releases_url, timeout=5, verify=False)
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
        header = self.terminal.colored_text(f" SSD掉速激活-冷数据维护系统 v{CURRENT_VERSION} 作者:support@e2bank.cn By Python3.12.3 QQ群：{qqun} Url: https://{url}", bg=44)
        
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
            f"GitHub:https://github.com/aspnmy/ColDataRefresh.git",
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

# ============================== 全盘刷新功能模块 ==============================
class FullRefreshManager:
    """
    全盘刷新功能管理器，负责实现完整的全盘刷新业务流程
    """
    
    @staticmethod
    def is_drive(directory: str) -> bool:
        """
        判断指定路径是否为整个盘符
        
        Args:
            directory: 指定路径
            
        Returns:
            bool: 是否为整个盘符
        """
        try:
            if os.name == 'nt':
                # Windows系统：判断是否为盘符格式（如 C: 或 C:\）
                import re
                return bool(re.match(r'^[a-zA-Z]:[\\/]?$', directory))
            else:
                # Linux系统：判断是否为根目录或挂载点
                return directory == '/' or os.path.ismount(directory)
        except Exception as e:
            print(f"判断路径类型失败: {str(e)}")
            return False
    
    @staticmethod
    def get_directory_stats(directory: str) -> tuple:
        """
        获取指定目录的总容量和已使用容量
        
        Args:
            directory: 指定目录路径
            
        Returns:
            tuple: (总容量字节, 已使用容量字节, 可用容量字节)
        """
        try:
            total_bytes, free_bytes = get_disk_space(directory)
            used_bytes = total_bytes - free_bytes
            return total_bytes, used_bytes, free_bytes
        except Exception as e:
            print(f"获取目录统计信息失败: {str(e)}")
            return 0, 0, 0
    
    @staticmethod
    def format_drive(drive: str) -> bool:
        """
        格式化指定盘符
        
        Args:
            drive: 盘符路径（如 "H:"）
            
        Returns:
            bool: 格式化是否成功
        """
        try:
            if os.name != 'nt':
                print("格式化功能仅支持Windows系统")
                return False
            
            import subprocess
            
            # 使用diskpart命令格式化盘符
            diskpart_script = f"select volume {drive[0]}\nformat fs=ntfs quick"
            
            # 创建临时diskpart脚本文件
            import tempfile
            with tempfile.NamedTemporaryFile(mode='w', suffix='.txt', delete=False) as f:
                f.write(diskpart_script)
                temp_script = f.name
            
            try:
                # 执行diskpart命令
                result = subprocess.run(
                    ['diskpart', '/s', temp_script],
                    capture_output=True,
                    text=True,
                    shell=True
                )
                
                # 检查执行结果
                if result.returncode == 0:
                    print(f"盘符 {drive} 格式化成功")
                    return True
                else:
                    print(f"盘符 {drive} 格式化失败: {result.stderr}")
                    return False
            finally:
                # 删除临时脚本文件
                os.unlink(temp_script)
        except Exception as e:
            print(f"格式化盘符失败: {str(e)}")
            return False
    
    @staticmethod
    def backup_files(source_dir: str, backup_dir: str) -> bool:
        """
        备份文件到临时目录
        
        Args:
            source_dir: 源目录路径
            backup_dir: 备份目录路径
            
        Returns:
            bool: 备份是否成功
        """
        try:
            import shutil
            
            # 创建备份目录
            if not os.path.exists(backup_dir):
                os.makedirs(backup_dir)
            
            # 遍历源目录，备份所有文件
            for root, dirs, files in os.walk(source_dir):
                # 跳过系统保护目录
                if "$RECYCLE.BIN" in root.upper() or "SYSTEM VOLUME INFORMATION" in root.upper():
                    continue
                
                # 创建相对路径
                rel_path = os.path.relpath(root, source_dir)
                if rel_path == '.':
                    rel_path = ''
                
                # 创建目标目录
                target_dir = os.path.join(backup_dir, rel_path)
                if not os.path.exists(target_dir):
                    os.makedirs(target_dir)
                
                # 复制文件
                for file in files:
                    src_file = os.path.join(root, file)
                    dst_file = os.path.join(target_dir, file)
                    shutil.copy2(src_file, dst_file)
            
            return True
        except Exception as e:
            print(f"备份文件失败: {str(e)}")
            return False
    
    @staticmethod
    def restore_files(backup_dir: str, target_dir: str) -> bool:
        """
        从临时目录恢复文件
        
        Args:
            backup_dir: 备份目录路径
            target_dir: 目标目录路径
            
        Returns:
            bool: 恢复是否成功
        """
        try:
            import shutil
            
            # 遍历备份目录，恢复所有文件
            for root, dirs, files in os.walk(backup_dir):
                # 创建相对路径
                rel_path = os.path.relpath(root, backup_dir)
                if rel_path == '.':
                    rel_path = ''
                
                # 创建目标目录
                target_subdir = os.path.join(target_dir, rel_path)
                if not os.path.exists(target_subdir):
                    os.makedirs(target_subdir)
                
                # 复制文件
                for file in files:
                    src_file = os.path.join(root, file)
                    dst_file = os.path.join(target_subdir, file)
                    shutil.copy2(src_file, dst_file)
            
            return True
        except Exception as e:
            print(f"恢复文件失败: {str(e)}")
            return False
    
    @staticmethod
    def calculate_max_files(total_capacity: int, unit_size: int) -> int:
        """
        计算需要创建的文件数量
        
        Args:
            total_capacity: 总容量（字节）
            unit_size: 单个文件大小（字节）
            
        Returns:
            int: 文件数量
        """
        if unit_size <= 0:
            return 0
        return total_capacity // unit_size
    
    @staticmethod
    def fill_available_space(directory: str, unit_size: int) -> tuple:
        """
        填满可用空间
        
        Args:
            directory: 指定目录路径
            unit_size: 单个文件大小（字节）
            
        Returns:
            tuple: (累积填入容量字节, 最大写入速度 MB/s)
        """
        try:
            # 创建工作目录
            work_dir = os.path.join(directory, "$aspnmytools")
            if not os.path.exists(work_dir):
                os.makedirs(work_dir)
            
            cumulative_capacity = 0
            max_write_speed = 0.0
            file_count = 0
            
            # 计算需要创建的文件数量
            total_bytes, _, free_bytes = FullRefreshManager.get_directory_stats(directory)
            max_files = FullRefreshManager.calculate_max_files(total_bytes, unit_size)
            print(f"预计需要创建 {max_files} 个文件")
            
            # 持续写入文件，直到填满可用空间
            while True:
                # 检查可用空间
                _, _, free_bytes = FullRefreshManager.get_directory_stats(directory)
                if free_bytes < unit_size:
                    break
                
                # 创建文件路径
                file_path = os.path.join(work_dir, f"refresh_{file_count}.dat")
                
                # 写入文件
                write_stats = FileOperator.continuous_full_refresh_file(file_path, unit_size)
                
                # 更新统计信息
                cumulative_capacity += write_stats["total_written"]
                if write_stats["max_speed_mbs"] > max_write_speed:
                    max_write_speed = write_stats["max_speed_mbs"]
                
                file_count += 1
                
                # 显示进度
                progress = file_count / max_files if max_files > 0 else 0
                print(f"已创建 {file_count} 个文件，进度: {progress:.1%}")
            
            return cumulative_capacity, max_write_speed
        except Exception as e:
            print(f"填满可用空间失败: {str(e)}")
            return 0, 0.0
    
    @staticmethod
    def cleanup(work_dir: str, backup_dir: str = None, keep_backup: bool = False) -> bool:
        """
        清理临时文件和目录
        
        Args:
            work_dir: 工作目录路径
            backup_dir: 备份目录路径
            keep_backup: 是否保留备份
            
        Returns:
            bool: 清理是否成功
        """
        try:
            import shutil
            
            # 清理工作目录
            if os.path.exists(work_dir):
                shutil.rmtree(work_dir)
            
            # 清理备份目录（如果不保留）
            if backup_dir and not keep_backup and os.path.exists(backup_dir):
                shutil.rmtree(backup_dir)
            
            return True
        except Exception as e:
            print(f"清理临时文件失败: {str(e)}")
            return False

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
            # 检查是否为系统保护目录
            dir_path = os.path.dirname(path)
            if "$RECYCLE.BIN" in dir_path.upper() or "SYSTEM VOLUME INFORMATION" in dir_path.upper():
                print(f"警告: 跳过系统保护目录中的文件: {path}")
                return False
            
            temp_file = f"{path}.tmp"
            processed_size = 0
            
            # 创建填充块
            fill_block = config.FULL_REFRESH_PATTERN * config.BUFFER_SIZE
            
            # 使用用户选择的写入容量，但不超过原始文件大小
            if hasattr(FileOperator, 'target_size'):
                target_size = min(size, FileOperator.target_size)
            else:
                target_size = size
            
            # 写入临时文件
            with open(temp_file, 'wb') as f:
                remaining = target_size
                while remaining > 0:
                    chunk_size = min(config.BUFFER_SIZE, remaining)
                    f.write(fill_block[:chunk_size])
                    processed_size += chunk_size
                    remaining -= chunk_size
                
                # 确保所有数据都写入磁盘
                f.flush()
                os.fsync(f.fileno())
            
            # 验证写入的数据量
            if processed_size != target_size:
                raise RuntimeError(f"写入数据量不匹配: 预期 {target_size} 字节，实际写入 {processed_size} 字节")
            
            # 使用原子操作替换文件
            os.replace(temp_file, path)
            
            # 再次打开文件确认内容已更改
            with open(path, 'rb') as f:
                # 验证文件大小
                actual_size = os.path.getsize(path)
                if actual_size != target_size:
                    raise RuntimeError(f"文件大小不匹配: 预期 {target_size} 字节，实际 {actual_size} 字节")
                
                # 读取并验证前1024字节
                first_chunk = f.read(1024)
                if not first_chunk or any(b != config.FULL_REFRESH_PATTERN[0] for b in first_chunk):
                    raise RuntimeError("文件内容验证失败，数据可能未被正确替换")
                
                # 验证文件末尾
                f.seek(-min(1024, actual_size), os.SEEK_END)
                last_chunk = f.read()
                if any(b != config.FULL_REFRESH_PATTERN[0] for b in last_chunk):
                    raise RuntimeError("文件末尾内容验证失败，数据可能未被正确替换")
            
            return True
        except Exception as e:
            # 确保临时文件被清理
            if 'temp_file' in locals() and os.path.exists(temp_file):
                try:
                    os.remove(temp_file)
                except:
                    pass
            raise RuntimeError(f"全盘刷新失败: {str(e)}")
            
    @staticmethod
    def continuous_full_refresh_file(path: str, target_unit_size: int) -> dict:
        """持续全盘写入模式：直接写入目标文件，不再使用临时文件
        
        Args:
            path: 文件路径
            target_unit_size: 用户指定的写入容量（字节）
            
        Returns:
            dict: 包含总写入容量和最高写入速度的统计信息
        """
        stats = {
            "total_written": 0,
            "max_speed_mbs": 0
        }
        
        # 检查是否为系统保护目录
        dir_path = os.path.dirname(path)
        if "$RECYCLE.BIN" in dir_path.upper() or "SYSTEM VOLUME INFORMATION" in dir_path.upper():
            print(f"警告: 跳过系统保护目录中的文件: {path}")
            return stats
        
        # 保存原始的SIGINT处理函数
        original_sigint_handler = signal.getsignal(signal.SIGINT)
        
        # 定义文件清理函数
        def cleanup_file(signum=None, frame=None):
            if os.path.exists(path):
                try:
                    print(f"\n正在清理文件: {path}")
                    os.remove(path)
                except Exception:
                    pass
            if signum is not None:
                # 如果是信号触发的清理，恢复原始处理函数并重新触发
                signal.signal(signal.SIGINT, original_sigint_handler)
                os.kill(os.getpid(), signum)
        
        # 设置Ctrl+C信号处理
        signal.signal(signal.SIGINT, cleanup_file)
        
        try:
            print(f"\n开始写入文件: {path}")
            # 创建填充块
            fill_block = config.FULL_REFRESH_PATTERN * config.BUFFER_SIZE
            
            # 检查目录是否存在
            if not os.path.exists(dir_path):
                print(f"错误: 目录 {dir_path} 不存在")
                raise RuntimeError(f"目标目录不存在: {dir_path}")
            
            # 检查是否有写入权限
            if not os.access(dir_path, os.W_OK):
                print(f"错误: 没有写入权限: {dir_path}")
                raise RuntimeError(f"无写入权限: {dir_path}")
            
            # 清理可能存在的旧文件
            if os.path.exists(path):
                print(f"删除已存在的文件: {path}")
                os.remove(path)
            
            # 准备写入数据
            print(f"开始直接写入目标文件...")
            with open(path, 'wb') as f:
                start_time = time.time()
                prev_time = start_time
                prev_written = 0
                
                # 写入一个单位的容量
                unit_start_time = time.time()
                unit_written = 0
                remaining = target_unit_size
                
                while remaining > 0:
                    try:
                        chunk_size = min(config.BUFFER_SIZE, remaining)
                        f.write(fill_block[:chunk_size])
                        unit_written += chunk_size
                        remaining -= chunk_size
                        
                        # 每1秒显示一次写入速度
                        current_time = time.time()
                        if current_time - prev_time >= 1:
                            elapsed = current_time - unit_start_time
                            if elapsed > 0:
                                speed_mbs = (unit_written / (1024 * 1024)) / elapsed
                                print(f"\r当前写入速度: {speed_mbs:.2f} MB/s", end="", flush=True)
                            prev_time = current_time
                            
                    except (IOError, OSError) as e:
                        # 捕获磁盘空间不足或其他写入错误
                        if "No space left on device" in str(e) or "磁盘空间不足" in str(e):
                            print("\n磁盘空间已写满，停止写入")
                            # 确保已写入的数据刷新到磁盘
                            f.flush()
                            os.fsync(f.fileno())
                            # 更新统计信息
                            stats["total_written"] = unit_written
                            return stats
                        else:
                            # 其他错误重新抛出
                            raise
                
                # 确保数据写入磁盘
                f.flush()
                os.fsync(f.fileno())
                
                # 计算最终写入速度
                unit_time = time.time() - unit_start_time
                if unit_time > 0:
                    unit_speed_mbs = (unit_written / (1024 * 1024)) / unit_time
                    stats["max_speed_mbs"] = unit_speed_mbs
                    print(f"\r当前写入速度: {unit_speed_mbs:.2f} MB/s")
                
                # 更新统计信息
                stats["total_written"] = unit_written
                
            # 验证文件大小
            if os.path.exists(path):
                actual_size = os.path.getsize(path)
                print(f"文件写入完成，实际大小: {format_size(actual_size)}")
                stats["total_written"] = actual_size
                
        except Exception as e:
            # 出现异常时清理文件
            if os.path.exists(path):
                try:
                    print(f"\n错误: {str(e)}")
                    print(f"清理未完成的文件: {path}")
                    os.remove(path)
                except Exception:
                    pass
            raise
        finally:
            # 恢复原始的SIGINT处理函数
            signal.signal(signal.SIGINT, original_sigint_handler)
            
        return stats
    
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
                # 获取文件所在的驱动器
                drive = os.path.splitdrive(path)[0]
                if not drive:  # 如果没有驱动器信息，使用当前目录的驱动器
                    drive = os.path.splitdrive(os.getcwd())[0]
                
                if drive:
                    # 检查是否已经处理过该驱动器，避免重复执行TRIM操作
                    global processed_drives
                    if drive in processed_drives:
                        LogManager.log_operation(f"驱动器{drive}已经执行过TRIM操作，跳过本次操作")
                        return True
                    
                    # 添加到已处理驱动器列表
                    processed_drives.add(drive)
                    
                    # 添加TRIM操作提示信息
                    print(f"\n" + "="*60)
                    print(f"正在对驱动器 {drive} 进行 SSD固态盘实时TRIM优化操作")
                    print("="*60)
                    print("注意事项：")
                    print("1. 后台执行时间预计需要10-30分钟")
                    print("2. 30分钟内请不要对该SSD固态硬盘进行断电操作")
                    print("3. TRIM操作有助于提高SSD性能并延长使用寿命")
                    print("4. 操作期间可以继续使用计算机，但建议减少对该驱动器的大量读写")
                    print("="*60)
                    
                    # 获取Windows版本信息
                    is_win11 = is_windows_11_or_higher()
                    is_win10 = is_windows_10_or_higher()
                    
                    LogManager.log_operation(f"在{platform.system()}系统上对{drive}执行TRIM操作，Windows 11: {is_win11}, Windows 10: {is_win10}")
                    
                    # 标记是否执行了PowerShell操作
                    powershell_executed = False
                    
                    try:
                        import subprocess
                        
                        if is_win11 or is_win10:
                            # Windows 11或10: 使用PowerShell执行TRIM操作
                            powershell_executed = True
                            
                            if is_win11:
                                # Windows 11: 使用最佳TRIM策略
                                LogManager.log_operation(f"Windows 11系统，执行最佳TRIM策略")
                                
                                # 执行ReTrim + SlabConsolidate + ReTrim组合操作
                                # 首先执行ReTrim操作
                                if not optimize_volume_retrim(drive):
                                    LogManager.log_operation("ReTrim操作失败，尝试回退到原方法", "WARNING")
                                    return False
                                
                                # 然后执行SlabConsolidate操作
                                if not optimize_volume_slab_consolidate(drive):
                                    LogManager.log_operation("SlabConsolidate操作失败，但继续执行", "WARNING")
                                
                                # 最后再次执行ReTrim操作
                                if not optimize_volume_retrim_again(drive):
                                    LogManager.log_operation("第二次ReTrim操作失败，但已完成主要优化", "WARNING")
                            else:
                                # Windows 10: 只执行ReTrim操作
                                LogManager.log_operation(f"Windows 10系统，执行ReTrim操作")
                                
                                # 只执行ReTrim操作
                                if not optimize_volume_retrim(drive):
                                    LogManager.log_operation("ReTrim操作失败，尝试回退到原方法", "WARNING")
                                    return False
                            
                            LogManager.log_operation(f"对驱动器{drive}的TRIM操作完成")
                            return True
                    except Exception as e:
                        LogManager.log_operation(f"执行TRIM操作时出错: {str(e)}", "WARNING")
                        # 如果PowerShell命令失败，继续执行下面的DeviceIoControl方法
                    
                    # Windows 10以下版本或PowerShell操作失败，继续执行DeviceIoControl方法
                
                # Windows 10以下版本或defrag.exe失败时，使用原来的DeviceIoControl方法
                LogManager.log_operation(f"使用DeviceIoControl对文件{path}执行TRIM操作")
                
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
                # 全盘数据刷新模式 - 使用持续写入功能
                for attempt in range(config.MAX_RETRIES + 1):
                    try:
                        # 检查是否设置了target_size（用户指定的写入单位）
                        if hasattr(FileOperator, 'target_size'):
                            # 使用持续写入模式
                            print(f"\n开始持续写入模式，写入单位: {FileOperator.target_size / (1024**3):.1f}GB")
                            print(f"目标文件: {path}")
                            print("按Ctrl+C可随时停止写入")
                            
                            # 执行持续写入
                            write_stats = cls.continuous_full_refresh_file(path, FileOperator.target_size)
                            
                            # 显示统计信息
                            total_gb = write_stats["total_written"] / (1024**3)
                            max_speed = write_stats["max_speed_mbs"]
                            print(f"\n\n写入完成！")
                            print(f"实际总写入容量: {total_gb:.2f} GB")
                            print(f"最高写入速度: {max_speed:.2f} MB/s")
                            
                            # 更新结果统计
                            result['speed'] = max_speed
                            # 更新传入的stats对象
                            stats.processed += 1
                            return result
                        else:
                            # 如果没有设置target_size，使用原有的单文件刷新模式
                            # 使用FF值填充文件
                            cls.full_refresh_file(path, size)
                            
                            # 计算处理速度
                            process_time = time.time() - start_time
                            if process_time > 0:
                                result['speed'] = size / process_time / 1024**2
                            return result
                    except KeyboardInterrupt:
                        # 处理用户中断
                        print("\n\n用户中断写入操作")
                        # 清理临时文件
                        if os.path.exists(temp_file) and not os.path.exists(path):
                            try:
                                os.remove(temp_file)
                            except:
                                pass
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
        
        # 添加主循环以支持返回主菜单
        while True:
            # 重置统计信息
            self.stats = OperationStats()
            
            # 用户配置阶段
            self.dashboard.update_display(self.stats, "初始化")
            LogManager.log_operation("程序进入执行阶段")
            
            # 重置本地模式标志
            local_full_refresh = full_refresh
            local_trim_mode = trim_mode
            confirmed = False
            
            # 显示菜单让用户选择操作模式
            if not full_refresh and not trim_mode:  # 只有在非命令行指定的情况下才显示菜单
                print("\n" + "="*50)
                print("          冷数据维护工具 - 操作模式选择")
                print("="*50)
                print("1. 智能模式 (推荐) - 保留原文件内容，仅激活冷数据")
                print("2. 全盘激活冷数据模式 (所有文件全部丢失无法找回) - 将文件内容替换为 FF 值 (ASCII 'f')")
                print("3. TRIM优化模式 (清理/如需找回数据不要使用这个模式) - 操作系统API来通知SSD哪些数据块是无效的，提高性能并延长寿命")
                print("="*50)
                
                while True:
                    choice = input("请选择操作模式 [1/2/3]: ").strip()
                    if choice == '1':
                        local_full_refresh = False
                        local_trim_mode = False
                        mode_name = "智能模式"
                        break
                    elif choice == '2':
                        local_full_refresh = True
                        local_trim_mode = False
                        mode_name = "全盘刷新模式"
                        break
                    elif choice == '3':
                        local_full_refresh = False
                        local_trim_mode = True
                        mode_name = "TRIM模式"
                        break
                    else:
                        print("无效的选择，请输入 1、2 或 3")
                
                LogManager.log_operation(f"用户选择操作模式: {mode_name}")
            
            # 根据不同模式显示相应的警告和确认提示
            if local_full_refresh:
                print("⚠️  警告: 正在使用全盘数据刷新模式！")
                print("   使用此模式将完全擦除SSD硬盘中的数据，所有文件内容将丢失且无法找回！")
                print(f"   所有文件内容将被替换为 {config.FULL_REFRESH_PATTERN.hex().upper()} 值 (ASCII '{chr(config.FULL_REFRESH_PATTERN[0])}')")
                print(f"   示例: 字符'{'A'}'将变为'{'f'}'，所有数据将被不可逆地覆盖")
                print("   此操作不可撤销，请确保您了解操作后果！")
                
                # 要求用户确认两次
                confirm1 = input("请输入 'yes' 确认执行全盘刷新操作 (第一次): ").strip().lower()
                if confirm1 != 'yes':
                    print("操作已取消")
                    print("返回主菜单...")
                    continue  # 返回到主菜单
                
                confirm2 = input("请再次输入 'yes' 确认执行全盘刷新操作 (第二次): ").strip().lower()
                if confirm2 != 'yes':
                    print("操作已取消")
                    print("返回主菜单...")
                    continue  # 返回到主菜单
                    
                LogManager.log_operation("用户已确认两次，开始执行全盘刷新操作")
                confirmed = True
            elif local_trim_mode:
                print("⚠️  警告: 正在使用TRIM优化模式！")
                print("   如需找回SSD中删除的数据请不要使用此模式，先找回数据以后再使用！")
                print(f"   通知SSD哪些数据块是无效的，提高写入性能、减少耗损")
                print(f"   针对文件前 {config.TRIM_BLOCK_SIZE/1024**2:.1f}MB 区域执行TRIM操作")
                
                # 要求用户确认一次
                confirm = input("请输入 'yes' 确认执行TRIM优化操作: ").strip().lower()
                if confirm != 'yes':
                    print("操作已取消")
                    print("返回主菜单...")
                    continue  # 返回到主菜单
                    
                LogManager.log_operation("用户已确认，开始执行TRIM优化操作")
                confirmed = True
            else:
                # 智能模式不需要确认
                confirmed = True
            
            if confirmed:
                # 使用确认的模式设置dashboard
                self.dashboard.full_refresh = local_full_refresh
                self.dashboard.trim_mode = local_trim_mode
                break  # 确认成功，退出循环继续执行后续流程

        # 获取目录输入
        directory = input("扫描目录: ").strip('"').replace('：', ':')  # 中文冒号转英文冒号
        
        # 根据不同模式设置参数
        if local_trim_mode or local_full_refresh:
            # TRIM模式或全盘刷新模式：仅要求输入路径/盘符，其他参数使用默认值
            print("自动设置参数...")
            # 对于Windows盘符，确保格式正确
            if os.name == 'nt' and len(directory) > 1 and directory[1] == ':' and not directory.endswith('\\'):
                directory += '\\'
            # 对于Linux路径，确保格式正确
            elif os.name == 'posix' and directory and not directory.endswith('/'):
                directory += '/'
            
            min_days = 0  # 处理所有文件，不按时效过滤
            skip_small = False  # 不跳过小文件
            
            # 只有在全盘刷新模式下才询问写入容量
            if local_full_refresh:
                print("\n请输入每个文件的写入容量 (1-100GB，默认50GB):")
                
                while True:
                    capacity_input = input("请输入容量大小: ").strip()
                    if not capacity_input:  # 空输入使用默认值50
                        capacity = 50
                        break
                    
                    try:
                        capacity = int(capacity_input)
                        if 1 <= capacity <= 100:
                            break
                        else:
                            print("容量必须在1-100之间，请重新输入")
                    except ValueError:
                        print("请输入有效的数字")
                
                target_size = capacity * 1024**3  # 转换为字节，使用局部变量
                print(f"已选择写入容量: {capacity}GB")
                # 保存目标大小到FileOperator类的类变量
                FileOperator.target_size = target_size
            
            print(f"路径: {directory}")
            print(f"数据时效: {min_days} 天 (自动设置)")
            print(f"跳过小文件: {'是' if skip_small else '否'} (自动设置)")
        else:
            # 常规模式：保持原有输入流程
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
        
        # 添加调试信息
        print(f"\n[调试信息]")
        print(f"扫描目录: {directory}")
        print(f"操作系统: {os.name}")
        print(f"当前工作目录: {os.getcwd()}")
        print(f"目录是否存在: {os.path.exists(directory)}")
        print(f"是否具有读取权限: {os.access(directory, os.R_OK)}")
        print(f"是否具有写入权限: {os.access(directory, os.W_OK)}")
        
        try:
            target_files = self._collect_files(directory, min_days)
            total_files = len(target_files)
            self.stats.progress = 0.1  # 进入处理阶段初始进度
            
            print(f"扫描完成，发现 {total_files} 个目标文件")
            # 显示前几个文件作为示例
            if target_files:
                print(f"\n发现的文件列表:")
                for i, file_path in enumerate(target_files[:3]):
                    print(f"  {i+1}. {file_path}")
                if len(target_files) > 3:
                    print(f"  ... 等 {len(target_files)} 个文件")
            else:
                print("警告: 未发现任何目标文件！")
                
            LogManager.log_operation(f"扫描完成，发现 {total_files} 个目标文件")
        except Exception as e:
            LogManager.log_operation(f"扫描目录失败: {str(e)}", "ERROR")
            raise

        # 文件处理阶段
        start_time = time.time()
        
        # 检查是否为全盘刷新模式
        if local_full_refresh:
            # 执行完整的全盘刷新业务流程
            print("\n开始执行全盘刷新业务流程...")
            
            # 1. 检查用户指定的目录是否为整个盘符
            is_drive = FullRefreshManager.is_drive(directory)
            print(f"\n检测到路径类型: {'整个盘符' if is_drive else '文件目录'}")
            
            # 2. 获取目录统计信息
            print("\n开始获取目录统计信息...")
            total_bytes, used_bytes, free_bytes = FullRefreshManager.get_directory_stats(directory)
            
            if total_bytes == 0:
                print("无法获取目录统计信息，程序将退出")
                input("按任意键返回主菜单...")
                return self.execute(full_refresh, trim_mode)
            
            print(f"磁盘信息:")
            print(f"  总容量: {format_size(total_bytes)}")
            print(f"  已使用容量: {format_size(used_bytes)}")
            print(f"  可用容量: {format_size(free_bytes)}")
            
            # 2. 询问用户是否保留已使用空间中的文件
            keep_files = input("\n是否保留已使用空间中的文件？ (Y/N, 默认Y): ").strip().lower()
            keep_files = keep_files != 'n' and keep_files != 'no'
            
            # 3. 备份文件（如果需要）
            backup_dir = None
            if keep_files and used_bytes > 0:
                print("\n开始备份文件...")
                backup_dir = os.path.join("d:", "$aspnmytools")
                if not os.path.exists(backup_dir):
                    os.makedirs(backup_dir)
                
                if FullRefreshManager.backup_files(directory, backup_dir):
                    print("文件备份成功")
                else:
                    print("文件备份失败，程序将退出")
                    input("按任意键返回主菜单...")
                    return self.execute(full_refresh, trim_mode)
            
            # 4. 根据是盘符还是目录，执行不同的清理操作
            format_success = False
            if is_drive:
                # 如果是盘符，先尝试格式化操作
                print("\n开始格式化盘符...")
                if FullRefreshManager.format_drive(directory):
                    print("盘符格式化成功")
                    format_success = True
                else:
                    print("盘符格式化失败，将尝试删除文件操作...")
            
            # 如果格式化失败或不是盘符，执行文件删除操作
            if not format_success:
                print("\n开始删除目录下的所有文件...")
                for root, dirs, files in os.walk(directory, topdown=False):
                    # 跳过系统保护目录
                    if "$RECYCLE.BIN" in root.upper() or "SYSTEM VOLUME INFORMATION" in root.upper():
                        continue
                    
                    # 删除文件
                    for file in files:
                        file_path = os.path.join(root, file)
                        try:
                            os.remove(file_path)
                        except Exception as e:
                            print(f"删除文件失败 {file_path}: {str(e)}")
                    
                    # 删除空目录
                    for dir_name in dirs:
                        dir_path = os.path.join(root, dir_name)
                        try:
                            os.rmdir(dir_path)
                        except Exception as e:
                            print(f"删除目录失败 {dir_path}: {str(e)}")
                
                print("目录清理完成")
            
            # 6. 获取写入单位大小
            unit_size = getattr(FileOperator, 'target_size', 50 * 1024**3)
            
            # 7. 填满可用空间
            print(f"\n开始填满可用空间，写入单位大小: {unit_size / 1024**3:.0f}GB")
            cumulative_capacity, max_write_speed = FullRefreshManager.fill_available_space(directory, unit_size)
            
            # 8. 清理工作目录
            work_dir = os.path.join(directory, "$aspnmytools")
            FullRefreshManager.cleanup(work_dir)
            
            # 9. 恢复文件（如果需要）
            if keep_files and backup_dir and os.path.exists(backup_dir):
                print("\n开始恢复文件...")
                if FullRefreshManager.restore_files(backup_dir, directory):
                    print("文件恢复成功")
                    # 删除备份目录
                    FullRefreshManager.cleanup(backup_dir, keep_backup=False)
                else:
                    print("文件恢复失败，请手动恢复")
            elif backup_dir:
                # 删除备份目录
                FullRefreshManager.cleanup(backup_dir, keep_backup=False)
            
            # 10. 执行TRIM优化操作（放在最后一步，避免中间流程卡住）
            print("\n开始执行TRIM优化操作...")
            # 创建一个临时文件用于触发TRIM操作
            trim_temp_file = os.path.join(directory, ".trim_temp")
            try:
                # 创建一个小文件
                with open(trim_temp_file, 'wb') as f:
                    f.write(b'\x00' * 1024)  # 1KB的临时文件
                
                # 调用trim_file函数执行TRIM操作
                FileOperator.trim_file(trim_temp_file, 1024)
                print("TRIM优化操作完成")
            except Exception as e:
                print(f"TRIM操作失败: {str(e)}")
            finally:
                # 清理临时文件
                if os.path.exists(trim_temp_file):
                    try:
                        os.remove(trim_temp_file)
                    except:
                        pass
            
            # 更新统计信息
            self.stats.processed = 1
            self.stats.progress = 1.0
            self.stats.speed = max_write_speed
            
            # 更新界面
            self.dashboard.update_display(self.stats, "完成")
        else:
            # 常规模式或TRIM模式：使用原有的多线程处理逻辑
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
                    
                    # 提交任务到线程池 - 使用用户选择的本地模式变量
                    print(f"\n准备处理文件: {path}")
                    print(f"文件大小: {os.path.getsize(path)/1024/1024:.2f} MB")
                    print(f"使用模式: {'TRIM' if local_trim_mode else '智能'}")
                    future = executor.submit(FileOperator.refresh_file, path, self.stats, self.dashboard, local_full_refresh, local_trim_mode)
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
        
        # 显示结果
        print("\n" + "="*60)
        print("          操作完成！")
        print("="*60)
        
        if local_full_refresh:
            # 全盘刷新模式的结果显示
            print(f"操作模式: 全盘刷新")
            print(f"总耗时: {elapsed_time:.2f} 秒")
            print(f"最大写入速度: {self.stats.speed:.2f} MB/s")
            print(f"操作日志: {log_file_path}")
            print(f"错误记录: {config.CORRUPTED_LOG}")
        else:
            # 常规模式或TRIM模式的结果显示
            print(f"操作模式: {'TRIM模式' if local_trim_mode else '智能模式'}")
            print(f"总耗时: {elapsed_time:.2f} 秒")
            print(f"处理文件数: {self.stats.processed} 个 (共发现 {self.stats.scanned} 个)")
            print(f"大文件: {self.stats.large}, 中等文件: {self.stats.medium}, 小文件: {self.stats.small}")
            print(f"损坏文件: {self.stats.corrupted}")
            print(f"平均处理速度: {self.stats.speed:.2f} MB/s")
            print(f"操作日志: {log_file_path}")
            print(f"错误记录: {config.CORRUPTED_LOG}")
        
        print("="*60)
        
        # 记录操作完成
        if local_full_refresh:
            mode = "全盘刷新"
        elif local_trim_mode:
            mode = "TRIM模式"
        else:
            mode = "常规刷新"
        LogManager.log_operation(f"操作完成 ({mode}): 扫描{self.stats.scanned}个文件, 处理{self.stats.processed}个, 损坏{self.stats.corrupted}个, 耗时{elapsed_time:.2f}秒")
        
        # 显示交互菜单
        while True:
            print("\n" + "="*50)
            print("          冷数据维护工具 - 操作完成")
            print("="*50)
            print("1. 返回 - 回到交互界面")
            print("2. 退出 - 关闭脚本")
            print("="*50)
            
            choice = input("请选择操作 [1/2]: ").strip()
            if choice == '1':
                # 返回主界面，重新执行
                return self.execute(full_refresh, trim_mode)
            elif choice == '2':
                # 退出程序
                print("感谢使用冷数据维护工具，再见！")
                LogManager.log_operation("用户选择退出程序")
                sys.exit(0)
            else:
                print("无效的选择，请输入 1 或 2")

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
    # 自动获取管理员权限
    if os.name == 'nt':
        try:
            # 检查是否已经是管理员权限
            import ctypes
            is_admin = ctypes.windll.shell32.IsUserAnAdmin() != 0
            if not is_admin:
                print("正在请求管理员权限...")
                # 使用ctypes请求管理员权限
                ctypes.windll.shell32.ShellExecuteW(
                    None,  # 父窗口句柄
                    "runas",  # 操作类型
                    sys.executable,  # 应用程序路径
                    ' '.join(['"' + arg + '"' for arg in sys.argv]),  # 命令行参数
                    None,  # 工作目录
                    1  # 显示方式（SW_SHOWNORMAL）
                )
                return  # 退出当前进程，等待新的管理员权限进程启动
        except Exception as e:
            print(f"自动获取管理员权限失败: {str(e)}")
            print("请手动以管理员权限运行程序")
    
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
                           help='使用全盘数据刷新模式（将文件内容统一写入FF值）')
        parser.add_argument('--trim-mode', action='store_true',
                           help='使用真正的TRIM功能（通知SSD哪些数据块无效，提高写入性能）')
        parser.add_argument('--test', action='store_true',
                           help='测试文件写入功能')
        parser.add_argument('--dir', type=str, default='H:',
                           help='测试目录，默认为H:')
        parser.add_argument('--size', type=int, default=50,
                           help='测试文件大小(GB)，默认为50GB')
        
        args = parser.parse_args()
        
        # 如果是测试模式
        if args.test:
            test_write_functionality(args.dir, args.size)
            return
        
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
