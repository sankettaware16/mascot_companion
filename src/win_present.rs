//! Windows presenter. DWM refuses per-pixel-alpha GPU swapchains for normal
//! windows, so on Windows the pet is shown the way classic desktop pets do it:
//! a small layered window whose content + alpha + position are pushed in one
//! call via `UpdateLayeredWindow`. No GPU, no fullscreen overlay — the window
//! is exactly the size of the pet (plus banner) and carries true transparency.

use windows_sys::Win32::Foundation::{POINT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION,
    DIB_RGB_COLORS,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, UpdateLayeredWindow, GWL_EXSTYLE, ULW_ALPHA,
    WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

type Hwnd = *mut core::ffi::c_void;

pub struct Layered {
    hwnd: Hwnd,
}

impl Layered {
    /// Wrap a winit window's HWND, adding the layered/click-through styles.
    pub fn new(hwnd: Hwnd) -> Self {
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(
                hwnd,
                GWL_EXSTYLE,
                ex | (WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW) as isize,
            );
        }
        Self { hwnd }
    }

    /// Push a premultiplied-BGRA frame to the screen at virtual-desktop
    /// coordinates (x, y). Sets content, size and position atomically.
    pub fn present(&self, bgra: &[u8], w: i32, h: i32, x: i32, y: i32) -> bool {
        if w <= 0 || h <= 0 || bgra.len() < (w * h * 4) as usize {
            return false;
        }
        unsafe {
            let screen = GetDC(std::ptr::null_mut());
            if screen.is_null() {
                return false;
            }
            let mem = CreateCompatibleDC(screen);

            let mut bi: BITMAPINFO = std::mem::zeroed();
            bi.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // negative = top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..std::mem::zeroed()
            };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let hbmp =
                CreateDIBSection(screen, &bi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
            if hbmp.is_null() || bits.is_null() {
                DeleteDC(mem);
                ReleaseDC(std::ptr::null_mut(), screen);
                return false;
            }
            std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, (w * h * 4) as usize);

            let old = SelectObject(mem, hbmp);
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let dst = POINT { x, y };
            let size = SIZE { cx: w, cy: h };
            let src = POINT { x: 0, y: 0 };
            let ok = UpdateLayeredWindow(
                self.hwnd,
                screen,
                &dst,
                &size,
                mem,
                &src,
                0,
                &blend,
                ULW_ALPHA,
            );

            SelectObject(mem, old);
            DeleteObject(hbmp);
            DeleteDC(mem);
            ReleaseDC(std::ptr::null_mut(), screen);
            ok != 0
        }
    }
}
