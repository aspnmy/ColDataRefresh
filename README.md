# ColDataRefresh — SSD 冷数据维护系统 v6.0 beta 一些说明
- v6.0版本主要用于对 大模型业务中 进行offlineload模式下把大量数据卸载到ssd中的这些数据进行定向维护，拼合碎片，均匀分配ssd热区，延长ssd的寿命
- 此版本主要对接agent助手，内置MCP_server 或者以标准openai接口协议对接，以skill或mcp的形式和任意agent-cli对接，定期对ssd进行维护，此版本开始以后将不在面向人类用户(没有交互菜单)
- 建议docker部署 ，不要用二进制裸跑，它的操作权限是很高的，一旦操作将数据完全丢失。
- v6.0开始的版本将在我们自己的git上进行维护：**https://git.t2be.cn/aspnmy/ColDataRefresh.git**
- v6.0版本开始 将提供i18n-语义包，需要其他语言的，请对i18n-zh.lang进行翻译即可 
- 如需预览v6.0版本请向邮箱写邮件说明应用场景，将给予我们内部仓库的协作者只读权限

# ColDataRefresh — SSD 冷数据维护系统 v5.0.3

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

# 命令行（非交互）模式：必须显式加 --cli，且必须指定 -p
# 智能模式：刷新 180 天以上未修改的文件
coldatafresh --cli -p "/data" -a 180

# 全盘刷新模式（破坏性操作，必须加 -y；且必须显式指定是否保留文件）
coldatafresh --cli -p "/data" -f -y --keep-files

# 全盘刷新 + 填充空闲空间（覆写未分配空间，不可恢复）
coldatafresh --cli -p "/data" -f -y --no-keep-files --fill-free --unit-gb 50 --write-buf-kb 512

# 仅执行 TRIM（破坏性操作，必须加 -y 确认）
coldatafresh --cli -p "/data" -t -y

# 详细日志输出 + 跳过小于 10MB 的文件
coldatafresh --cli -p "/data" -a 365 -s 10 -v
```

> **交互 vs 非交互**：不带 `--cli` 启动即进入交互菜单（行为与 v5.0 一致）；带 `--cli` 则全程不读 stdin，缺参直接报错退出（退出码 `2`），适合脚本化 / 无人值守。

### 命令行参数

| 参数 | 说明 |
|------|------|
| `--cli` | 进入命令行（非交互）模式；必须同时指定 `-p` |
| `-p`, `--path` | 目标目录（`--cli` 模式下必填；交互模式下忽略） |
| `-a`, `--age` | 文件年龄阈值（天，智能模式必填） |
| `-f`, `--full-refresh` | 全盘刷新模式 |
| `-t`, `--trim` | TRIM 优化模式 |
| `-y`, `--yes` | 破坏性操作确认（`--cli` 下使用 `-f` / `-t` 时必填） |
| `-b`, `--buffer-size` | 处理缓冲区大小（MB，`--cli` 下生效） |
| `--keep-files` | 全盘刷新：保留文件（备份→删除→填充→恢复）；`-cli -f` 必填其一 |
| `--no-keep-files` | 全盘刷新：不保留文件（数据不可恢复） |
| `--fill-free` | 全盘刷新：额外填充空闲空间（不可恢复） |
| `--unit-gb` | 填充空闲空间时每文件写入容量（GB，1-100，默认 50） |
| `--write-buf-kb` | 写入缓冲区大小（KB，64~1048576，默认 512） |
| `-s`, `--skip-smaller` | 跳过小于 N MB 的文件 |
| `-v`, `--verbose` | 启用详细日志 |

### 退出码

| 码 | 含义 |
|----|------|
| `0` | 成功 |
| `1` | 运行中存在失败项 |
| `2` | 参数错误（缺参、目录不存在、破坏性操作未加 `-y`） |
| `130` | 用户中断（Ctrl+C） |

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

## CI/CD 自动发布

项目使用 GitHub Actions 实现跨平台自动发布。

触发发布：
```bash
git checkout v5.0.0
git tag v5.0.3
git push origin v5.0.0
git push origin v5.0.3
```

构建矩阵（11 个目标平台）：

| 平台 | 目标三元组 | 运行时库 | CPU 架构 | 适用场景 |
|------|-----------|---------|---------|---------|
| Linux | `x86_64-unknown-linux-gnu` | glibc | x86_64 (64位) | 桌面/服务器主流 |
| Linux | `x86_64-unknown-linux-musl` | musl | x86_64 (64位) | Alpine/Docker 静态编译 |
| Linux | `i686-unknown-linux-musl` | musl | i686 (32位) | 旧硬件/嵌入式 |
| Linux | `aarch64-unknown-linux-gnu` | glibc | ARMv8 (64位) | 树莓派/ARM服务器 |
| Linux | `aarch64-unknown-linux-musl` | musl | ARMv8 (64位) | ARM Alpine/Docker |
| Linux | `armv7-unknown-linux-gnueabihf` | glibc | ARMv7 (32位) | 树莓派3及以下 |
| Linux | `arm-unknown-linux-gnueabihf` | glibc | ARMv6 (32位) | 树莓派Zero/旧ARM |
| macOS | `x86_64-apple-darwin` | — | Intel Mac | MacBook Pro/Air (Intel) |
| macOS | `aarch64-apple-darwin` | — | Apple Silicon | MacBook Pro/Air (M芯片) |
| Windows | `x86_64-pc-windows-msvc` | MSVC | x86_64 (64位) | Win10/11 主流 |
| Windows | `i686-pc-windows-msvc` | MSVC | i686 (32位) | Win10/11 32位兼容 |

> **glibc vs musl 说明：** glibc 版本性能更优，适合桌面/服务器环境；musl 版本静态链接，不依赖系统运行时，适合 Docker/Alpine 容器。ARM 版本覆盖树莓派全系列（Zero 到 5）。

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

### v5.0.3 — 全盘刷新 CLI 参数化
- **全盘刷新在 `--cli` 下彻底不读 stdin**：新增 `--keep-files` / `--no-keep-files`（`-cli -f` 必填其一）、`--fill-free`、`--unit-gb`、`--write-buf-kb`，替代原有的 5 处交互提问
- 修复全盘刷新退出码：原 `execute()` 无返回值、失败也报 `0`（假成功），现返回真实结果（`0` 成功 / `1` 失败）
- 交互模式行为完全不变（仍走原来的 stdin 提问流程）
- 至此**三种模式（智能 / 全盘刷新 / TRIM）在 CLI 与交互模式下全部可用**

### v5.0.2 — 命令行模式与双语文案
- **新增真正的命令行（非交互）模式**：`--cli` 显式触发，全程不读 stdin，适合脚本化 / 无人值守
- `-p/--path` 在 `--cli` 下真实生效（此前该参数未接线、各流程仍会读取标准输入）
- **破坏性操作闸门**：`--cli` 下执行全盘刷新 / TRIM 必须显式加 `-y/--yes`
- **缺参即报错退出**（退出码 `2`），不再回落到交互提问
- 新增 `-b/--buffer-size` 参数；`-v/--verbose` 接入日志级别控制
- 退出码规范化：`0` 成功 / `1` 存在失败项 / `2` 参数错误 / `130` 用户中断
- **修复 TRIM 结果误报**：此前无论成功失败均显示"✅ 成功"，现按真实返回值上报
- **界面与提示改为中英双语文案**（中文 + 括号内英文），交互模式行为保持不变
- 源码注释统一为 `///` 文档注释 / `//!` 模块注释，便于 rustdoc 抓取

### v5.0.1 — 内部修订

### v5.0.0 — Rust 完全重写
- 从 Python 完全迁移到 Rust
- 线程安全架构（`OnceLock` + `Mutex`，`static mut` 全部消除）
- 全盘刷新完整流程：备份 → 删除 → 覆写 → 恢复（刷新时间戳）→ TRIM
- 命令行参数支持脚本化调用
- 实时进度仪表盘
- 跨平台：Windows + Linux

## 许可证

Apache License 2.0 — 详见 [LICENSE](LICENSE)。

## 作者

**aspnmy** — [博客](https://aspnmy.blog.csdn.net/)
