@echo off
setlocal enabledelayedexpansion

REM 保存当前目录并切换到脚本所在目录
set "CURRENT_DIR=%cd%"
cd /d "%~dp0"

REM 从version.txt读取版本号
set "DEFAULT_VERSION=4.3.2"
if exist "version.txt" (
    set /p "APP_VERSION=<version.txt"
    set "APP_VERSION=!APP_VERSION: =!"
) else (
    set "APP_VERSION=!DEFAULT_VERSION!"
    echo 警告：未找到version.txt文件，使用默认版本 !DEFAULT_VERSION!
    echo !DEFAULT_VERSION! > version.txt
)

echo 正在构建冷数据维护工具 v!APP_VERSION!..

REM 检查Python是否安装
python --version >nul 2>&1
if !errorlevel! neq 0 (
    echo 错误：未找到Python环境，请先安装Python
    pause
    exit /b 1
)

REM 安装必要的依赖
echo 正在安装依赖...
pip install -r requirements.txt
if !errorlevel! neq 0 (
    echo 警告：依赖安装失败，可能会影响构建
)

REM 强制安装pyinstaller，避免PATH问题
echo 安装PyInstaller...
pip install pyinstaller --upgrade
if !errorlevel! neq 0 (
    echo 错误：PyInstaller安装失败，请检查网络连接或Python环境
    echo 尝试使用管理员权限运行此脚本
    pause
    exit /b 1
)

REM 检查必要文件是否存在
if not exist "coldatafresh.py" (
    echo 错误：找不到coldatafresh.py文件
    pause
    exit /b 1
)

if not exist "devrom.ico" (
    echo 警告：找不到devrom.ico图标文件，将使用默认图标
)

REM 使用Python -m方式运行pyinstaller，避免PATH环境变量问题
echo 正在生成可执行文件...
echo 使用Python模块方式调用pyinstaller...
if exist "devrom.ico" (
    python -m PyInstaller --onefile --uac-admin --name coldatafresh_v!APP_VERSION! --icon=devrom.ico coldatafresh.py
) else (
    python -m PyInstaller --onefile --uac-admin --name coldatafresh_v!APP_VERSION! coldatafresh.py
)

if !errorlevel! equ 0 (
    echo 构建完成！可执行文件: dist\coldatafresh_v!APP_VERSION!.exe
    echo 请以管理员权限运行生成的可执行文件
    
    REM 可选：自动打开dist目录
    echo 正在打开输出目录...
    start dist
) else (
    echo 错误：构建失败
    echo 请尝试：
    echo 1. 以管理员权限运行此脚本
    echo 2. 确保Python已正确安装并添加到系统PATH
    echo 3. 检查网络连接是否正常
    pause
)

REM 恢复原始目录
cd /d "%CURRENT_DIR%"
endlocal
