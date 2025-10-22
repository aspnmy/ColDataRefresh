@echo off
setlocal enabledelayedexpansion

REM 保存当前目录并切换到脚本所在目录
set "CURRENT_DIR=%cd%"
cd /d "%~dp0"

echo 正在构建冷数据维护工具 v4.3.2..

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

REM 检查pyinstaller是否安装，如果没有则安装
echo 检查PyInstaller...
pyinstaller --version >nul 2>&1
if !errorlevel! neq 0 (
    echo 正在安装PyInstaller...
    pip install pyinstaller
    if !errorlevel! neq 0 (
        echo 错误：PyInstaller安装失败
        pause
        exit /b 1
    )
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

REM 使用pyinstaller构建单文件可执行程序
echo 正在生成可执行文件...
if exist "devrom.ico" (
    pyinstaller --onefile --uac-admin --name coldatafresh_v4.3.2 --icon=devrom.ico coldatafresh.py
) else (
    pyinstaller --onefile --uac-admin --name coldatafresh_v4.3.2 coldatafresh.py
)

if !errorlevel! equ 0 (
    echo 构建完成！可执行文件: dist\coldatafresh_v4.3.2.exe
    echo 请以管理员权限运行生成的可执行文件
    
    REM 可选：自动打开dist目录
    echo 正在打开输出目录...
    start dist
) else (
    echo 错误：构建失败
    pause
)

REM 恢复原始目录
cd /d "%CURRENT_DIR%"
endlocal
