#!/usr/bin/env python3
# FullDataFresh.py - 全盘数据覆写工具
# 功能：对指定路径或盘符进行全空间覆写操作

import os
import sys
import time
import platform
import shutil
import random
import string
from concurrent.futures import ThreadPoolExecutor

# 尝试导入elevate模块用于获取管理员权限
try:
    import elevate
    HAS_ELEVATE = True
except ImportError:
    HAS_ELEVATE = False

def get_version():
    """
    从version.txt文件读取当前版本号
    支持PyInstaller打包后的环境
    """
    try:
        if hasattr(sys, '_MEIPASS'):
            VERSION_FILE = os.path.join(sys._MEIPASS, 'version.txt')
        else:
            VERSION_FILE = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'version.txt')
        
        with open(VERSION_FILE, 'r', encoding='utf-8') as f:
            return f.read().strip()
    except Exception:
        return '4.5.0'

CURRENT_VERSION = get_version()

def clear_screen():
    """清屏函数"""
    os.system('cls' if os.name == 'nt' else 'clear')

def print_banner():
    """打印程序横幅"""
    banner = f'''
    ========================================================
            全盘数据覆写工具 v{CURRENT_VERSION}
    ========================================================
    功能：对指定路径或盘符进行全空间覆写操作
    注意：此操作会占用目标路径所有可用空间，可能影响系统性能
    ========================================================
    '''
    print(banner)

def check_admin_privileges():
    """检查并获取管理员/root权限"""
    if platform.system() == 'Windows':
        # 在Windows上检查管理员权限
        is_admin = ctypes.windll.shell32.IsUserAnAdmin() != 0
        if not is_admin and HAS_ELEVATE:
            print("需要管理员权限以进行全盘操作...")
            elevate.elevate()
    else:
        # 在Linux/Unix上检查root权限
        if os.geteuid() != 0 and HAS_ELEVATE:
            print("需要root权限以进行全盘操作...")
            elevate.elevate()

def get_directory_space_info(path):
    """
    获取目录或盘符的空间信息
    返回(total_space_gb, free_space_gb)
    """
    try:
        # Windows路径处理
        if platform.system() == 'Windows' and len(path) == 2 and path[1] == ':':
            path = path + '\\'
        
        # 获取磁盘使用情况
        usage = shutil.disk_usage(path)
        total_space_gb = usage.total / (1024**3)
        free_space_gb = usage.free / (1024**3)
        
        return total_space_gb, free_space_gb
    except Exception as e:
        print(f"获取空间信息失败: {e}")
        return 0, 0

def generate_random_filename(length=16):
    """生成随机文件名"""
    letters = string.ascii_letters + string.digits
    return ''.join(random.choice(letters) for _ in range(length))

def write_file_chunks(file_path, chunk_size=1024*1024*1024):  # 1GB
    """
    按块写入文件，每块1GB
    """
    try:
        with open(file_path, 'wb') as f:
            # 创建1MB的数据块（全0）
            buffer = b'\x00' * (1024 * 1024)
            chunks_to_write = chunk_size // (1024 * 1024)
            
            for i in range(chunks_to_write):
                f.write(buffer)
                # 每100MB显示一次进度
                if (i + 1) % 100 == 0:
                    print(f"  写入进度: {((i + 1) / chunks_to_write) * 100:.1f}%", end='\r')
        return True
    except Exception as e:
        print(f"  写入文件失败: {e}")
        if os.path.exists(file_path):
            try:
                os.remove(file_path)
            except:
                pass
        return False

def fill_space_with_zero_files(path, free_space_gb, chunk_size_gb=1):
    """
    用全0文件填充可用空间
    """
    files_created = []
    total_written_gb = 0
    max_file_size_gb = chunk_size_gb  # 每文件1GB
    
    print(f"开始填充空间，目标大小: {free_space_gb:.2f} GB")
    start_time = time.time()
    
    try:
        while total_written_gb < free_space_gb - 1:  # 留1GB空间避免系统问题
            # 计算本次要写入的文件大小
            remaining_gb = free_space_gb - total_written_gb
            file_size_gb = min(max_file_size_gb, remaining_gb)
            
            # 生成随机文件名
            filename = generate_random_filename() + '.fill'
            file_path = os.path.join(path, filename)
            
            print(f"创建文件: {filename} ({file_size_gb} GB)")
            
            # 写入文件
            success = write_file_chunks(file_path, int(file_size_gb * 1024 * 1024 * 1024))
            
            if success:
                files_created.append(file_path)
                total_written_gb += file_size_gb
                print(f"  已写入: {total_written_gb:.2f} / {free_space_gb:.2f} GB")
            else:
                # 如果写入失败，可能是空间已满，退出循环
                print("写入失败，可能空间已满")
                break
    except KeyboardInterrupt:
        print("\n操作已中断")
    except Exception as e:
        print(f"填充过程中出错: {e}")
    
    end_time = time.time()
    elapsed_minutes = (end_time - start_time) / 60
    
    print(f"\n填充完成!")
    print(f"  实际写入: {total_written_gb:.2f} GB")
    print(f"  耗时: {elapsed_minutes:.2f} 分钟")
    print(f"  创建文件数: {len(files_created)}")
    
    return files_created

def delete_files(file_list):
    """
    删除创建的文件
    """
    print(f"\n开始删除 {len(file_list)} 个文件...")
    start_time = time.time()
    
    deleted_count = 0
    for file_path in file_list:
        try:
            if os.path.exists(file_path):
                os.remove(file_path)
                deleted_count += 1
                if deleted_count % 10 == 0:
                    print(f"  已删除: {deleted_count}/{len(file_list)}", end='\r')
        except Exception as e:
            print(f"  删除文件失败 {file_path}: {e}")
    
    end_time = time.time()
    elapsed_seconds = end_time - start_time
    
    print(f"  已删除: {deleted_count}/{len(file_list)}")
    print(f"  删除耗时: {elapsed_seconds:.2f} 秒")
    
    return deleted_count

def ask_cycle_count():
    """
    询问用户循环次数
    """
    while True:
        try:
            count = input("请输入覆写-删除循环次数 (1-3，默认1): ").strip()
            if not count:
                return 1  # 默认1次
            
            count = int(count)
            if 1 <= count <= 3:
                return count
            else:
                print("错误: 循环次数必须在1到3之间")
        except ValueError:
            print("错误: 请输入有效的数字")

def validate_path(path):
    """
    验证路径是否存在且可写
    """
    if not os.path.exists(path):
        print(f"错误: 路径 {path} 不存在")
        return False
    
    if not os.access(path, os.W_OK):
        print(f"错误: 没有写入权限: {path}")
        return False
    
    # 测试是否可以在路径下创建文件
    test_file = os.path.join(path, 'test_write_access.tmp')
    try:
        with open(test_file, 'wb') as f:
            f.write(b'test')
        os.remove(test_file)
        return True
    except Exception as e:
        print(f"错误: 无法在路径下写入文件: {e}")
        return False

def get_target_path():
    """
    获取用户输入的目标路径
    """
    while True:
        path = input("请输入要覆写的目标路径或盘符: ").strip()
        
        # Windows盘符处理
        if platform.system() == 'Windows' and len(path) == 1 and path.isalpha():
            path = path + ':/'
        
        if validate_path(path):
            return path

def main():
    """主函数"""
    clear_screen()
    print_banner()
    
    # 检查并获取管理员权限
    check_admin_privileges()
    
    # 获取目标路径
    target_path = get_target_path()
    
    # 获取空间信息
    total_space_gb, free_space_gb = get_directory_space_info(target_path)
    
    if total_space_gb <= 0 or free_space_gb <= 0:
        print("无法获取有效的空间信息，程序退出")
        sys.exit(1)
    
    print(f"\n空间信息:")
    print(f"  总容量: {total_space_gb:.2f} GB")
    print(f"  可用空间: {free_space_gb:.2f} GB")
    print(f"  已用空间: {(total_space_gb - free_space_gb):.2f} GB")
    
    # 再次确认
    confirm = input("\n警告: 此操作将占用目标路径的所有可用空间，可能影响系统性能。继续? (y/n): ").lower()
    if confirm != 'y':
        print("操作已取消")
        sys.exit(0)
    
    # 获取循环次数
    cycle_count = ask_cycle_count()
    
    print(f"\n开始执行 {cycle_count} 次覆写-删除循环")
    print("=" * 60)
    
    for cycle in range(1, cycle_count + 1):
        print(f"\n【循环 {cycle}/{cycle_count}】")
        
        # 1. 覆写阶段
        print("\n1. 覆写阶段:")
        files_created = fill_space_with_zero_files(target_path, free_space_gb)
        
        # 2. 删除阶段
        print("\n2. 删除阶段:")
        delete_files(files_created)
        
        print(f"\n循环 {cycle}/{cycle_count} 完成")
        print("=" * 60)
    
    print("\n✓ 所有覆写-删除循环已完成！")
    print("\n提示: 您可以考虑运行磁盘整理工具来进一步优化磁盘性能。")
    
    # 等待用户按键
    if os.name == 'nt':
        os.system('pause')
    else:
        input("按Enter键退出...")

if __name__ == "__main__":
    main()