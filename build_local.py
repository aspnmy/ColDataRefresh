#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
构建脚本（本地依赖版）
将所有依赖安装到项目本地目录，不依赖系统PATH环境
"""

import os
import sys
import subprocess
import zipfile
import shutil
from pathlib import Path

# 设置默认编码为UTF-8
os.environ['PYTHONIOENCODING'] = 'utf-8'

# 项目根目录
PROJECT_ROOT = Path(__file__).parent

# 虚拟环境目录
VENV_DIR = PROJECT_ROOT / ".venv"

# 依赖文件
REQUIREMENTS_FILE = PROJECT_ROOT / "requirements.txt"

# 主脚本文件
MAIN_SCRIPT = PROJECT_ROOT / "coldatafresh.py"

# 图标文件
ICON_FILE = PROJECT_ROOT / "devrom.ico"

# 版本文件
VERSION_FILE = PROJECT_ROOT / "version.txt"

# 输出目录
DIST_DIR = PROJECT_ROOT / "dist"

# 默认版本号
DEFAULT_VERSION = "4.5.0"


def get_version():
    """
    从version.txt读取版本号，如果文件不存在则使用默认版本
    
    Returns:
        str: 版本号
    """
    print(f"尝试读取版本文件: {VERSION_FILE}")
    
    if VERSION_FILE.exists():
        print(f"找到版本文件: {VERSION_FILE}")
        try:
            with open(VERSION_FILE, 'r', encoding='utf-8') as f:
                APP_VERSION = f.read().strip().replace(' ', '')
            
            if not APP_VERSION:
                print(f"警告：version.txt文件内容为空，使用默认版本 {DEFAULT_VERSION}")
                APP_VERSION = DEFAULT_VERSION
            else:
                print(f"成功从文件读取版本号: {APP_VERSION}")
        except Exception as e:
            print(f"读取版本文件失败: {e}")
            APP_VERSION = DEFAULT_VERSION
    else:
        print(f"错误：在路径 {VERSION_FILE} 未找到version.txt文件")
        APP_VERSION = DEFAULT_VERSION
        print(f"使用默认版本 {DEFAULT_VERSION}")
        # 尝试创建version.txt文件
        try:
            with open(VERSION_FILE, 'w', encoding='utf-8') as f:
                f.write(DEFAULT_VERSION)
            print("已创建version.txt文件")
        except Exception as e:
            print(f"创建version.txt文件失败: {e}")
    
    return APP_VERSION


def check_python():
    """
    检查Python是否安装
    
    Returns:
        bool: Python是否可用
    """
    try:
        result = subprocess.run([sys.executable, "--version"], capture_output=True, text=True, check=True)
        print(f"Python版本: {result.stdout.strip()}")
        return True
    except subprocess.CalledProcessError:
        print("错误：未找到Python环境，请先安装Python")
        return False
    except Exception as e:
        print(f"检查Python环境失败: {e}")
        return False


def create_venv():
    """
    创建虚拟环境
    
    Returns:
        bool: 虚拟环境创建是否成功
    """
    print("正在创建虚拟环境...")
    
    if VENV_DIR.exists():
        print(f"虚拟环境已存在: {VENV_DIR}")
        return True
    
    try:
        result = subprocess.run(
            [sys.executable, "-m", "venv", str(VENV_DIR)],
            capture_output=True,
            text=True,
            encoding='utf-8'
        )
        
        if result.returncode == 0:
            print(f"虚拟环境创建成功: {VENV_DIR}")
            return True
        else:
            print(f"虚拟环境创建失败: {result.stderr}")
            return False
    except Exception as e:
        print(f"创建虚拟环境时发生错误: {e}")
        return False


def get_venv_python():
    """
    获取虚拟环境中的Python可执行文件路径
    
    Returns:
        Path: Python可执行文件路径
    """
    if sys.platform == 'win32':
        return VENV_DIR / "Scripts" / "python.exe"
    else:
        return VENV_DIR / "bin" / "python"


def get_venv_pip():
    """
    获取虚拟环境中的pip可执行文件路径
    
    Returns:
        Path: pip可执行文件路径
    """
    if sys.platform == 'win32':
        return VENV_DIR / "Scripts" / "pip.exe"
    else:
        return VENV_DIR / "bin" / "pip"


def install_dependencies():
    """
    安装必要的依赖
    
    Returns:
        bool: 依赖安装是否成功
    """
    print("正在安装依赖...")
    
    pip_path = get_venv_pip()
    
    try:
        # 安装依赖（跳过pip升级，避免升级失败）
        result = subprocess.run(
            [str(pip_path), "install", "-r", str(REQUIREMENTS_FILE)],
            capture_output=True,
            text=True,
            encoding='utf-8'
        )
        
        if result.returncode == 0:
            print("依赖安装成功")
            return True
        else:
            print(f"警告：依赖安装失败，可能会影响构建")
            print(f"错误信息: {result.stderr}")
            return False
    except Exception as e:
        print(f"安装依赖时发生错误: {e}")
        return False


def install_pyinstaller():
    """
    安装PyInstaller
    
    Returns:
        bool: PyInstaller安装是否成功
    """
    print("安装PyInstaller...")
    
    pip_path = get_venv_pip()
    
    try:
        result = subprocess.run(
            [str(pip_path), "install", "pyinstaller"],
            capture_output=True,
            text=True,
            encoding='utf-8'
        )
        
        if result.returncode == 0:
            print("PyInstaller安装成功")
            return True
        else:
            print(f"错误：PyInstaller安装失败")
            print(f"错误信息: {result.stderr}")
            return False
    except Exception as e:
        print(f"安装PyInstaller时发生错误: {e}")
        return False


def check_files():
    """
    检查必要文件是否存在
    
    Returns:
        tuple: (coldatafresh.py是否存在, devrom.ico是否存在)
    """
    has_main = MAIN_SCRIPT.exists()
    has_icon = ICON_FILE.exists()
    
    if not has_main:
        print(f"错误：找不到主脚本文件 {MAIN_SCRIPT}")
    
    if not has_icon:
        print(f"警告：找不到图标文件 {ICON_FILE}，将使用默认图标")
    
    return has_main, has_icon


def build_executable(version, has_icon):
    """
    使用PyInstaller生成可执行文件
    
    Args:
        version: 版本号
        has_icon: 是否有图标文件
    
    Returns:
        bool: 构建是否成功
    """
    print("正在生成可执行文件...")
    
    # 构建输出文件路径
    exe_path = DIST_DIR / f"ColDataFresh_v{version}_win.exe"
    
    # 如果文件已存在，尝试重命名它，避免权限冲突
    if exe_path.exists():
        backup_path = DIST_DIR / f"ColDataFresh_v{version}_win_old.exe"
        try:
            exe_path.rename(backup_path)
            print(f"已将现有可执行文件重命名为: {backup_path.name}")
        except PermissionError as e:
            print(f"警告：无法重命名现有可执行文件 {exe_path.name}，可能正在被使用: {e}")
            # 尝试使用不同的输出名称
            cmd_version = f"{version}_new"
            print(f"将使用新的输出名称: ColDataFresh_v{cmd_version}_win.exe")
            version = cmd_version
    
    python_path = get_venv_python()
    
    # 构建PyInstaller命令
    cmd = [
        str(python_path), "-m", "PyInstaller",
        "--onefile",
        "--uac-admin",
        f"--name=ColDataFresh_v{version}_win",
        "--add-data=version.txt;."
    ]
    
    if has_icon:
        cmd.append("--icon=devrom.ico")
    
    cmd.append("coldatafresh.py")
    
    try:
        result = subprocess.run(
            cmd,
            text=True,
            encoding='utf-8',
            shell=False
        )
        
        return result.returncode == 0
    except Exception as e:
        print(f"构建可执行文件时发生错误: {e}")
        return False


def create_zip(version):
    """
    创建压缩文件
    
    Args:
        version: 版本号
    """
    print("正在创建压缩文件...")
    
    exe_file = DIST_DIR / f"ColDataFresh_v{version}_win.exe"
    zip_file = DIST_DIR / f"ColDataFresh_v{version}_win.zip"
    
    if not exe_file.exists():
        print(f"错误：找不到可执行文件 {exe_file}")
        return
    
    try:
        with zipfile.ZipFile(zip_file, 'w', zipfile.ZIP_DEFLATED) as zf:
            zf.write(exe_file, exe_file.name)
        print(f"压缩文件创建成功: {zip_file}")
    except Exception as e:
        print(f"警告：压缩文件创建失败: {e}")


def open_dist_dir():
    """
    打开输出目录
    """
    print("正在打开输出目录...")
    
    if not DIST_DIR.exists():
        print(f"错误：输出目录 {DIST_DIR} 不存在")
        return
    
    try:
        if sys.platform == 'win32':
            os.startfile(DIST_DIR)
        elif sys.platform == 'darwin':
            subprocess.run(['open', str(DIST_DIR)])
        else:
            subprocess.run(['xdg-open', str(DIST_DIR)])
    except Exception as e:
        print(f"打开输出目录失败: {e}")


def clean_build():
    """
    清理构建文件
    """
    print("正在清理构建文件...")
    
    # 清理PyInstaller生成的文件
    clean_dirs = [
        PROJECT_ROOT / "build"
    ]
    
    clean_files = [
        PROJECT_ROOT.glob("*.spec"),
        PROJECT_ROOT.glob("*.pyc"),
        PROJECT_ROOT.glob("__pycache__")
    ]
    
    # 清理目录
    for dir_path in clean_dirs:
        if dir_path.exists():
            try:
                shutil.rmtree(dir_path)
                print(f"已清理目录: {dir_path}")
            except PermissionError as e:
                print(f"警告：无法清理目录 {dir_path}，权限不足: {e}")
            except Exception as e:
                print(f"警告：清理目录 {dir_path} 失败: {e}")
    
    # 清理dist目录中的旧文件（保留目录）
    if DIST_DIR.exists():
        try:
            for item in DIST_DIR.iterdir():
                if item.is_dir():
                    try:
                        shutil.rmtree(item)
                        print(f"已清理子目录: {item}")
                    except PermissionError as e:
                        print(f"警告：无法清理子目录 {item}，权限不足: {e}")
                    except Exception as e:
                        print(f"警告：清理子目录 {item} 失败: {e}")
                else:
                    try:
                        item.unlink()
                        print(f"已清理文件: {item}")
                    except PermissionError as e:
                        print(f"警告：无法清理文件 {item}，权限不足: {e}")
                    except Exception as e:
                        print(f"警告：清理文件 {item} 失败: {e}")
        except Exception as e:
            print(f"警告：清理dist目录内容失败: {e}")
    
    # 清理其他文件
    for file_pattern in clean_files:
        for file_path in file_pattern:
            try:
                if file_path.is_dir():
                    shutil.rmtree(file_path)
                    print(f"已清理目录: {file_path}")
                else:
                    file_path.unlink()
                    print(f"已清理文件: {file_path}")
            except PermissionError as e:
                print(f"警告：无法清理 {file_path}，权限不足: {e}")
            except Exception as e:
                print(f"警告：清理 {file_path} 失败: {e}")


def main():
    """
    主函数
    """
    print("=" * 60)
    print("冷数据维护工具构建脚本（本地依赖版）")
    print("=" * 60)
    print("特点：")
    print("  - 所有依赖安装到项目本地，不依赖系统环境")
    print("  - 使用虚拟环境管理依赖")
    print("  - 提高跨平台兼容性")
    print("  - 简化用户使用步骤")
    print("=" * 60)
    
    # 获取版本号
    version = get_version()
    print(f"当前版本号: {version}")
    print(f"正在构建冷数据维护工具 v{version}..")
    
    # 检查Python环境
    if not check_python():
        input("按任意键继续...")
        return 1
    
    # 创建虚拟环境
    if not create_venv():
        input("按任意键继续...")
        return 1
    
    # 安装依赖
    install_dependencies()
    
    # 安装PyInstaller
    if not install_pyinstaller():
        input("按任意键继续...")
        return 1
    
    # 检查必要文件
    has_main, has_icon = check_files()
    if not has_main:
        input("按任意键继续...")
        return 1
    
    # 清理旧的构建文件
    clean_build()
    
    # 构建可执行文件
    if build_executable(version, has_icon):
        print(f"构建完成！可执行文件: {DIST_DIR}/ColDataFresh_v{version}_win.exe")
        print("请以管理员权限运行生成的可执行文件")
        
        # 创建压缩文件
        create_zip(version)
        
        # 打开输出目录
        open_dist_dir()
    else:
        print("错误：构建失败")
        print("请尝试：")
        print("1. 以管理员权限运行此脚本")
        print("2. 确保Python已正确安装")
        print("3. 检查网络连接是否正常")
        input("按任意键继续...")
        return 1
    
    return 0


if __name__ == "__main__":
    sys.exit(main())
