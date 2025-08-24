@echo off
echo 正在构建冷数据维护工具 v4.3.2..
pyinstaller --onefile --uac-admin --name coldatafresh_v4.3.2 --icon=devrom.ico coldatafresh.py
echo 构建完成！可执行文件: dist\coldatafresh_v4.3.exe
