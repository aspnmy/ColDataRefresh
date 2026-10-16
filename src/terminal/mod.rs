//! 终端控制：彩色输出、ASCII 回退、窗口标题。
//!
//! 平台无关逻辑（颜色包装、字符选择）留在本文件；平台专有实现按分册维护：
//! - `terminal/linux.rs`   —— Linux 控制台（ANSI 转义序列）
//! - `terminal/windows.rs` —— Windows 控制台（Win32 Console API / VT 检测）
//! - `terminal/darwin.rs`  —— macOS（同 Linux 的 ANSI 路径）

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
mod darwin;

#[cfg(target_os = "linux")]
use linux as imp;

#[cfg(target_os = "windows")]
use windows as imp;

#[cfg(target_os = "macos")]
use darwin as imp;

use std::sync::OnceLock;

/// 终端管理器 — 处理彩色输出、ASCII回退模式、窗口标题
pub struct Terminal;

impl Terminal {
    /// 是否使用 ASCII 回退（平台专有判定，惰性初始化）
    pub fn is_safe() -> bool {
        static ASCII_FALLBACK: OnceLock<bool> = OnceLock::new();
        *ASCII_FALLBACK.get_or_init(imp::use_ascii_fallback)
    }

    /// 清除屏幕（平台专有实现）
    pub fn clear() {
        if Self::is_safe() {
            imp::clear_ascii();
        } else {
            imp::clear_ansi();
        }
        // 确保 stdout 刷新
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }

    /// 生成带 ANSI 颜色的文本
    pub fn colored(text: &str, fg: u8, bg: u8) -> String {
        if Self::is_safe() {
            text.to_string()
        } else {
            format!("\x1B[{};{}m{}\x1B[0m", fg, bg, text)
        }
    }

    /// 带前景色
    pub fn fg(text: &str, fg: u8) -> String {
        if Self::is_safe() {
            text.to_string()
        } else {
            format!("\x1B[{}m{}\x1B[0m", fg, text)
        }
    }

    /// 设置控制台窗口标题（平台专有实现）
    pub fn set_window_title(title: &str) {
        imp::set_window_title(title);
    }
}

/// 边框字符选择
pub fn border_chars() -> (&'static str, &'static str, &'static str) {
    if Terminal::is_safe() {
        ("=", "|", "=")
    } else {
        ("═", "│", "═")
    }
}

/// 进度条字符
pub fn progress_chars() -> (&'static str, &'static str) {
    if Terminal::is_safe() {
        ("#", "-")
    } else {
        ("▓", "░")
    }
}
