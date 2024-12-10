import os
import time
import threading
import signal
import json
import zlib
import shutil
import random
from concurrent.futures import ThreadPoolExecutor
from elevate import elevate

# 全局变量
LOG_FILE = "refresh_log.json"
BUFFER_SIZE = 4 * 1024  # 缓冲区大小
ENABLE_MULTITHREADING = False  # 设置为 False 时禁用多线程
THREAD_COUNT = 4  # 线程数
BENCHMARK_SIZE_GB = 1  # 基准速度测试大小 (GB)
RATIO = 0.3 # 假设基准测试读取值为100MB/s, 若测试文件读取速度为100*0.3 = 30MB/s，则判断为冷数据
SKIP_SIZE = 1 * 1024**2 #小于1（MB）的文件会被跳过。删除此行或填0则不跳过文件。
EXIT_FLAG = False  # 用于检测是否终止程序,请不要修改这个

def signal_handler(sig, frame):
    global EXIT_FLAG
    print("\nTerminating program...")
    EXIT_FLAG = True

signal.signal(signal.SIGINT, signal_handler)

def load_log():
    if os.path.exists(LOG_FILE):
        with open(LOG_FILE, "r") as f:
            return json.load(f)
    return {"pending": [], "completed": []}

def save_log(log):
    with open(LOG_FILE, "w") as f:
        json.dump(log, f)

def benchmark_speed(directory, size_in_gb=BENCHMARK_SIZE_GB):
    size_in_bytes = size_in_gb * 1024**3
    small_file_sizes = [random.randint(100 * 1024, 10 * 1024**2) for _ in range(10)]  # 100KB - 10MB
    medium_file_sizes = [random.randint(10 * 1024**2, 100 * 1024**2) for _ in range(10)]  # 10MB - 100MB

    benchmark_results = {
        "large": {"speed": 0, "file_size": size_in_gb * 1024**3},
        "medium": {"speed": 0, "file_size": sum(medium_file_sizes)},
        "small": {"speed": 0, "file_size": sum(small_file_sizes)},
    }

    # 大文件测试
    try:
        benchmark_file = os.path.join(directory, "benchmark_large.bin")
        print(f"Benchmarking large file ({size_in_gb}GB)...")
        with open(benchmark_file, "wb") as f:
            for _ in range(size_in_bytes // BUFFER_SIZE):
                f.write(os.urandom(BUFFER_SIZE))

        start = time.time()
        with open(benchmark_file, "rb") as f:
            while f.read(BUFFER_SIZE):
                pass
        elapsed = time.time() - start
        benchmark_results["large"]["speed"] = size_in_bytes / elapsed / 1024**2  # MB/s
        os.remove(benchmark_file)
    except Exception as e:
        print(f"Error in large file benchmark: {e}")

    # 中小文件测试
    for category, file_sizes in [("medium", medium_file_sizes), ("small", small_file_sizes)]:
        files = []
        try:
            # 写入多个文件
            for idx, file_size in enumerate(file_sizes):
                file_path = os.path.join(directory, f"benchmark_{category}_{idx}.bin")
                with open(file_path, "wb") as f:
                    f.write(os.urandom(file_size))
                files.append(file_path)

            start = time.time()
            for file_path in files:
                with open(file_path, "rb") as f:
                    while f.read(BUFFER_SIZE):
                        pass
            elapsed = time.time() - start
            benchmark_results[category]["speed"] = sum(file_sizes) / elapsed / 1024**2  # MB/s
        except Exception as e:
            print(f"Error in {category} file benchmark: {e}")
        finally:
            for file_path in files:
                if os.path.exists(file_path):
                    os.remove(file_path)
    return benchmark_results

def refresh_file(file_path, benchmark_speed_results, max_retries=2):
    if EXIT_FLAG:
        return

    temp_path = file_path + ".temp"
    checksum_src = 0
    checksum_dest = 0
    retries = 0

    try:
        file_size = os.path.getsize(file_path)

        # 判断文件大小，选择合适的基准速度
        if file_size > 100 * 1024**2:  # 大于100MB，使用大文件基准
            benchmark_speed = benchmark_speed_results["large"]["speed"]
        elif file_size > 10 * 1024**2:  # 10MB-100MB，使用中等文件基准
            benchmark_speed = benchmark_speed_results["medium"]["speed"]
        else:  # 小于10MB，使用小文件基准
            benchmark_speed = benchmark_speed_results["small"]["speed"]

        # 如果文件太小，跳过刷新
        if file_size <= BUFFER_SIZE:
            print(f"Skipping tiny file: {file_path} (size: {file_size} bytes)")
            return

        if SKIP_SIZE and file_size <= SKIP_SIZE:
            print(f"Skipping tiny file: {file_path} (size: {file_size} bytes)")
            return

        file_speed = test_read_speed(file_path)

        if file_speed < benchmark_speed * RATIO:
            print(f"Refreshing cold data: {file_path} (read speed: {file_speed:.2f} MB/s, benchmark: {benchmark_speed:.2f} MB/s)")

            # 读取和写入
            while retries < max_retries:
                with open(file_path, "rb") as src, open(temp_path, "wb") as dest:
                    while chunk := src.read(BUFFER_SIZE):
                        checksum_src = zlib.crc32(chunk, checksum_src)
                        dest.write(chunk)
                        checksum_dest = zlib.crc32(chunk, checksum_dest)

                # 校验
                if checksum_src == checksum_dest:
                    break
                else:
                    retries += 1
                    print(f"CRC mismatch, retrying {file_path}... ({retries}/{max_retries})")
                    os.remove(temp_path)
            else:
                # 如果多次重试失败，保留源文件并报告损坏
                print(f"Failed to refresh {file_path} after {max_retries} retries. The file might be corrupted.")
                return

            # 保留原文件时间
            file_stat = os.stat(file_path)
            shutil.move(temp_path, file_path)
            os.utime(file_path, (file_stat.st_atime, file_stat.st_mtime))  # 恢复时间戳

            # 保留原文件夹时间
            if os.path.isdir(file_path):
                dir_stat = os.stat(os.path.dirname(file_path))
                os.utime(os.path.dirname(file_path), (dir_stat.st_atime, dir_stat.st_mtime))  # 恢复目录时间戳

            print(f"File refreshed: {file_path}")

        else:
            print(f"Skipping non-cold data: {file_path} (read speed: {file_speed:.2f} MB/s)")

    except Exception as e:
        print(f"Error refreshing {file_path}: {e}")

    finally:
        if os.path.exists(temp_path):
            os.remove(temp_path)


# 多线程刷新文件
def refresh_files(cold_files, benchmark_speed):
    log = load_log()

    # 筛选待处理文件
    pending_files = list(set(cold_files) - set(log["completed"]))
    log["pending"] = pending_files
    save_log(log)

    lock = threading.Lock()

    def worker(file_path):
        if EXIT_FLAG:
            return
        try:
            refresh_file(file_path, benchmark_speed)
            with lock:
                log["completed"].append(file_path)
                save_log(log)
        except Exception as e:
            print(f"Thread error: {e}")

    if ENABLE_MULTITHREADING:
        # 使用多线程池
        with ThreadPoolExecutor(max_workers=THREAD_COUNT) as executor:
            futures = [executor.submit(worker, file) for file in pending_files]
            for future in futures:
                if EXIT_FLAG:
                    break
                future.result()
    else:
        for file_path in pending_files:
            if EXIT_FLAG:
                break
            worker(file_path)


def scan_files(directory, min_days_old=30):
    now = time.time()
    cold_files = []

    print(f"Scanning files in directory: {directory} for files older than {min_days_old} days...")
    for root, _, files in os.walk(directory):
        for file in files:
            file_path = os.path.join(root, file)
            try:
                stat = os.stat(file_path)
                if (now - stat.st_atime) > min_days_old * 86400:
                    cold_files.append(file_path)
            except Exception as e:
                print(f"Error accessing file {file_path}: {e}")

    print(f"Found {len(cold_files)} cold files.")
    return cold_files

def test_read_speed(file_path):
    try:
        start = time.time()
        with open(file_path, "rb") as f:
            while f.read(BUFFER_SIZE):
                pass
        elapsed = time.time() - start
        file_size = os.path.getsize(file_path)
        read_speed = file_size / elapsed / 1024**2  # MB/s
        return read_speed
    except Exception as e:
        print(f"Error testing read speed for file {file_path}: {e}")
        return 0

# 主函数
def main():
    try:
        elevate()
    except Exception as e:
        print("Warning: some files may fail to refresh without granting administrator privileges")
    directory = input("Enter directory to scan for cold data: ").strip('"')
    min_days_old = int(input("Enter minimum days to consider data as cold: "))

    print("Benchmarking speed for new data...")
    benchmark_speed_value = benchmark_speed(directory, BENCHMARK_SIZE_GB)

    print(f"Benchmark read speed for large files: {benchmark_speed_value['large']['speed']:.2f} MB/s")
    print(f"Benchmark read speed for medium files: {benchmark_speed_value['medium']['speed']:.2f} MB/s")
    print(f"Benchmark read speed for small files: {benchmark_speed_value['small']['speed']:.2f} MB/s")


    print("Scanning for cold files...")
    cold_files = scan_files(directory, min_days_old)
    if not cold_files:
        print("No cold files found. Exiting.")
        return

    print("Refreshing cold files...")
    refresh_files(cold_files, benchmark_speed_value)
    print("All tasks completed.")
    input("Press Enter to exit...")

if __name__ == "__main__":
    main()
