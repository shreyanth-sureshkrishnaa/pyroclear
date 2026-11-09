// win.rs — hand-rolled Win32 console bindings (Windows only, zero deps).
// The few pieces of the Windows console pyroclear needs: raw mode,
// terminal size, and Ctrl+C. Everything else is plain ANSI output, which
// works because virtual-terminal (VT) processing is enabled here.

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::io;

const STD_INPUT_HANDLE: u32 = (-10i32) as u32;
const STD_OUTPUT_HANDLE: u32 = (-11i32) as u32;

const ENABLE_PROCESSED_INPUT: u32 = 0x0001;
const ENABLE_LINE_INPUT: u32 = 0x0002;
const ENABLE_ECHO_INPUT: u32 = 0x0004;
const ENABLE_VIRTUAL_TERMINAL_INPUT: u32 = 0x0200;
const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;

#[repr(C)]
struct Coord {
    x: i16,
    y: i16,
}

#[repr(C)]
struct SmallRect {
    left: i16,
    top: i16,
    right: i16,
    bottom: i16,
}

#[repr(C)]
struct ConsoleScreenBufferInfo {
    dw_size: Coord,
    dw_cursor_position: Coord,
    w_attributes: u16,
    sr_window: SmallRect,
    dw_maximum_window_size: Coord,
}

/// KEY_EVENT_RECORD; the other INPUT_RECORD variants fit in the same 16 bytes.
#[repr(C)]
struct KeyEventRecord {
    key_down: i32,
    repeat_count: u16,
    virtual_key_code: u16,
    virtual_scan_code: u16,
    unicode_char: u16,
    control_key_state: u32,
}

/// INPUT_RECORD: an event type tag, then the event union (at offset 4).
#[repr(C)]
struct InputRecord {
    event_type: u16,
    event: KeyEventRecord,
}

const KEY_EVENT: u16 = 0x0001;
const WAIT_OBJECT_0: u32 = 0;

extern "system" {
    fn GetStdHandle(n_std_handle: u32) -> *mut c_void;
    fn WaitForSingleObject(h_handle: *mut c_void, dw_milliseconds: u32) -> u32;
    fn PeekConsoleInputW(h: *mut c_void, buf: *mut InputRecord, len: u32, read: *mut u32) -> i32;
    fn ReadConsoleInputW(h: *mut c_void, buf: *mut InputRecord, len: u32, read: *mut u32) -> i32;
    fn GetConsoleMode(h_console: *mut c_void, lp_mode: *mut u32) -> i32;
    fn SetConsoleMode(h_console: *mut c_void, dw_mode: u32) -> i32;
    fn GetConsoleScreenBufferInfo(
        h_console: *mut c_void,
        lp_info: *mut ConsoleScreenBufferInfo,
    ) -> i32;
    pub fn SetConsoleCtrlHandler(handler: unsafe extern "system" fn(u32) -> i32, add: i32) -> i32;
}

// ── Raw mode ──────────────────────────────────────────────────────────

pub struct RawConsole {
    in_mode: u32,
    out_mode: u32,
    out_saved: bool,
}

pub fn enter_raw() -> io::Result<RawConsole> {
    unsafe {
        let hin = GetStdHandle(STD_INPUT_HANDLE);
        let hout = GetStdHandle(STD_OUTPUT_HANDLE);

        let mut in_mode = 0u32;
        if GetConsoleMode(hin, &mut in_mode) == 0 {
            return Err(io::Error::last_os_error());
        }
        let raw = in_mode & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT)
            | ENABLE_VIRTUAL_TERMINAL_INPUT;
        if SetConsoleMode(hin, raw) == 0 {
            return Err(io::Error::last_os_error());
        }

        // Let conhost interpret ANSI escapes; Windows Terminal does anyway.
        let mut out_mode = 0u32;
        let out_saved = GetConsoleMode(hout, &mut out_mode) != 0;
        if out_saved {
            SetConsoleMode(hout, out_mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        }

        Ok(RawConsole {
            in_mode,
            out_mode,
            out_saved,
        })
    }
}

pub fn leave_raw(saved: &RawConsole) {
    unsafe {
        SetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), saved.in_mode);
        if saved.out_saved {
            SetConsoleMode(GetStdHandle(STD_OUTPUT_HANDLE), saved.out_mode);
        }
    }
}

// ── Key polling ───────────────────────────────────────────────────────

/// Waits up to `timeout` for a key press that `stdin().read()` will return.
///
/// The console input handle is signaled by *any* input event: key releases
/// (such as the Enter release left over from launching the command), mouse,
/// focus and resize events. `read()` skips those and blocks until a real key
/// press, which froze the TUI before its first frame. So non-key-press events
/// are drained here, and only a key press with a character counts.
pub fn wait_for_key(timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    unsafe {
        let hin = GetStdHandle(STD_INPUT_HANDLE);
        if hin.is_null() {
            return false;
        }
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if WaitForSingleObject(hin, left.as_millis() as u32) != WAIT_OBJECT_0 {
                return false; // timed out (or failed)
            }
            let mut rec: InputRecord = std::mem::zeroed();
            let mut n = 0u32;
            if PeekConsoleInputW(hin, &mut rec, 1, &mut n) == 0 || n == 0 {
                return false;
            }
            // With VT input on, arrows and other keys also arrive as characters.
            if rec.event_type == KEY_EVENT && rec.event.key_down != 0 && rec.event.unicode_char != 0 {
                return true;
            }
            ReadConsoleInputW(hin, &mut rec, 1, &mut n); // drop the event, keep waiting
            if left.is_zero() {
                return false;
            }
        }
    }
}

// ── Terminal size ─────────────────────────────────────────────────────

pub fn terminal_size() -> Option<(usize, usize)> {
    unsafe {
        let h = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut info: ConsoleScreenBufferInfo = std::mem::zeroed();
        if GetConsoleScreenBufferInfo(h, &mut info) == 0 {
            return None;
        }
        let w = info.sr_window.right - info.sr_window.left + 1;
        let h_ = info.sr_window.bottom - info.sr_window.top + 1;
        if w > 0 && h_ > 0 {
            Some((w as usize, h_ as usize))
        } else {
            None
        }
    }
}
