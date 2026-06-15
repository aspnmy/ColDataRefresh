use std::path::Path;
use std::process::Command;

use crate::log::logger;

// ─── Windows 专有 ─────────────────────────────────────────────

/// 判断路径是否为盘符（Windows: C: 或 C:\；Linux: / 或 /mnt 等根目录）
pub fn is_root_path(path: &str) -> bool {
    let p = path.trim_end_matches(&['\\', '/', ' ', '\t'][..]);
    #[cfg(windows)]
    {
        // 匹配 "C:" 或 "C:\" 或 "C:/"
        if p.len() >= 2 && p.as_bytes()[1] == b':' {
            return p.len() == 2
                || (p.len() == 3 && (p.as_bytes()[2] == b'\\' || p.as_bytes()[2] == b'/'));
        }
        false
    }
    #[cfg(unix)]
    {
        // 检查是否为挂载点根（简化：路径以 / 开头且只有一级）
        p == "/" || p.matches('/').count() <= 1
    }
    #[cfg(not(any(windows, unix)))]
    {
        false
    }
}

/// 解析路径中的设备标识符
/// Windows: 返回 "C"（盘符大写字母）
/// Linux:   返回挂载点路径，如 "/mnt/data"
pub fn resolve_device_name(path: &str) -> Option<String> {
    let p = path.trim_end_matches(&['\\', '/', ' ', '\t'][..]);

    #[cfg(windows)]
    {
        if p.len() >= 2 && p.as_bytes()[1] == b':' {
            let letter = p.chars().next()?;
            Some(letter.to_ascii_uppercase().to_string())
        } else {
            None
        }
    }
    #[cfg(unix)]
    {
        if p.starts_with('/') {
            // 尝试从 /proc/mounts 查找最长前缀匹配的挂载点
            resolve_best_mount_point(p)
        } else {
            None
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        None
    }
}

/// 获取磁盘空间信息  返回 (总字节, 已用字节, 可用字节)
pub fn get_disk_space(path: &Path) -> (u64, u64, u64) {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        let p = path.to_string_lossy();
        let root = if p.len() >= 2 && p.as_bytes()[1] == b':' {
            let drive = &p[..2];
            drive.to_string() + "\\"
        } else {
            "C:\\".to_string()
        };

        let wide: Vec<u16> = std::ffi::OsStr::new(&root)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        unsafe {
            let mut free_bytes: u64 = 0;
            let mut total_bytes: u64 = 0;
            let ret = windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
                wide.as_ptr(),
                std::ptr::null_mut(),
                &mut total_bytes,
                &mut free_bytes,
            );
            if ret != 0 {
                (
                    total_bytes,
                    total_bytes.saturating_sub(free_bytes),
                    free_bytes,
                )
            } else {
                (0, 0, 0)
            }
        }
    }
    #[cfg(unix)]
    {
        unsafe {
            let p_cstr =
                std::ffi::CString::new(path.to_string_lossy().as_bytes()).unwrap_or_default();
            let mut stat: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(p_cstr.as_ptr(), &mut stat) == 0 {
                let total = (stat.f_frsize as u64).saturating_mul(stat.f_blocks as u64);
                let free = (stat.f_frsize as u64).saturating_mul(stat.f_bfree as u64);
                let used = total.saturating_sub(free);
                (total, used, free)
            } else {
                (0, 0, 0)
            }
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = path;
        (0, 0, 0)
    }
}

/// 检查当前进程是否以管理员权限运行
pub fn is_admin() -> bool {
    #[cfg(windows)]
    {
        match Command::new("powershell")
            .args(["-Command", "[bool]([Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))"])
            .output()
        {
            Ok(output) => {
                let s = String::from_utf8_lossy(&output.stdout);
                s.trim() == "True"
            }
            Err(_) => false,
        }
    }
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(any(windows, unix)))]
    {
        false
    }
}

/// 执行 TRIM 操作
/// Windows: 通过 PowerShell Optimize-Volume
/// Linux: 通过 fstrim 命令
pub fn trim_volume(device: &str) -> bool {
    logger().log(&format!("开始对 {} 执行 TRIM 操作", device), "INFO");

    #[cfg(windows)]
    {
        let is_win11 = get_windows_build() >= 22000;
        let is_win10 = get_windows_build() >= 10240;

        if is_win11 || is_win10 {
            logger().log(
                &format!(
                    "在 Windows {} 上对 {} 执行 TRIM 操作",
                    if is_win11 { "11" } else { "10" },
                    device
                ),
                "INFO",
            );

            if is_win11 {
                if !run_optimize_volume(device, "ReTrim") {
                    logger().log("ReTrim 操作失败", "WARNING");
                    return false;
                }
                let _ = run_optimize_volume(device, "SlabConsolidate");
                let _ = run_optimize_volume(device, "ReTrim");
            } else if !run_optimize_volume(device, "ReTrim") {
                logger().log("ReTrim 操作失败", "WARNING");
                return false;
            }
            true
        } else {
            logger().log("Windows 10 以下系统，不支持 PowerShell TRIM", "WARNING");
            false
        }
    }
    #[cfg(unix)]
    {
        // Linux: 使用 fstrim 命令
        match Command::new("fstrim").arg(device).output() {
            Ok(output) => {
                if output.status.success() {
                    let msg = String::from_utf8_lossy(&output.stdout);
                    logger().log(&format!("fstrim 成功: {}", msg.trim()), "INFO");
                    true
                } else {
                    let err = String::from_utf8_lossy(&output.stderr);
                    logger().log(&format!("fstrim 失败: {}", err.trim()), "WARNING");
                    false
                }
            }
            Err(e) => {
                logger().log(&format!("执行 fstrim 失败: {}", e), "ERROR");
                false
            }
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = device;
        logger().log("当前平台不支持 TRIM 操作", "WARNING");
        false
    }
}

/// 获取操作系统信息（用于日志和显示）
pub fn get_os_display() -> String {
    #[cfg(windows)]
    {
        let build = get_windows_build();
        if build >= 22000 {
            format!("Windows 11 (build {})", build)
        } else if build >= 10240 {
            format!("Windows 10 (build {})", build)
        } else if build > 0 {
            format!("Windows (build {})", build)
        } else {
            "Windows".to_string()
        }
    }
    #[cfg(unix)]
    {
        // 读取 /etc/os-release
        if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
            for line in content.lines() {
                if let Some(val) = line.strip_prefix("PRETTY_NAME=\"") {
                    return val.trim_end_matches('"').to_string();
                } else if let Some(val) = line.strip_prefix("PRETTY_NAME=") {
                    return val.to_string();
                }
            }
        }
        "Linux".to_string()
    }
    #[cfg(not(any(windows, unix)))]
    {
        "Unknown".to_string()
    }
}

// ─── Windows 内部辅助 ─────────────────────────────────────────

#[cfg(windows)]
fn get_windows_build() -> u32 {
    use std::sync::OnceLock;
    static BUILD: OnceLock<u32> = OnceLock::new();
    *BUILD.get_or_init(|| match Command::new("cmd").args(["/c", "ver"]).output() {
        Ok(output) => {
            let s = String::from_utf8_lossy(&output.stdout);
            if let Some(ver_part) = s.split('[').nth(1) {
                let ver = ver_part.trim_end_matches(']').trim();
                if let Some(build_str) = ver.rsplit('.').next() {
                    return build_str.trim().parse().unwrap_or(0);
                }
            }
            0
        }
        Err(_) => 0,
    })
}

#[cfg(windows)]
fn run_optimize_volume(drive: &str, operation: &str) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let cmd = format!(
        "Optimize-Volume -DriveLetter {} -{} -Verbose",
        drive, operation
    );
    logger().log(&format!("执行 PowerShell: {}", cmd), "INFO");

    match Command::new("powershell")
        .args(["-Command", &cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(output) => {
            if output.status.success() {
                logger().log(
                    &format!(
                        "{} 操作成功: {}",
                        operation,
                        String::from_utf8_lossy(&output.stdout).trim()
                    ),
                    "INFO",
                );
                true
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                logger().log(&format!("{} 操作失败: {}", operation, stderr), "WARNING");
                false
            }
        }
        Err(e) => {
            logger().log(&format!("执行 PowerShell 失败: {}", e), "ERROR");
            false
        }
    }
}

// ─── Linux 内部辅助 ───────────────────────────────────────────

#[cfg(unix)]
/// 从 /proc/mounts 查找路径的最长前缀挂载点
fn resolve_best_mount_point(path: &str) -> Option<String> {
    let mounts = std::fs::read_to_string("/proc/mounts").ok()?;
    let mut best: Option<String> = None;

    for line in mounts.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 2 {
            continue;
        }
        // 解码转义序列（/proc/mounts 中使用 \040 表示空格等）
        let mount_point = decode_mount_point(fields[1]);

        if path.starts_with(&mount_point) {
            let should_replace = match best.as_ref() {
                None => true,
                Some(current) => mount_point.len() > current.len(),
            };
            if should_replace {
                best = Some(mount_point);
            }
        }
    }

    best
}

#[cfg(unix)]
/// 解码 /proc/mounts 中的转义序列
fn decode_mount_point(encoded: &str) -> String {
    let mut result = String::new();
    let mut chars = encoded.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let digits: String = chars.by_ref().take(3).collect();
            if digits.len() == 3 {
                if let Ok(code) = u32::from_str_radix(&digits, 8) {
                    result.push(char::from_u32(code).unwrap_or('?'));
                    continue;
                }
            }
            result.push('\\');
            result.push_str(&digits);
        } else {
            result.push(c);
        }
    }
    result
}
