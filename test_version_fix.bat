@echo off
setlocal enabledelayedexpansion

echo ====== 版本号读取修复测试 ======

REM 保存当前目录并切换到脚本所在目录
set "CURRENT_DIR=%cd%"
echo 当前目录: %CURRENT_DIR%

REM 获取脚本所在目录的绝对路径
set "SCRIPT_DIR=%~dp0"
echo 脚本目录: %SCRIPT_DIR%

cd /d "%SCRIPT_DIR%"
echo 切换后目录: %cd%

REM 从version.txt读取版本号
set "DEFAULT_VERSION=4.3.2"
set "VERSION_FILE=%SCRIPT_DIR%version.txt"

echo 尝试读取版本文件: %VERSION_FILE%

REM 检查文件是否存在
if exist "%VERSION_FILE%" (
    echo 找到版本文件: %VERSION_FILE%
    REM 使用绝对路径和更可靠的方式读取版本号
    for /f "usebackq tokens=1 delims=\r\n" %%i in ("%VERSION_FILE%") do (
        set "APP_VERSION=%%i"
        REM 移除所有空格
        set "APP_VERSION=!APP_VERSION: =!"
        goto VersionFound
    )
    :VersionFound
    REM 验证版本号是否有效
    if "!APP_VERSION!"=="" (
        echo 警告：version.txt文件内容为空，使用默认版本 !DEFAULT_VERSION!
        set "APP_VERSION=!DEFAULT_VERSION!"
    ) else (
        echo 成功从文件读取版本号: !APP_VERSION!
    )
) else (
    echo 错误：在路径 %VERSION_FILE% 未找到version.txt文件
    set "APP_VERSION=!DEFAULT_VERSION!"
    echo 使用默认版本 !DEFAULT_VERSION!
    REM 尝试创建version.txt文件
    echo !DEFAULT_VERSION! > "%VERSION_FILE%"
    echo 已创建version.txt文件
)

REM 显示测试结果
echo 当前版本号: !APP_VERSION!
if "!APP_VERSION!"=="4.5.0" (
    echo 测试通过！正确读取了version.txt中的版本号 4.5.0
) else (
    echo 测试失败！读取的版本号 !APP_VERSION! 与预期的 4.5.0 不匹配
)

echo ====== 测试完成 ======
pause