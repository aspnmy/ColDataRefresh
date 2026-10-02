# ColDataRefresh — SSD Cold Data Maintenance Tool v5.0.3

> **This branch (`v5`) is the Rust main line.**
> Original author & upstream repository (Python original): **infrost** — <https://github.com/infrost/ColDataRefresh.git>
> This branch builds on the original author's Python implementation and evolved through a Python→Rust rewrite; full credit to the original author.

[中文](README.md)

Intelligently detects cold data on SSDs and prevents read slowdown caused by charge leakage on NAND cells. Written in Rust for maximum performance and reliability.

## Features

### Mode 1: Cold Data Refresh (Smart Mode)
Refreshes files that haven't been accessed beyond a configurable age threshold (default 365 days). Each file is read, its data is verified via CRC32, written back in-place with 0xFF then restored, and re-verified. This rewrites the physical NAND cells, restoring their charge level and read performance.

**Safe — no data loss.**

### Mode 2: Full Disk Refresh
A complete NAND cell-level refresh cycle:
1. **Backup** — All files are backed up to another drive (auto-detected, prioritizes D:)
2. **Delete** — Original files are removed to free up space
3. **Overwrite** — The freed space is overwritten with 0xFF pattern for full NAND cell refresh
4. **Cleanup** — Temporary fill files are removed
5. **Restore** — Data is restored from backup with **current timestamps** (files appear "fresh" to the OS)
6. **TRIM** — Final TRIM optimization is executed

> ⚠️ Toggleable backup: choose whether to preserve data. If backup is skipped, restored data cannot be recovered.

### Mode 3: Real-time TRIM
Directly issues TRIM commands to the SSD, bypassing the OS idle-time scheduling. Safe for routine maintenance every 3 months. Irreversibly releases space marked as deleted.

## Usage

```bash
# Interactive menu (no args)
coldatafresh

# CLI (non-interactive) mode: requires explicit --cli and a mandatory -p
# Smart mode: refresh files older than 180 days
coldatafresh --cli -p "/data" -a 180

# Full disk refresh (destructive — -y required; keep/no-keep must be explicit)
coldatafresh --cli -p "/data" -f -y --keep-files

# Full disk refresh + fill free space (overwrites unallocated space, unrecoverable)
coldatafresh --cli -p "/data" -f -y --no-keep-files --fill-free --unit-gb 50 --write-buf-kb 512

# Execute TRIM only (destructive — -y required)
coldatafresh --cli -p "/data" -t -y

# Verbose logging + skip files smaller than 10MB
coldatafresh --cli -p "/data" -a 365 -s 10 -v
```

> **Interactive vs non-interactive:** running without `--cli` opens the interactive menu (same behavior as v5.0); with `--cli` the program never reads stdin and exits with an error when a required argument is missing (exit code `2`) — suitable for scripting / unattended use.

### CLI Options

| Flag | Description |
|------|-------------|
| `--cli` | Enter CLI (non-interactive) mode; `-p` is mandatory |
| `-p`, `--path` | Target directory (required in `--cli` mode; ignored in interactive mode) |
| `-a`, `--age` | File age threshold in days (required in smart mode) |
| `-f`, `--full-refresh` | Full disk refresh mode |
| `-t`, `--trim` | TRIM optimization mode |
| `-y`, `--yes` | Confirmation for destructive operations (required with `-f` / `-t` under `--cli`) |
| `-b`, `--buffer-size` | Processing buffer size in MB (effective under `--cli`) |
| `--keep-files` | Full refresh: keep files (backup → delete → fill → restore); one of the two is required with `-cli -f` |
| `--no-keep-files` | Full refresh: do not keep files (unrecoverable) |
| `--fill-free` | Full refresh: also fill free space (unrecoverable) |
| `--unit-gb` | Write size per file when filling free space (GB, 1-100, default 50) |
| `--write-buf-kb` | Write buffer size (KB, 64~1048576, default 512) |
| `-s`, `--skip-smaller` | Skip files smaller than N MB |
| `-v`, `--verbose` | Enable detailed logging |

### Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | Completed with failures |
| `2` | Argument error (missing args, directory not found, destructive op without `-y`) |
| `130` | Interrupted by user (Ctrl+C) |

## Installation

### From Source
```bash
git clone https://github.com/aspnmy/ColDataRefresh.git
cd ColDataRefresh
cargo build --release
./target/release/coldatafresh
```

Requires Rust 2021 Edition or later.

### Pre-built Binaries
Download the latest release from the [Releases](https://github.com/aspnmy/ColDataRefresh/releases) page.

## CI/CD

This project uses GitHub Actions for automated cross-platform release builds.

Trigger a release:
```bash
git checkout v5.0.0
git tag v5.0.3
git push origin v5.0.0
git push origin v5.0.3
```

Build matrix (11 targets):

| Platform | Target Triple | Libc | CPU Arch | Use Case |
|----------|--------------|------|----------|----------|
| Linux | `x86_64-unknown-linux-gnu` | glibc | x86_64 (64-bit) | Desktop/Server主流 |
| Linux | `x86_64-unknown-linux-musl` | musl | x86_64 (64-bit) | Alpine/Docker 静态编译 |
| Linux | `i686-unknown-linux-musl` | musl | i686 (32-bit) | 旧硬件/嵌入式 |
| Linux | `aarch64-unknown-linux-gnu` | glibc | ARMv8 (64-bit) | 树莓派/ARM服务器 |
| Linux | `aarch64-unknown-linux-musl` | musl | ARMv8 (64-bit) | ARM Alpine/Docker |
| Linux | `armv7-unknown-linux-gnueabihf` | glibc | ARMv7 (32-bit) | 树莓派3及以下 |
| Linux | `arm-unknown-linux-gnueabihf` | glibc | ARMv6 (32-bit) | 树莓派Zero/旧ARM |
| macOS | `x86_64-apple-darwin` | — | Intel Mac | MacBook Pro/Air (Intel) |
| macOS | `aarch64-apple-darwin` | — | Apple Silicon | MacBook Pro/Air (M芯片) |
| Windows | `x86_64-pc-windows-msvc` | MSVC | x86_64 (64-bit) | Win10/11 主流 |
| Windows | `i686-pc-windows-msvc` | MSVC | i686 (32-bit) | Win10/11 32位兼容 |

> **glibc vs musl:** glibc 版本性能更优，适合桌面/服务器；musl 版本静态链接，适合 Docker/Alpine 容器环境。ARM 版本覆盖树莓派全系列（Zero~5）。

## System Requirements

| Platform | Support |
|----------|---------|
| Windows 10/11 | ✅ Full support (NTFS, ReFS) |
| Linux | ✅ Full support (ext4, XFS, Btrfs) |

## Technical Details

- **Language**: Rust 2021 Edition
- **Concurrency**: Rayon lock-free parallel processing
- **Data Integrity**: CRC32 checksum before and after every write
- **Logging**: Centralized log system with operation, error, and corruption reports
- **Signal Handling**: Graceful Ctrl+C shutdown with interrupted file logging
- **No runtime dependencies** — single static binary

## Changelog

### v5.0.3 — Parameterized Full Refresh for CLI
- **Full refresh under `--cli` no longer reads stdin at all**: added `--keep-files` / `--no-keep-files` (one required with `-cli -f`), `--fill-free`, `--unit-gb`, `--write-buf-kb`, replacing the previous 5 interactive prompts
- Fixed the full-refresh exit code: `execute()` returned nothing and reported `0` even on failure; it now returns the real result (`0` success / `1` failure)
- Interactive mode behavior is unchanged (still uses the original stdin prompts)
- All **three modes (smart / full refresh / TRIM) are now fully available in both CLI and interactive mode**

### v5.0.2 — CLI Mode & Bilingual Text
- **Genuine CLI (non-interactive) mode**: explicitly triggered by `--cli`, never reads stdin — suitable for scripting / unattended use
- `-p/--path` now actually takes effect under `--cli` (previously the flag was never wired up and every flow still prompted on stdin)
- **Destructive-operation gate**: full disk refresh / TRIM under `--cli` require the explicit `-y/--yes` flag
- **Missing arguments now exit with an error** (exit code `2`) instead of falling back to interactive prompts
- New `-b/--buffer-size` option; `-v/--verbose` now wired to log level control
- Standardized exit codes: `0` success / `1` completed with failures / `2` argument error / `130` user interrupt
- **Fixed false TRIM success report**: previously always displayed "✅ success" regardless of outcome; now reports the real return value
- **UI and prompts are now bilingual** (Chinese with English in parentheses); interactive mode behavior is unchanged
- Source comments unified as `///` doc comments / `//!` module comments for rustdoc

### v5.0.1 — Internal Revision

### v5.0.0 — Rust Rewrite
- Complete rewrite from Python to Rust
- Thread-safe architecture (`OnceLock` + `Mutex`, no `static mut`)
- Full disk refresh: backup → delete → overwrite → restore (with timestamp refresh) → TRIM
- CLI arguments for non-interactive / scripted use
- Real-time progress dashboard
- Cross-platform: Windows + Linux

## License

Apache License 2.0 — see [LICENSE](LICENSE).

## Author

**aspnmy** — [Blog](https://aspnmy.blog.csdn.net/)
