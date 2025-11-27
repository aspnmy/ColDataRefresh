use std::sync::Once;

/// 终端管理器，负责处理终端相关的操作
pub struct TerminalManager {
    /// 安全模式标志
    safe_mode: bool,
}

impl TerminalManager {
    /// 静态实例
    static mut INSTANCE: Option<TerminalManager> = None;
    static ONCE: Once = Once::new();
    
    /// 获取终端管理器实例（单例模式）
    pub fn get_instance() -> &'static mut Self {
        Self::ONCE.call_once(|| {
            let safe_mode = Self::detect_safe_mode();
            unsafe {
                Self::INSTANCE = Some(Self {
                    safe_mode,
                });
            }
        });
        unsafe {
            Self::INSTANCE.as_mut().unwrap()
        }
    }
    
    /// 检测终端是否支持特殊字符
    fn detect_safe_mode() -> bool {
        // 尝试编码特殊字符，检测终端是否支持
        if let Err(_) = "▓░║═".encode(std::io::stdout().encoding().unwrap_or(std::ffi::OsStr::new("UTF-8")), std::str::Utf8Error::REPLACEMENT_CHARACTER) {
            return true;
        }
        false
    }
    
    /// 是否处于安全模式
    pub fn safe_mode(&self) -> bool {
        self.safe_mode
    }
    
    /// 清除终端屏幕
    pub fn clear(&self) {
        if !self.safe_mode {
            print!("\x1B[2J\x1B[H");
        }
    }
    
    /// 生成带颜色的文本
    pub fn colored_text(&self, text: &str, fg: u8, bg: u8) -> String {
        if self.safe_mode {
            text.to_string()
        } else {
            format!("\x1B[{};{}m{}\x1B[0m", fg, bg, text)
        }
    }
    
    /// 设置控制台窗口标题
    pub fn set_window_title(&self, title: Option<&str>) {
        let title = title.unwrap_or("冷数据维护工具 v4.7.0");
        
        if cfg!(target_os = "windows") {
            // Windows系统：使用Windows API设置窗口标题
            // TODO: 实现Windows下的窗口标题设置
        } else {
            // Linux系统：使用ANSI转义序列
            print!("\x1B]0;{}\x07", title);
        }
    }
}
