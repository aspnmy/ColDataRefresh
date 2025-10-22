#!/bin/bash

# 下载并安装7-zip组件脚本
# 用于从官方网站下载7-zip Linux版本并解压到bin目录

# 保存当前目录
CURRENT_DIR=$(pwd)

# 确保脚本在bin目录中执行
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "正在下载7-zip组件..."

# 7-zip下载URL
SEVEN_ZIP_URL="https://www.7-zip.org/a/7z2501-linux-x64.tar.xz"
SEVEN_ZIP_FILE="7z2501-linux-x64.tar.xz"
SEVEN_ZIP_DIR="7z2501-linux-x64"

# 下载7-zip文件
if command -v wget &> /dev/null; then
    wget -q "$SEVEN_ZIP_URL" -O "$SEVEN_ZIP_FILE"
elif command -v curl &> /dev/null; then
    curl -s -L "$SEVEN_ZIP_URL" -o "$SEVEN_ZIP_FILE"
else
    echo "错误：未找到wget或curl命令，无法下载7-zip组件"
    cd "$CURRENT_DIR"
    exit 1
fi

# 检查下载是否成功
if [ ! -f "$SEVEN_ZIP_FILE" ]; then
    echo "错误：7-zip文件下载失败"
    cd "$CURRENT_DIR"
    exit 1
fi

echo "下载完成，正在解压7-zip..."

# 解压tar.xz文件
if command -v tar &> /dev/null; then
    tar -xf "$SEVEN_ZIP_FILE"
    if [ $? -ne 0 ]; then
        echo "警告：使用tar解压失败，尝试直接搜索7zz文件..."
    fi
else
    echo "警告：未找到tar命令，尝试直接搜索7zz文件..."
fi

# 搜索7zz可执行文件（更健壮的方式）
if [ -f "7zz" ]; then
    # 如果直接解压到当前目录
    cp "7zz" "$SCRIPT_DIR/"
elif [ -d "$SEVEN_ZIP_DIR" ] && [ -f "$SEVEN_ZIP_DIR/7zz" ]; then
    # 如果解压到子目录
    cp "$SEVEN_ZIP_DIR/7zz" "$SCRIPT_DIR/"
else
    # 尝试查找任何包含7zz的目录
    SEVEN_ZIP_FOUND=$(find . -name "7zz" 2>/dev/null | head -1)
    if [ -n "$SEVEN_ZIP_FOUND" ]; then
        cp "$SEVEN_ZIP_FOUND" "$SCRIPT_DIR/"
    else
        echo "错误：无法找到7zz可执行文件"
        rm -f "$SEVEN_ZIP_FILE"
        # 尝试清理可能的目录
        find . -type d -name "7z*" -exec rm -rf {} \; 2>/dev/null
        cd "$CURRENT_DIR"
        exit 1
    fi
fi

# 检查复制是否成功
if [ ! -f "$SCRIPT_DIR/7zz" ]; then
    echo "错误：7zz可执行文件复制失败"
    rm -f "$SEVEN_ZIP_FILE"
    # 尝试清理可能的目录
    find . -type d -name "7z*" -exec rm -rf {} \; 2>/dev/null
    cd "$CURRENT_DIR"
    exit 1
fi

# 设置可执行权限
chmod +x "$SCRIPT_DIR/7zz"

# 清理临时文件
# 删除下载的压缩包
rm -f "$SEVEN_ZIP_FILE"
# 删除任何7z开头的目录
find . -type d -name "7z*" -exec rm -rf {} \; 2>/dev/null
# 删除可能的单个7zz文件（如果存在）
rm -f "7zz"

echo "7-zip组件安装成功！"
echo "7zz可执行文件位于: $SCRIPT_DIR/7zz"

# 恢复原始目录
cd "$CURRENT_DIR"