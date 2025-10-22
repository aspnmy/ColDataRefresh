@echo off
setlocal enabledelayedexpansion

REM 从version.txt读取版本号（使用修改后的代码）
set "DEFAULT_VERSION=4.3.2"
if exist "version.txt" (
    REM 使用更可靠的方式读取版本号，避免换行符和空格问题
    for /f "usebackq tokens=1 delims=\r\n" %%i in ("version.txt") do (
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
    )
) else (
    set "APP_VERSION=!DEFAULT_VERSION!"
    echo 警告：未找到version.txt文件，使用默认版本 !DEFAULT_VERSION!
)

REM 显示测试结果
echo 测试结果：成功读取版本号为 !APP_VERSION!
if "!APP_VERSION!"=="4.5.0" (
    echo 测试通过！正确读取了version.txt中的版本号 4.5.0
) else (
    echo 测试失败！读取的版本号 !APP_VERSION! 与预期的 4.5.0 不匹配
)

pause