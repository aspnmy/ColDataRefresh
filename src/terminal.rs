use std::sync::OnceLock;

/// 终端管理器 — 处理彩色输出、ASCII回退模式、窗口标题
pub struct Terminal;

impl Terminal {
    /// 检测终端是否支持 UTF-8/ANSI（否则回退到纯 ASCII）
    fn use_ascii_fallback() -> bool {
        #[cfg(windows)]
        {
            match std::env::var("WT_SESSION") {
                Ok(_) => false, // Windows Terminal — 支持 UTF-8
                Err(_) => {
                    let cp = unsafe { windows_sys::Win32::System::Console::GetConsoleOutputCP() };
                    cp != 65001 // 非 UTF-8 代码页 → 回退 ASCII
                }
            }
        }
        #[cfg(not(windows))]
        false
    }

    /// 是否使用 ASCII 回退（惰性初始化）
    pub fn is_safe() -> bool {
        static ASCII_FALLBACK: OnceLock<bool> = OnceLock::new();
        *ASCII_FALLBACK.get_or_init(Self::use_ascii_fallback)
    }

    /// 清除屏幕（支持 ANSI 和 Windows API 双重方式）
    pub fn clear() {
        if !Self::is_safe() {
            // ANSI 转义序列 — Windows Terminal / VT 已启用
            print!("\x1B[2J\x1B[H");
        } else {
            // cmd.exe 非 UTF-8 代码页：使用 Windows 控制台 API 清屏
            #[cfg(windows)]
            unsafe {
                use windows_sys::Win32::System::Console::{
                    FillConsoleOutputAttribute, FillConsoleOutputCharacterW,
                    GetConsoleScreenBufferInfo, GetStdHandle, SetConsoleCursorPosition,
                    CONSOLE_SCREEN_BUFFER_INFO, COORD, STD_OUTPUT_HANDLE,
                };
                let handle = GetStdHandle(STD_OUTPUT_HANDLE);
                if handle.is_null() || handle as isize == -1 {
                    return;
                }
                let mut info: CONSOLE_SCREEN_BUFFER_INFO = std::mem::zeroed();
                if GetConsoleScreenBufferInfo(handle, &mut info) == 0 {
                    return;
                }
                let chars_to_write = (info.dwSize.X as u32) * (info.dwSize.Y as u32);
                let start: COORD = COORD { X: 0, Y: 0 };
                let mut written: u32 = 0;

                FillConsoleOutputCharacterW(handle, 0x20, chars_to_write, start, &mut written);
                FillConsoleOutputAttribute(
                    handle,
                    info.wAttributes,
                    chars_to_write,
                    start,
                    &mut written,
                );
                SetConsoleCursorPosition(handle, start);
            }
            #[cfg(not(windows))]
            {
                // 非 Windows 平台回退方式
                print!("\x1B[2J\x1B[H");
            }
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

    /// 设置控制台窗口标题
    pub fn set_window_title(title: &str) {
        #[cfg(windows)]
        {
            use std::ffi::OsStr;
            use std::os::windows::ffi::OsStrExt;
            let wide: Vec<u16> = OsStr::new(title)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            unsafe {
                windows_sys::Win32::System::Console::SetConsoleTitleW(wide.as_ptr());
            }
        }
        #[cfg(not(windows))]
        {
            print!("\x1B]0;{}\x07", title);
        }
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
