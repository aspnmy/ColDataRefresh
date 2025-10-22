#!/usr/bin/env python3
import os

# 读取version.txt文件内容
version_file = '/root/gitdata/ColDataRefresh/version.txt'
if os.path.exists(version_file):
    with open(version_file, 'r') as f:
        version = f.read().strip()
    print(f"Python读取的版本号: '{version}'")
    print(f"版本号长度: {len(version)}")
    
    # 模拟Windows批处理文件中的set /p行为
    # set /p通常会读取到第一个换行符或文件结束
    with open(version_file, 'r') as f:
        first_line = f.readline().rstrip('\r\n')
    print(f"模拟set/p读取的第一行: '{first_line}'")
else:
    print(f"文件 {version_file} 不存在")

# 分析make.bat的版本读取逻辑
print("\n分析make.bat版本读取逻辑:")
print('1. 使用 "< version.txt set /p APP_VERSION=" 直接读取第一行')
print('2. 移除所有空格: "set APP_VERSION=!APP_VERSION: =!"')
print('3. 验证是否为空，为空则使用默认值')
print("\n结论: 修改后的逻辑应该能正确读取版本号4.5.0")