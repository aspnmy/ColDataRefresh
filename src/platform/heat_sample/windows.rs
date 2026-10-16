//! Windows 分册 —— ETW `Microsoft-Windows-Kernel-Disk` 实时会话
//!
//! 等价物：Linux 的 block tracepoint（`block_rq_issue`）。
//! Windows 无 tracefs，唯一能拿到**每个 LBA 被访问几次**的机制是 ETW
//! ⇒ 热力图在 Windows 上必须走 ETW，⛔ 不能以「该平台不支持」收口。
//!
//! ## 实现链路
//!
//! 1. `StartTraceW`  —— 建实时会话
//! 2. `EnableTraceEx2` —— 启用 `Microsoft-Windows-Kernel-Disk` provider + 关键字
//! 3. `OpenTraceW` + `ProcessTrace` —— 消费事件（回调里解析 payload）
//! 4. `ControlTraceW(STOP)` + `CloseTrace` —— 收尾
//!
//! ## 事件 payload（`DiskIo_TypeGroup1`，事件 ID 10=Read / 11=Write）
//!
//! ```text
//! typedef struct _EVENT_TRACE_HEADER { ... }  // ETW 头（已由 ETW 解析）
//! ULONG64 ByteOffset;    // 起始偏移（字节）= LBA × 512
//! ULONG   TransferSize;  // 传输字节数
//! ...
//! ```
//!
//! ## 权限
//!
//! 内核 provider 需**管理员**（`SeSystemProfilePrivilege`）；
//! 非管理员时 ETW 会失败 ⇒ 走明确报错路径（C 档要求）。

use super::{RawIo, SampleOutput};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::log::logger;

/// 扇区大小（ETW 的 `ByteOffset` 是字节偏移，须换算成扇区）
///
/// ⚠️ 事件解析（`ProcessTrace` 回调）尚未接入 —— 该常量属预留，
/// 待回调实现时启用；先加 `allow` 以免非 Windows 目标报 dead_code。
#[allow(dead_code)]
const SECTOR_SIZE: u64 = 512;

/// ETW provider GUID：`Microsoft-Windows-Kernel-Disk`
/// {C7B624EC-C0CD-4C8B-AE62-3A2C3B8C0E6E}
const KERNEL_DISK_PROVIDER: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0xC7B6_24EC,
    data2: 0xC0CD,
    data3: 0x4C8B,
    data4: [0xAE, 0x62, 0x3A, 0x2C, 0x3B, 0x8C, 0x0E, 0x6E],
};

/// 采样：建 ETW 实时会话 → 跑 probe → 停会话 → 汇总事件
pub fn sample_io<F>(device: &str, probe: F) -> Result<SampleOutput, String>
where
    F: FnOnce() -> Result<(), String>,
{
    // 权限闸门：内核 provider 需管理员
    if !crate::platform::is_admin() {
        return Err(
            "ETW 内核 provider 需要管理员权限 (requires administrator for ETW kernel provider)；\
             请以管理员身份运行，否则无法采集 LBA 热度"
                .to_string(),
        );
    }

    let (device_sectors, partition_start_sectors) = device_geometry(device)?;

    let events = Arc::new(Mutex::new(Vec::<RawIo>::new()));
    let stop = Arc::new(AtomicBool::new(false));

    logger().log("启动 ETW 实时会话 (starting ETW realtime session)", "INFO");

    // 1) 建会话
    let session_name = to_wide("coldatafresh-lba-heat\0");
    let mut props = build_session_properties(&session_name);
    let mut session_handle = windows_sys::Win32::System::Diagnostics::Etw::CONTROLTRACE_HANDLE {
        Value: 0,
    };
    // SAFETY: props 已按 StartTraceW 契约填好（wnode / logger name / buffer 大小）；
    //         session_name 以 NUL 结尾的 UTF-16，生命周期覆盖本次调用
    let rc = unsafe {
        windows_sys::Win32::System::Diagnostics::Etw::StartTraceW(
            &mut session_handle,
            session_name.as_ptr(),
            props.as_mut_ptr() as *mut windows_sys::Win32::System::Diagnostics::Etw::EVENT_TRACE_PROPERTIES,
        )
    };
    if rc != 0 {
        return Err(format!(
            "StartTraceW 失败 (rc={})：无法建 ETW 会话，可能权限不足或同名会话已存在",
            rc
        ));
    }

    // 2) 启用 Microsoft-Windows-Kernel-Disk provider
    let rc = unsafe {
        windows_sys::Win32::System::Diagnostics::Etw::EnableTraceEx2(
            session_handle,
            &KERNEL_DISK_PROVIDER,
            windows_sys::Win32::System::Diagnostics::Etw::EVENT_CONTROL_CODE_ENABLE_PROVIDER,
            windows_sys::Win32::System::Diagnostics::Etw::TRACE_LEVEL_INFORMATION as u8,
            0,
            0,
            0,
            std::ptr::null(),
        )
    };
    if rc != 0 {
        // 清理会话再报错
        stop_session(session_handle);
        return Err(format!(
            "EnableTraceEx2 失败 (rc={})：无法启用 Kernel-Disk provider",
            rc
        ));
    }

    // 3) 打开实时会话并消费事件
    //
    // ⚠️ `ProcessTrace` 是**阻塞式**：它会一直取事件直到会话停止。
    // 所以形态必须是「先在独立线程里跑 ProcessTrace → 主线程跑 probe → 停会话
    // → ProcessTrace 因会话停止而返回」；⛔ 不能先跑 probe 再 ProcessTrace
    // （那样 probe 期间的缓冲要么丢失、要么在停会话时被丢弃）。
    let events_handle = Arc::clone(&events);
    let consumer = std::thread::spawn(move || consume_events(events_handle));

    // probe 期间 ETW 在另一线程里持续收事件
    let probe_result = probe();

    // 停会话 ⇒ ProcessTrace 自然返回
    stop_session(session_handle);
    let consumed = consumer.join().unwrap_or(0);

    logger().log(
        &format!("ETW 事件消费结束，共 {} 笔 (events consumed)", consumed),
        "INFO",
    );

    let collected = events.lock().map(|v| v.clone()).unwrap_or_default();
    let _ = stop.store(true, Ordering::Relaxed);

    probe_result?;

    // ⚠️ 采到 0 笔不再静默放过：明确告知"已连上 provider 但没拿到数据"，
    // 便于真机排查（磁盘为空闲 / GUID 或关键字不匹配 / 事件级别过滤过严）。
    if collected.is_empty() {
        return Err(
            "ETW 已建立会话但未采到任何磁盘 IO 事件 (ETW session up but no disk IO events)；\
             可能原因：采样期间无 IO / provider 关键字不匹配 / 事件级别过滤过严"
                .to_string(),
        );
    }

    Ok(SampleOutput {
        events: collected,
        device_sectors,
        partition_start_sectors,
        backend: "etw(kernel-disk)".to_string(),
    })
}

/// 打开实时会话并消费事件（**阻塞**，须在独立线程运行）
///
/// 返回消费到的事件数。
fn consume_events(events: Arc<Mutex<Vec<RawIo>>>) -> u64 {
    use windows_sys::Win32::System::Diagnostics::Etw::{
        CloseTrace, OpenTraceW, ProcessTrace, EVENT_TRACE_LOGFILEW,
    };

    let session_name = to_wide("coldatafresh-lba-heat\0");

    // SAFETY: EVENT_TRACE_LOGFILEW 全零 + 只填必要字段；
    //         LoggerName 指向的 UTF-16 缓冲在本函数存活期内有效
    let mut logfile: EVENT_TRACE_LOGFILEW = unsafe { std::mem::zeroed() };
    logfile.LoggerName = session_name.as_ptr() as *mut u16;
    // ProcessTraceMode 位于 Anonymous1 联合体内（与 LogFileMode 共用存储）
    logfile.Anonymous1.ProcessTraceMode =
        windows_sys::Win32::System::Diagnostics::Etw::PROCESS_TRACE_MODE_REAL_TIME
            | windows_sys::Win32::System::Diagnostics::Etw::PROCESS_TRACE_MODE_EVENT_RECORD;
    // 上下文 = 事件缓冲的裸指针（回调经 UserContext 取回）
    logfile.Context = Arc::as_ptr(&events) as *mut std::ffi::c_void;

    // 回调签名（windows-sys 0.59）：`Option<unsafe extern "system" fn(*mut EVENT_RECORD)>`
    logfile.Anonymous2.EventRecordCallback = Some(event_record_callback);

    // SAFETY: 传入的 LoggerName 已 NUL 结尾；返回的 handle 交由 ProcessTrace/CloseTrace
    let handle = unsafe { OpenTraceW(&mut logfile) };
    // INVALID_PROCESSTRACE_HANDLE = 0xFFFFFFFFFFFFFFFF（以 u64 表示）
    if handle.Value == u64::MAX {
        logger().log("OpenTraceW 失败：ETW 句柄无效", "WARNING");
        return 0;
    }

    // SAFETY: handle 来自 OpenTraceW；ProcessTrace 阻塞直到会话停止
    unsafe {
        ProcessTrace(&handle, 1, std::ptr::null(), std::ptr::null());
        CloseTrace(handle);
    }

    events.lock().map(|v| v.len() as u64).unwrap_or(0)
}

/// ETW 事件回调 —— 从 `EVENT_RECORD` 的 UserData 解析 `DiskIo_TypeGroup1`
///
/// payload 布局（`Microsoft-Windows-Kernel-Disk`，事件 ID 10=Read / 11=Write）：
/// ```text
/// offset 0  : ULONG64 ByteOffset    // 起始字节偏移（= LBA × 512）
/// offset 8  : ULONG   TransferSize  // 传输字节数
/// ```
/// ⚠️ 只取前 12 字节（偏移 + 长度）—— 这是热力图唯一需要的两项；
/// 不同 Windows 版本该结构尾部字段可能增减，**头部两项稳定**。
///
/// ⚠️ `windows-sys 0.59` 的 `PEVENT_RECORD_CALLBACK` 签名**无返回值**
/// （`Option<unsafe extern "system" fn(*mut EVENT_RECORD)>`）——
/// 消费在 `ProcessTrace` 返回后结束，回调自身不控制继续/停止。
unsafe extern "system" fn event_record_callback(
    record: *mut windows_sys::Win32::System::Diagnostics::Etw::EVENT_RECORD,
) {
    use windows_sys::Win32::System::Diagnostics::Etw::EVENT_RECORD;

    if record.is_null() {
        return;
    }
    // SAFETY: record 由 ETW 保证有效（回调契约）
    let rec = unsafe { &*(record as *const EVENT_RECORD) };

    // 上下文 = Arc<Mutex<Vec<RawIo>>> 的裸指针
    let ctx = rec.UserContext;
    if ctx.is_null() {
        return;
    }
    // SAFETY: 上下文由 consume_events 传入且在其存活期内有效
    let events = unsafe { &*(ctx as *const Mutex<Vec<RawIo>>) };

    // payload 至少要有 offset(8) + size(4)
    let size = rec.UserDataLength as usize;
    if rec.UserData.is_null() || size < 12 {
        return;
    }

    // SAFETY: UserData 有效长度 = UserDataLength
    let bytes = unsafe { std::slice::from_raw_parts(rec.UserData as *const u8, size) };

    let byte_offset = u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]);
    let transfer_size = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as u64;

    if transfer_size == 0 {
        return;
    }

    // 事件 ID：10=Read / 11=Write（provider 定义）
    let event_id = rec.EventHeader.EventDescriptor.Id;
    let is_write = event_id == 11;

    let io = RawIo {
        sector: byte_offset / SECTOR_SIZE,
        sectors: transfer_size / SECTOR_SIZE,
        is_write,
    };

    if let Ok(mut guard) = events.lock() {
        guard.push(io);
    }
}

/// 停掉并释放 ETW 会话
fn stop_session(handle: windows_sys::Win32::System::Diagnostics::Etw::CONTROLTRACE_HANDLE) {
    // SAFETY: handle 来自 StartTraceW；properties 与 instancename 传 null 表示按句柄停
    unsafe {
        windows_sys::Win32::System::Diagnostics::Etw::ControlTraceW(
            handle,
            std::ptr::null(),
            std::ptr::null_mut(),
            windows_sys::Win32::System::Diagnostics::Etw::EVENT_TRACE_CONTROL_STOP,
        );
    }
}

/// 设备几何：整盘扇区数 + 本设备起始扇区
///
/// ⚠️ Windows 侧**尚无**块设备反解（`resolve_block_device` 恒返 None）⇒
/// 几何信息通过 `IOCTL_DISK_GET_LENGTH_INFO` / `IOCTL_DISK_GET_DRIVE_GEOMETRY_EX` 取；
/// 当前先返占位（容量未知时按 0 处理，分桶逻辑仍可用「有访问的桶」部分）。
pub fn device_geometry(device: &str) -> Result<(u64, u64), String> {
    // TODO(issue #12)：接 IOCTL_DISK_GET_DRIVE_GEOMETRY_EX 拿真实容量
    // 现阶段返回 0 会让"从未访问桶"统计失真，但**不影响有访问桶的热力分布**
    let _ = device;
    Ok((0, 0))
}

/// 构造 `EVENT_TRACE_PROPERTIES`（含 WNODE 头 + logger 名尾随缓冲）
///
/// 返回**堆上字节缓冲**：`EVENT_TRACE_PROPERTIES` 的 logger 名尾随在结构体之后，
/// 按 Win32 契约必须整体分配（`Wnode.BufferSize` = 结构体 + 名字字节数）。
fn build_session_properties(session_name: &[u16]) -> Vec<u8> {
    use windows_sys::Win32::System::Diagnostics::Etw::{
        EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE, WNODE_FLAG_TRACED_GUID,
    };

    let name_bytes = session_name.len() * 2;
    let size = std::mem::size_of::<EVENT_TRACE_PROPERTIES>() + name_bytes;
    let mut buf = vec![0u8; size];

    // SAFETY: buf 按 EVENT_TRACE_PROPERTIES 布局分配；只写头字段，
    //         名字区由下面的 copy_nonoverlapping 填充
    unsafe {
        let p = buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES;
        (*p).Wnode.BufferSize = size as u32;
        (*p).Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        (*p).LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        (*p).BufferSize = 64; // 64 KB
        (*p).MinimumBuffers = 4;
        (*p).MaximumBuffers = 16;

        // logger 名紧跟结构体尾部（Win32 约定，无内联数组字段）
        let name_dst = buf.as_mut_ptr().add(std::mem::size_of::<EVENT_TRACE_PROPERTIES>()) as *mut u16;
        std::ptr::copy_nonoverlapping(session_name.as_ptr(), name_dst, session_name.len());
    }

    buf
}

/// UTF-8 → UTF-16（含结尾 NUL）
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}
