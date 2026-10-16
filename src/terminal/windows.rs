//! Windows 控制台专有实现 —— 平台无关逻辑在 `terminal/mod.rs`。
//!
//! 覆盖：UTF-8 代码页检测（决定是否回退 ASCII）、Win32 Console API 清屏、窗口标题设置。

/// 检测终端是否支持 UTF-8/ANSI（否则回退到纯 ASCII）
pub fn use_ascii_fallback() -> bool {
    match std::env::var("WT_SESSION") {
        Ok(_) => false, // Windows Terminal — 支持 UTF-8
        Err(_) => {
            let cp = unsafe { windows_sys::Win32::System::Console::GetConsoleOutputCP() };
            cp != 65001 // 非 UTF-8 代码页 → 回退 ASCII
        }
    }
}

/// 清屏（ASCII 回退路径）—— cmd.exe 非 UTF-8 代码页：使用 Win32 控制台 API
pub fn clear_ascii() {
    unsafe {
        use windows_sys::Win32::System::Console::{
            FillConsoleOutputAttribute, FillConsoleOutputCharacterW, GetConsoleScreenBufferInfo,
            GetStdHandle, SetConsoleCursorPosition, CONSOLE_SCREEN_BUFFER_INFO, COORD,
            STD_OUTPUT_HANDLE,
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
        FillConsoleOutputAttribute(handle, info.wAttributes, chars_to_write, start, &mut written);
        SetConsoleCursorPosition(handle, start);
    }
}

/// 清屏（ANSI 路径）—— Windows Terminal / 已启用 VT
pub fn clear_ansi() {
    print!("\x1B[2J\x1B[H");
}

/// 设置控制台窗口标题 —— Win32 `SetConsoleTitleW`
pub fn set_window_title(title: &str) {
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
