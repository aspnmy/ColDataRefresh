#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
测试构建脚本 - 用于验证pyinstaller命令的正确语法
"""

import os
import subprocess
import sys

def main():
    """执行测试构建命令"""
    print("开始测试构建命令...")
    
    # 构建命令 - 这是我们在Windows批处理中使用的核心命令
    # 在Linux环境下，我们移除--uac-admin参数，因为它只在Windows上有效
    cmd = [
        sys.executable, "-m", "PyInstaller", 
        "--onefile", 
        "--name", "coldatafresh_v4.3.2", 
        "coldatafresh.py"
    ]
    
    # 检查图标文件是否存在
    if os.path.exists("devrom.ico"):
        cmd.extend(["--icon", "devrom.ico"])
        print("检测到图标文件，将包含在构建中")
    else:
        print("未检测到图标文件，将使用默认图标")
    
    print(f"执行命令: {' '.join(cmd)}")
    
    try:
        # 执行命令
        result = subprocess.run(
            cmd,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True
        )
        
        print("命令执行成功！")
        print("\n输出摘要:")
        # 显示最后几行输出
        last_lines = result.stdout.strip().split('\n')[-10:]
        for line in last_lines:
            print(f"  {line}")
            
        return 0
        
    except subprocess.CalledProcessError as e:
        print(f"命令执行失败，退出码: {e.returncode}")
        print("\n错误输出:")
        print(e.stderr)
        return e.returncode
    except Exception as e:
        print(f"发生异常: {str(e)}")
        return 1

if __name__ == "__main__":
    sys.exit(main())