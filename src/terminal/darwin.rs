//! macOS 控制台专有实现 —— 平台无关逻辑在 `terminal/mod.rs`。
//!
//! macOS 终端一律走 ANSI 转义序列（与 Linux 同路径，无平台 Console API）。

/// 检测是否需回退 ASCII —— macOS 支持 UTF-8/ANSI，恒不需要回退
pub fn use_ascii_fallback() -> bool {
    false
}

/// 清屏（ASCII 回退路径）—— macOS 下仍用 ANSI
pub fn clear_ascii() {
    print!("\x1B[2J\x1B[H");
}

/// 清屏（ANSI 路径）
pub fn clear_ansi() {
    print!("\x1B[2J\x1B[H");
}

/// 设置控制台窗口标题 —— ANSI OSC 序列
pub fn set_window_title(title: &str) {
    print!("\x1B]0;{}\x07", title);
}
