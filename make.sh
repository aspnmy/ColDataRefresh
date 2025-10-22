#!/bin/bash

# 冷数据维护工具构建脚本(Linux版本)
# 用于在Linux环境下构建冷数据维护工具的可执行文件

# 保存当前目录并切换到脚本所在目录
CURRENT_DIR=$(pwd)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# 从version.txt读取版本号
DEFAULT_VERSION="4.3.3"
if [ -f "version.txt" ]; then
    APP_VERSION=$(cat version.txt | tr -d ' ')
else
    APP_VERSION="$DEFAULT_VERSION"
    echo "警告：未找到version.txt文件，使用默认版本 $DEFAULT_VERSION"
    echo "$DEFAULT_VERSION" > version.txt
fi

echo "正在构建冷数据维护工具 v$APP_VERSION.."

# 检查Python是否安装
if ! command -v python3 &> /dev/null && ! command -v python &> /dev/null; then
    echo "错误：未找到Python环境，请先安装Python"
    exit 1
fi

# 确定使用的Python命令
if command -v python3 &> /dev/null; then
    PYTHON_CMD="python3"
else
    PYTHON_CMD="python"
fi

# 检查pip是否可用
if ! $PYTHON_CMD -m pip --version &> /dev/null; then
    echo "错误：未找到pip，请确保Python已正确安装"
    exit 1
fi

# 安装必要的依赖
echo "正在安装依赖..."
if [ -f "requirements.txt" ]; then
    $PYTHON_CMD -m pip install -r requirements.txt
    if [ $? -ne 0 ]; then
        echo "警告：依赖安装失败，可能会影响构建"
    fi
else
    echo "警告：未找到requirements.txt文件"
fi

# 安装PyInstaller
echo "安装PyInstaller..."
$PYTHON_CMD -m pip install pyinstaller --upgrade
if [ $? -ne 0 ]; then
    echo "错误：PyInstaller安装失败，请检查网络连接或Python环境"
    echo "尝试使用sudo权限运行此脚本"
    exit 1
fi

# 检查必要文件是否存在
if [ ! -f "coldatafresh.py" ]; then
    echo "错误：找不到coldatafresh.py文件"
    exit 1
fi

if [ ! -f "devrom.ico" ]; then
    echo "警告：找不到devrom.ico图标文件，将使用默认图标"
fi

# 使用Python -m方式运行pyinstaller
echo "正在生成可执行文件..."
echo "使用Python模块方式调用pyinstaller..."

# 在Linux上构建，不使用--uac-admin参数
if [ -f "devrom.ico" ]; then
    $PYTHON_CMD -m PyInstaller --onefile --name ColDataFresh_v"$APP_VERSION"_linux --icon=devrom.ico coldatafresh.py
else
    $PYTHON_CMD -m PyInstaller --onefile --name ColDataFresh_v"$APP_VERSION"_linux coldatafresh.py
fi

if [ $? -eq 0 ]; then
    echo "构建完成！可执行文件: dist/ColDataFresh_v${APP_VERSION}_linux"
    echo "请以管理员权限运行生成的可执行文件"
    
    # 自动打包成zip文件
    echo "正在创建压缩文件..."
    
    # 获取7-zip工具
    SEVEN_ZIP_PATH="$SCRIPT_DIR/bin/7zz"
    if [ ! -f "$SEVEN_ZIP_PATH" ]; then
        echo "未找到7-zip工具，正在下载..."
        if [ -f "$SCRIPT_DIR/bin/get7-zip.sh" ]; then
            chmod +x "$SCRIPT_DIR/bin/get7-zip.sh"
            "$SCRIPT_DIR/bin/get7-zip.sh"
            if [ $? -ne 0 ]; then
                echo "错误：下载7-zip工具失败"
            fi
        else
            echo "错误：未找到get7-zip.sh脚本"
        fi
    fi
    
    # 检查7-zip是否可用
    if [ -f "$SEVEN_ZIP_PATH" ]; then
        cd dist
        ZIP_FILE="ColDataFresh_v${APP_VERSION}_linux.zip"
        # 使用7-zip创建zip文件
        # $SCRIPT_DIR/bin/7zz a "$ZIP_FILE" ColDataFresh_v${APP_VERSION}_linux
        "$SEVEN_ZIP_PATH" a "$ZIP_FILE" ColDataFresh_v${APP_VERSION}_linux
        if [ $? -eq 0 ]; then
            echo "压缩文件创建成功: dist/$ZIP_FILE"
        else
            echo "警告：压缩文件创建失败"
        fi
        cd ..
    else
        echo "错误：7-zip工具不可用，无法创建压缩文件"
    fi
    
    # 可选：尝试自动打开dist目录
    echo "正在打开输出目录..."
    if command -v xdg-open &> /dev/null; then
        xdg-open dist
    elif command -v nautilus &> /dev/null; then
        nautilus dist
    elif command -v dolphin &> /dev/null; then
        dolphin dist
    else
        echo "无法自动打开目录，请手动查看dist文件夹"
    fi
else
    echo "错误：构建失败"
    echo "请尝试："
    echo "1. 使用sudo权限运行此脚本"
    echo "2. 确保Python已正确安装并添加到系统PATH"
    echo "3. 检查网络连接是否正常"
fi

# 恢复原始目录
cd "$CURRENT_DIR"