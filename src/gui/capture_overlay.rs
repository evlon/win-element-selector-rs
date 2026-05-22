// src/gui/capture_overlay.rs
//
// Floating capture guidance overlay window.
// Uses egui multi-viewport to create a true independent OS window
// that can be moved anywhere on screen, always on top, outside main window bounds.
//
// Positioning: always on the primary monitor, top corner.
// - mouse.x < 50% screen width  → overlay at top-right
// - mouse.x >= 50% screen width → overlay at top-left
// - mouse unknown (hook swallows) → default top-right

use eframe::egui::{
    self, Color32, Frame, Margin, Pos2, RichText, Stroke,
};

use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
};

use super::types::CaptureState;

const OVERLAY_SIZE: [f32; 2] = [360.0, 200.0];
const MARGIN: f32 = 12.0;

/// The floating overlay panel content.
///
/// Positioning rules (relative to mouse's current screen):
/// - mouse.x < 50% screen width  → overlay at top-right
/// - mouse.x >= 50% screen width → overlay at top-left
/// - mouse unknown (hook swallows) → default top-right
pub struct CaptureOverlay {
    visible: bool,
    logged_screen: bool,
}

impl CaptureOverlay {
    pub fn new() -> Self {
        Self { visible: false, logged_screen: false }
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.logged_screen = false;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    #[allow(dead_code)]
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Position overlay in a top corner based on mouse X relative to 50% of screen width.
    /// Uses physical screen dimensions (GetSystemMetrics) and global cursor position (GetCursorPos).
    /// - mouse.x < sw/2 → top-right corner
    /// - mouse.x >= sw/2 → top-left corner
    /// - mouse unknown → top-right (default)
    fn smart_position(&mut self) -> Pos2 {
        let sw = Self::screen_width();
        let sh = Self::screen_height();

        // Log screen info once per capture session
        if !self.logged_screen {
            self.logged_screen = true;
            let mx = Self::mouse_x();
            log::info!("[overlay] screen: {}x{}, mouse_x: {:?}", sw, sh, mx);
        }

        let x = match Self::mouse_x() {
            Some(mx) if mx < sw / 2 => {
                log::info!("[overlay] -> RIGHT (mx={} < {})", mx, sw / 2);
                sw - OVERLAY_SIZE[0] as i32 - MARGIN as i32
            }
            Some(mx) => {
                log::info!("[overlay] -> LEFT (mx={} >= {})", mx, sw / 2);
                MARGIN as i32
            }
            None => {
                sw - OVERLAY_SIZE[0] as i32 - MARGIN as i32
            }
        };

        Pos2::new(x as f32, MARGIN)
    }

    fn screen_width() -> i32 {
        unsafe { GetSystemMetrics(SM_CXSCREEN) }
    }

    fn screen_height() -> i32 {
        unsafe { GetSystemMetrics(SM_CYSCREEN) }
    }

    /// Get global cursor X position in physical screen coordinates.
    fn mouse_x() -> Option<i32> {
        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_ok() {
                Some(pt.x)
            } else {
                None
            }
        }
    }

    pub fn draw(&mut self, ctx: &egui::Context, status_msg: &str, capture_state: &CaptureState) {
        if !self.visible {
            return;
        }

        // 不显示 Idle 状态的悬浮窗口
        if matches!(capture_state, CaptureState::Idle) {
            return;
        }

        let (title_color, icon, show_shortcuts) = match capture_state {
            CaptureState::WaitingClick { .. } => {
                (Color32::from_rgb(255, 200, 100), "⚡", true)
            }
            CaptureState::Capturing => {
                (Color32::from_rgb(100, 180, 255), "⟳", false)
            }
            CaptureState::Idle => {
                unreachable!()
            }
        };

        let pos = self.smart_position();

        #[allow(deprecated)]
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("capture_overlay"),
            egui::ViewportBuilder::default()
                .with_decorations(false)
                .with_transparent(true)
                .with_always_on_top()
                .with_taskbar(false)
                .with_resizable(false)
                .with_inner_size(OVERLAY_SIZE)
                .with_position([pos.x, pos.y]),
            |ctx, _queue| {
                egui::CentralPanel::default()
                    .frame(Frame::NONE
                        .fill(Color32::from_rgba_premultiplied(18, 18, 24, 245))
                        .corner_radius(egui::CornerRadius::same(10))
                        .stroke(Stroke::new(1.0, title_color.linear_multiply(0.6)))
                        .inner_margin(Margin::same(12)))
                    .show(ctx, |ui| {
                        ui.spacing_mut().item_spacing = [8.0, 6.0].into();

                        // Header
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icon).size(15.0).color(title_color));
                            ui.add_space(4.0);
                            ui.label(RichText::new(status_msg)
                                .color(Color32::from_rgb(210, 215, 225))
                                .size(12.5)
                                .strong());
                        });

                        if show_shortcuts {
                            ui.label(RichText::new("快捷键")
                                .color(Color32::from_rgb(130, 140, 160))
                                .size(10.0)
                                .weak());

                            draw_shortcut_row(ui, "确认捕获");
                            draw_shortcut_row(ui, "添加/移除样本");
                            draw_shortcut_row(ui, "切换元素");
                            draw_shortcut_row(ui, "退出捕获");
                            draw_shortcut_row(ui, "取消捕获");
                        }
                    });
            },
        );
    }
}

fn draw_shortcut_row(ui: &mut egui::Ui, action: &str) {
    ui.label(RichText::new(action)
        .color(Color32::from_rgb(170, 175, 190))
        .size(11.0));
}

impl Default for CaptureOverlay {
    fn default() -> Self {
        Self::new()
    }
}