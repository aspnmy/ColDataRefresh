//! Linux 控制台专有实现 —— 平台无关逻辑在 `terminal/mod.rs`。
//!
//! Linux 终端一律走 ANSI 转义序列（无需 ASCII 回退、无需平台 API）。

/// 检测是否需回退 ASCII —— Linux 支持 UTF-8/ANSI，恒不需要回退
pub fn use_ascii_fallback() -> bool {
    false
}

/// 清屏（ASCII 回退路径）—— Linux 下仍用 ANSI（无平台 API 可用）
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
