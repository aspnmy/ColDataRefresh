//! Windows 专有实现 —— 平台无关流程留在 `platform/mod.rs`，本文件只放 Windows 特有代码。
//!
//! 覆盖：磁盘空间（`GetDiskFreeSpaceExW`）、盘符解析（`C:`）、TRIM（`Optimize-Volume`）、
//! 管理员检测（PowerShell）、系统信息（`cmd /c ver`）。

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use crate::log::logger;

/// 判断路径是否为盘符（Windows: `C:` 或 `C:\`）
pub fn is_root_path(path: &str) -> bool {
    let p = path.trim_end_matches(&['\\', '/', ' ', '\t'][..]);
    // 匹配 "C:" 或 "C:\" 或 "C:/"
    if p.len() >= 2 && p.as_bytes()[1] == b':' {
        return p.len() == 2
            || (p.len() == 3 && (p.as_bytes()[2] == b'\\' || p.as_bytes()[2] == b'/'));
    }
    false
}

/// 解析路径中的盘符 —— 返回大写字母（如 `C`）
pub fn resolve_device_name(path: &str) -> Option<String> {
    let p = path.trim_end_matches(&['\\', '/', ' ', '\t'][..]);
    if p.len() >= 2 && p.as_bytes()[1] == b':' {
        let letter = p.chars().next()?;
        Some(letter.to_ascii_uppercase().to_string())
    } else {
        None
    }
}

/// 获取磁盘空间信息 —— 返回 (总字节, 已用字节, 可用字节)
pub fn get_disk_space(path: &Path) -> (u64, u64, u64) {
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
            (total_bytes, total_bytes.saturating_sub(free_bytes), free_bytes)
        } else {
            (0, 0, 0)
        }
    }
}

/// 检查当前进程是否以管理员权限运行
pub fn is_admin() -> bool {
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

/// 获取 Windows 构建号（取一次即缓存）
fn get_windows_build() -> u32 {
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

/// 执行 TRIM —— Windows 走 PowerShell `Optimize-Volume`
pub fn trim_volume(device: &str) -> bool {
    logger().log(&format!("开始对 {} 执行 TRIM 操作", device), "INFO");

    let build = get_windows_build();
    let is_win11 = build >= 22000;
    let is_win10 = build >= 10240;

    if !(is_win11 || is_win10) {
        logger().log("Windows 10 以下系统，不支持 PowerShell TRIM", "WARNING");
        return false;
    }

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
}

/// 通过 PowerShell `Optimize-Volume` 执行指定操作
fn run_optimize_volume(drive: &str, operation: &str) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let cmd = format!("Optimize-Volume -DriveLetter {} -{} -Verbose", drive, operation);
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

/// 获取操作系统信息 —— 按构建号显示 Windows 11 / 10
pub fn get_os_display() -> String {
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

/// 从路径反解物理盘设备 —— Windows 侧块设备直写尚未实现（issue #4 待补）
///
/// TODO(issue #4)：走 `IOCTL_STORAGE_QUERY_PROPERTY` 的
/// `STORAGE_ACCESS_ALIGNMENT_DESCRIPTOR.BytesPerPhysicalSector` 探测物理扇区，
/// 并以 `FILE_FLAG_NO_BUFFERING` 打开 `\\.\PhysicalDriveN`。
pub fn resolve_block_device(_path: &str) -> Option<String> {
    None
}

//// ===================== 块设备底层实现 (block device low-level) =====================
// Windows 侧尚未实现（issue #4 待补）。以下函数返回明确错误，⛔ 不静默降级。
// 抽象层与对外类型在 `block_dev.rs`；本文件只放落到平台 API 的那一层。

/// 设备几何探测 —— 待用 `IOCTL_STORAGE_QUERY_PROPERTY` 实现
pub fn block_geometry(device_path: &str) -> Result<(u64, u64, u64), String> {
    Err(format!(
        "Windows 侧设备几何探测尚未实现 (geometry probe not implemented): {}",
        device_path
    ))
}

/// 安全闸门 —— 待实现 PhysicalDrive 系统盘校验；当前一律拒绝（保守）
pub fn block_safety_gate(device_path: &str) -> Result<(bool, Option<String>), String> {
    Ok((
        false,
        Some(format!(
            "Windows 侧安全闸门尚未实现 (safety gate not implemented)，保守拒绝: {}",
            device_path
        )),
    ))
}

/// 直写 —— 待用 `FILE_FLAG_NO_BUFFERING` + `WriteFile` 实现
pub fn block_direct_write(
    device: &str,
    _offset: u64,
    _buf: &crate::block_dev::AlignedBuf,
    _len: u64,
) -> Result<(), String> {
    Err(format!(
        "Windows 侧块设备直写尚未实现 (direct write not implemented): {}",
        device
    ))
}

/// 直读 —— 待实现
pub fn block_direct_read(
    device: &str,
    _offset: u64,
    _buf: &mut crate::block_dev::AlignedBuf,
    _len: u64,
) -> Result<(), String> {
    Err(format!(
        "Windows 侧块设备直读尚未实现 (direct read not implemented): {}",
        device
    ))
}

/// 直 TRIM —— 待用 `DeviceIoControl` 实现
pub fn block_direct_discard(device: &str, _offset: u64, _len: u64) -> Result<(), String> {
    Err(format!(
        "Windows 侧直 TRIM 尚未实现 (direct discard not implemented): {}",
        device
    ))
}

//// ===================== 文件属性 (file attributes) =====================
// 平台专有的「文件属性操作」——原散在 `full_refresh.rs`，按 issue #7 移入分册。

/// 移除文件的只读属性 —— Windows 用 `attrib -R`（只读位会阻止删除）
pub fn clear_readonly(path: &std::path::Path) {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_READONLY: u32 = 0x0001;
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.file_attributes() & FILE_ATTRIBUTE_READONLY != 0 {
            let _ = std::process::Command::new("attrib")
                .args(["-R", &path.to_string_lossy()])
                .output();
        }
    }
}

/// 保留文件的创建时间 —— Windows 用 Win32 `SetFileTime`
pub fn preserve_creation_time(path: &std::path::Path) {
    use std::os::windows::fs::MetadataExt;
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    let ctime_raw = meta.creation_time();
    if ctime_raw > 0 {
        let _ = set_file_creation_time_win(path, ctime_raw);
    }
}

/// 用 Win32 `SetFileTime` 设置文件创建时间
fn set_file_creation_time_win(
    path: &std::path::Path,
    creation_time_raw: u64,
) -> Result<(), String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::SetFileTime;

    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|e| format!("无法打开文件 {}: {}", path.display(), e))?;

    let handle = file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
    let ft: *const windows_sys::Win32::Foundation::FILETIME =
        &creation_time_raw as *const u64 as *const windows_sys::Win32::Foundation::FILETIME;

    let result = unsafe { SetFileTime(handle, ft, std::ptr::null(), std::ptr::null()) };
    if result == 0 {
        Err(format!("设置创建时间失败: {}", path.display()))
    } else {
        Ok(())
    }
}
