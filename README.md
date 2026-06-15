# ColDataRefresh — SSD 冷数据维护系统 v5.0

[English](README_EN.md)

智能检测固态硬盘（SSD）的冷数据，解决 NAND 颗粒电荷泄漏导致的读取掉速问题。使用 Rust 重写，兼顾高性能与数据安全。

## 功能模式

### 模式 1：冷数据维护（智能模式）
扫描指定目录中超过设定天数（默认 365 天）未修改的文件。每个文件：读取 → CRC32 校验 → 原地重写 → 读回验证。物理刷新 NAND 单元，恢复电荷水平，解决掉速问题。

**安全无风险 — 不会丢失数据。**

### 模式 2：全盘刷新
完整的 NAND 单元级刷新流程：
1. **备份** — 所有文件自动备份到另一块硬盘（自动检测，优先 D:）
2. **删除** — 删除目标目录中的原始文件释放空间
3. **覆写** — 用 0xFF 模式覆写已释放空间，全面刷新 NAND 单元
4. **清理** — 删除临时填充文件
5. **恢复** — 从备份恢复数据，**时间戳自动设为当前时间**（文件变为"新"文件）
6. **TRIM** — 执行最终 TRIM 优化

> ⚠️ 可选择不保留文件，全盘刷新后数据不可恢复。

### 模式 3：实时 TRIM
跳过系统空闲调度策略，直接向 SSD 发送 TRIM 指令，立即释放已标记删除的空间。日常维护建议每 3 个月执行一次。

## 快速使用

```bash
# 交互式菜单（无参数）
coldatafresh

# 智能模式：刷新 180 天以上未修改的文件
coldatafresh -p "D:\Data" -a 180

# 全盘刷新模式
coldatafresh -f -p "D:\Data"

# 仅执行 TRIM
coldatafresh -t -p "D:\Data"

# 详细日志输出
coldatafresh -v -p "D:\Data" -a 365
```

### 命令行参数

| 参数 | 说明 |
|------|------|
| `-p`, `--path` | 目标目录（默认当前目录） |
| `-a`, `--age` | 文件年龄阈值（天） |
| `-f`, `--full-refresh` | 全盘刷新模式 |
| `-t`, `--trim` | TRIM 优化模式 |
| `-v`, `--verbose` | 启用详细日志 |
| `-s`, `--skip-smaller` | 跳过小于 N MB 的文件 |

## 安装

### 源码编译
```bash
git clone https://github.com/aspnmy/ColDataRefresh.git
cd ColDataRefresh
cargo build --release
./target/release/coldatafresh
```

需要 Rust 2021 Edition 或更高版本。

### 预编译二进制
从 [Releases](https://github.com/aspnmy/ColDataRefresh/releases) 页面下载最新版本。

## 系统支持

| 平台 | 支持情况 |
|------|---------|
| Windows 10/11 | ✅ 完整支持（NTFS, ReFS） |
| Linux | ✅ 完整支持（ext4, XFS, Btrfs） |

## 技术特性

- **语言**：Rust 2021 Edition，零成本抽象
- **并发**：Rayon 无锁并行，多文件并发处理
- **数据完整性**：每次写入前后均做 CRC32 校验
- **日志系统**：操作日志、错误日志、文件损坏报告集中管理
- **信号处理**：Ctrl+C 优雅退出，记录已处理文件
- **零运行时依赖** — 单文件静态编译

## 更新日志

### v5.0.0 — Rust 完全重写
- 从 Python 完全迁移到 Rust
- 线程安全架构（`OnceLock` + `Mutex`，`static mut` 全部消除）
- 全盘刷新完整流程：备份 → 删除 → 覆写 → 恢复（刷新时间戳）→ TRIM
- 命令行参数支持脚本化调用
- 实时进度仪表盘
- 跨平台：Windows + Linux

## 许可证

MIT License — 详见 [LICENSE](LICENSE)。

## 作者

**aspnmy** — [博客](https://aspnmy.blog.csdn.net/)
