// src/gui/capture_overlay.rs
//
// Floating capture guidance overlay window.
// Uses egui multi-viewport to create a true independent OS window
// that can be positioned anywhere on screen, always on top.

use eframe::egui::{
    self, Color32, Frame, Margin, Pos2, RichText, Stroke, Vec2,
};

use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
};

use super::theme::Theme;
use super::types::CaptureState;

const OVERLAY_WIDTH: f32 = 230.0;
const OVERLAY_HEIGHT: f32 = 150.0;
const MARGIN: f32 = 12.0;

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

    fn smart_position(&mut self) -> Pos2 {
        let sw = Self::screen_width();

        if !self.logged_screen {
            self.logged_screen = true;
            let mx = Self::mouse_x();
            log::info!("[overlay] screen: {}x{}, mouse_x: {:?}", sw, Self::screen_height(), mx);
        }

        let x = match Self::mouse_x() {
            Some(mx) if mx < sw / 2 => {
                sw - OVERLAY_WIDTH as i32 - MARGIN as i32
            }
            Some(__mx) => MARGIN as i32,
            None => sw - OVERLAY_WIDTH as i32 - MARGIN as i32,
        };

        Pos2::new(x as f32, MARGIN)
    }

    fn screen_width() -> i32 {
        unsafe { GetSystemMetrics(SM_CXSCREEN) }
    }

    fn screen_height() -> i32 {
        unsafe { GetSystemMetrics(SM_CYSCREEN) }
    }

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

    pub fn draw(&mut self, ctx: &egui::Context, _status_msg: &str, capture_state: &CaptureState) {
        if !self.visible || matches!(capture_state, CaptureState::Idle) {
            return;
        }

        let t = Theme::new(true);
        let pos = self.smart_position();

        let title_color = match capture_state {
            CaptureState::WaitingClick { .. } => t.warn,
            CaptureState::Capturing => t.target_fg,
            CaptureState::Idle => unreachable!(),
        };

        #[allow(deprecated)]
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("capture_overlay"),
            egui::ViewportBuilder::default()
                .with_decorations(false)
                .with_transparent(true)
                .with_always_on_top()
                .with_taskbar(false)
                .with_resizable(false)
                .with_inner_size([OVERLAY_WIDTH, OVERLAY_HEIGHT])
                .with_position([pos.x, pos.y]),
            |ctx, _queue| {
                egui::CentralPanel::default()
                    .frame(Frame::NONE
                        .fill(t.panel_fill)
                        .corner_radius(egui::CornerRadius::same(12))
                        .stroke(Stroke::new(2.0, title_color.linear_multiply(0.7)))
                        .inner_margin(Margin::same(10)))
                    .show(ctx, |ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);

                        self.draw_header(ui, t, title_color);
                        self.draw_shortcuts_section(ui, t);
                    });
            },
        );
    }

    fn draw_header(&self, ui: &mut egui::Ui, _t: Theme, color: Color32) {
        Frame::NONE
            .fill(Color32::from_rgb(25, 35, 55))
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⏳ 捕获模式").color(color).size(12.5).strong());
                    // ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    //     ui.label(RichText::new("Esc").color(t.muted).size(10.5));
                    // });
                });
            });
    }

    fn draw_shortcuts_section(&self, ui: &mut egui::Ui, t: Theme) {
        ui.add_space(8.0);
        
        let shortcuts = [
            ("Ctrl+左键点击", "确认单一元素"),
            ("Ctrl+右键点击", "添加或删除样本元素"),
            ("Ctrl+中键点击", "切换相同区域的不同元素"),
            ("Ctrl+右键双击", "完成样本选择并退出"),
        ];

        Frame::NONE
            .fill(Color32::from_rgb(20, 30, 48))
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(Margin::symmetric(6, 4))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);
                
                for (key, action) in shortcuts {
                    ui.horizontal(|ui| {
                        Frame::NONE
                            .fill(Color32::from_rgb(35, 50, 75))
                            .corner_radius(egui::CornerRadius::same(3))
                            .inner_margin(Margin::symmetric(4, 1))
                            .show(ui, |key_ui| {
                                key_ui.label(RichText::new(key)
                                    .color(t.sel_fg)
                                    .size(9.5)
                                    .strong());
                            });
                        
                        ui.add_space(6.0);
                        ui.label(RichText::new(action).color(t.text).size(10.5));
                    });
                }
            });
    }
}

impl Default for CaptureOverlay {
    fn default() -> Self {
        Self::new()
    }
}