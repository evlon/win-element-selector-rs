// src/highlight.rs
//
// Draws a red highlight rectangle over the target element's bounding box.
// Windows: creates a layered, click-through WS_EX_LAYERED window.
// Other platforms: no-op stub.

use crate::model::ElementRect;
use log::error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// ─── Public API ──────────────────────────────────────────────────────────────

/// Show a highlight rectangle for `duration_ms` milliseconds, then hide it.
/// Runs the flash in a background thread so it never blocks the UI.
pub fn flash(rect: &ElementRect, duration_ms: u64) {
    #[cfg(target_os = "windows")]
    windows_impl::flash(rect, duration_ms);

    #[cfg(not(target_os = "windows"))]
    {
        debug!(
            "highlight::flash (stub) rect=({},{},{},{})",
            rect.x, rect.y, rect.width, rect.height
        );
    }
}

/// Show a persistent highlight (call hide() to remove).
pub fn show(rect: &ElementRect) -> HighlightHandle {
    #[cfg(target_os = "windows")]
    {
        windows_impl::show(rect)
    }
    #[cfg(not(target_os = "windows"))]
    {
        debug!("highlight::show (stub)");
        HighlightHandle { active: Arc::new(AtomicBool::new(true)) }
    }
}

/// Opaque handle; dropping it hides the highlight.
pub struct HighlightHandle {
    active: Arc<AtomicBool>,
}

impl Drop for HighlightHandle {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
    }
}

// ─── Windows implementation ──────────────────────────────────────────────────
#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;
    use std::thread;
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
            Graphics::Gdi::{
                BeginPaint, CreatePen, DeleteObject, EndPaint,
                GetStockObject, PAINTSTRUCT, PS_SOLID, SelectObject,
                NULL_BRUSH, Rectangle,
            },
            UI::{
                WindowsAndMessaging::{
                    CreateWindowExW, DefWindowProcW, DestroyWindow,
                    PostQuitMessage, RegisterClassExW,
                    SetLayeredWindowAttributes, SetWindowPos, ShowWindow,
                    HWND_TOPMOST, LWA_ALPHA,
                    SWP_NOMOVE, SWP_NOSIZE, SW_SHOW,
                    WM_DESTROY, WM_PAINT, WNDCLASSEXW,
                    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOPMOST,
                    WS_EX_TRANSPARENT, WS_POPUP, GetClientRect,
                },
            },
        },
    };

    const BORDER: i32 = 3;
    const CLASS_NAME: &str = "ElemSelectorHighlight\0";

    pub fn flash(rect: &ElementRect, duration_ms: u64) {
        let r = rect.clone();
        thread::spawn(move || {
            if let Some(hwnd) = create_highlight_window(&r) {
                unsafe { let _ = ShowWindow(hwnd, SW_SHOW); };
                thread::sleep(std::time::Duration::from_millis(duration_ms));
                unsafe { let _ = DestroyWindow(hwnd); };
            }
        });
    }

    pub fn show(rect: &ElementRect) -> HighlightHandle {
        let active = Arc::new(AtomicBool::new(true));
        let active2 = active.clone();
        let r = rect.clone();

        thread::spawn(move || {
            if let Some(hwnd) = create_highlight_window(&r) {
                unsafe { let _ = ShowWindow(hwnd, SW_SHOW); };
                // Simple message pump - wait until handle is dropped
                while active2.load(Ordering::SeqCst) {
                    thread::sleep(std::time::Duration::from_millis(30));
                }
                unsafe { let _ = DestroyWindow(hwnd); };
            }
        });

        HighlightHandle { active }
    }

    fn create_highlight_window(rect: &ElementRect) -> Option<HWND> {
        let class: Vec<u16> = CLASS_NAME.encode_utf16().collect();

        unsafe {
            // Register class (idempotent — ignore ERROR_CLASS_ALREADY_EXISTS).
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(highlight_wnd_proc),
                lpszClassName: PCWSTR(class.as_ptr()),
                ..Default::default()
            };
            let _ = RegisterClassExW(&wc); // ignore failure (already registered)

            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
                PCWSTR(class.as_ptr()),
                PCWSTR::null(),
                WS_POPUP,
                rect.x - BORDER,
                rect.y - BORDER,
                rect.width  + BORDER * 2,
                rect.height + BORDER * 2,
                None, None, None, None,
            );

            match hwnd {
                Ok(h) => {
                    // 60% opacity red overlay.
                    SetLayeredWindowAttributes(h, COLORREF(0), 153, LWA_ALPHA).ok()?;
                    SetWindowPos(
                        h, HWND_TOPMOST, 0, 0, 0, 0,
                        SWP_NOMOVE | SWP_NOSIZE,
                    ).ok()?;
                    Some(h)
                }
                Err(e) => {
                    error!("CreateWindowEx highlight failed: {e}");
                    None
                }
            }
        }
    }

    unsafe extern "system" fn highlight_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);

                let pen   = CreatePen(PS_SOLID, BORDER, COLORREF(0x0000FF)); // red BGR
                let brush = GetStockObject(NULL_BRUSH);
                let old_pen   = SelectObject(hdc, pen);
                let old_brush = SelectObject(hdc, brush);

                let mut rc = RECT::default();
                GetClientRect(hwnd, &mut rc).ok();
                Rectangle(
                    hdc, rc.left, rc.top, rc.right, rc.bottom,
                );

                SelectObject(hdc, old_pen);
                SelectObject(hdc, old_brush);
                DeleteObject(pen);
                EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
