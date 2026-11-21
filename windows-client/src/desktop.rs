//! Virtual-desktop geometry and absolute cursor placement.
//!
//! The whole process is per-monitor DPI aware, so every value here is in
//! physical pixels and lines up with what `SetCursorPos` expects.

use windows_sys::Win32::{
    Foundation::GetLastError,
    UI::{
        HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
        WindowsAndMessaging::{
            GetSystemMetrics, SetCursorPos, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
            SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
        },
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualDesktop {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl std::fmt::Display for VirtualDesktop {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}x{}+{}+{}",
            self.width, self.height, self.x, self.y
        )
    }
}

pub fn enable_dpi_awareness() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

pub fn virtual_desktop() -> VirtualDesktop {
    unsafe {
        VirtualDesktop {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1),
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1),
        }
    }
}

pub fn move_cursor_to(x: i32, y: i32) -> std::result::Result<(), u32> {
    let ok = unsafe { SetCursorPos(x, y) };
    if ok == 0 {
        return Err(unsafe { GetLastError() });
    }
    Ok(())
}
