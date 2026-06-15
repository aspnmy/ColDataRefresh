# ColDataRefresh — SSD Cold Data Maintenance Tool v5.0

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

# Smart mode: refresh files older than 180 days
coldatafresh -p "D:\Data" -a 180

# Full disk refresh
coldatafresh -f -p "D:\Data"

# Execute TRIM only
coldatafresh -t -p "D:\Data"

# Verbose logging
coldatafresh -v -p "D:\Data" -a 365
```

### CLI Options

| Flag | Description |
|------|-------------|
| `-p`, `--path` | Target directory (default: `.`) |
| `-a`, `--age` | File age threshold in days |
| `-f`, `--full-refresh` | Full disk refresh mode |
| `-t`, `--trim` | TRIM optimization mode |
| `-v`, `--verbose` | Enable detailed logging |
| `-s`, `--skip-smaller` | Skip files smaller than N MB |

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
git checkout v5.0
git tag v5.0.0
git push origin v5.0.0
```

The workflow builds for 11 targets:
| Platform | Targets |
|----------|---------|
| Linux (gnu) | `x86_64`, `aarch64`, `armv7`, `arm` |
| Linux (musl) | `x86_64`, `i686`, `aarch64` |
| macOS | `x86_64`, `aarch64` |
| Windows | `x86_64`, `i686` |

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

### v5.0.0 — Rust Rewrite
- Complete rewrite from Python to Rust
- Thread-safe architecture (`OnceLock` + `Mutex`, no `static mut`)
- Full disk refresh: backup → delete → overwrite → restore (with timestamp refresh) → TRIM
- CLI arguments for non-interactive / scripted use
- Real-time progress dashboard
- Cross-platform: Windows + Linux

## License

MIT License — see [LICENSE](LICENSE).

## Author

**aspnmy** — [Blog](https://aspnmy.blog.csdn.net/)
