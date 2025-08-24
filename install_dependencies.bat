@echo off
echo 正在安装冷数据刷新工具依赖库...
echo.

REM 检查Python是否安装
python --version >nul 2>&1
if errorlevel 1 (
    echo 错误: 未找到Python，请先安装Python 3.6+
    pause
    exit /b 1
)

REM 安装依赖库
echo 检查依赖库...
pip install -r requirements.txt

if errorlevel 1 (
    echo 安装失败，请检查网络连接或pip配置
    pause
    exit /b 1
)

echo.
echo 依赖库安装完成！
echo 现在可以运行 coldatafresh.py 了
pause