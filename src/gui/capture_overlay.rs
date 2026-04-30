// src/capture_overlay.rs
//
// Floating capture guidance overlay window.
// Displays a semi-transparent, always-on-top panel showing capture shortcuts.

use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Stroke, Vec2,
};

/// The floating overlay panel content.
/// This is drawn in a secondary viewport that stays on top of all windows.
pub struct CaptureOverlay {
    /// Whether the overlay should be visible.
    visible: bool,
}

impl CaptureOverlay {
    pub fn new() -> Self {
        Self { visible: false }
    }

    /// Show the overlay.
    pub fn show(&mut self) {
        self.visible = true;
    }

    /// Hide the overlay.
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Check if the overlay is visible.
    #[allow(dead_code)]
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Draw the overlay panel.
    /// Returns true if the overlay is visible and should be rendered.
    pub fn draw(&self, ctx: &egui::Context) {
        if !self.visible {
            return;
        }

        // Use an immediate viewport for the overlay.
        // This creates a separate window that stays on top.
        egui::Window::new("capture_overlay")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .interactable(false)
            .fixed_size(Vec2::new(320.0, 220.0))
            .anchor(egui::Align2::LEFT_TOP, Vec2::new(20.0, 20.0))
            .frame(Frame::NONE
                .fill(Color32::from_rgba_premultiplied(20, 20, 30, 200))  // Semi-transparent dark
                .corner_radius(egui::CornerRadius::same(8))
                .stroke(Stroke::new(1.0, Color32::from_rgb(60, 60, 80)))
                .inner_margin(Margin::same(16)))
            .show(ctx, |ui| {
                // Title
                ui.horizontal(|ui| {
                    ui.label(RichText::new("正在捕获元素")
                        .color(Color32::from_rgb(255, 200, 100))
                        .size(16.0)
                        .strong());
                });
                ui.add_space(8.0);
                
                // Divider line
                let painter = ui.painter();
                let rect = ui.available_rect_before_wrap();
                painter.line_segment(
                    [egui::pos2(rect.left(), rect.top() + 2.0), egui::pos2(rect.right(), rect.top() + 2.0)],
                    Stroke::new(1.0, Color32::from_rgb(80, 80, 100)),
                );
                ui.add_space(8.0);
                
                // Instruction
                ui.label(RichText::new("将鼠标悬停在目标元素上方，按以下快捷键进行捕获：")
                    .color(Color32::from_rgb(180, 180, 200))
                    .size(12.0));
                ui.add_space(12.0);
                
                // Shortcuts table
                draw_shortcut_row(ui, "捕获单个元素", "Ctrl + 单击", "捕获单个独立控件，如按钮、输入框");
                ui.add_space(6.0);
                draw_shortcut_row(ui, "捕获相似元素组", "Shift + 单击", "批量捕获列表/表格中结构相似的元素");
                ui.add_space(6.0);
                draw_shortcut_row(ui, "退出捕获", "Esc", "随时终止捕获模式，回到正常界面");
            });
    }
}

/// Draw a single shortcut row.
fn draw_shortcut_row(ui: &mut egui::Ui, action: &str, shortcut: &str, desc: &str) {
    ui.horizontal(|ui| {
        // Action label
        ui.label(RichText::new(action)
            .color(Color32::WHITE)
            .size(13.0));
        
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Shortcut badge
            egui::Frame::NONE
                .fill(Color32::from_rgb(40, 60, 100))
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(Margin::symmetric(8, 3))
                .show(ui, |ui| {
                    ui.label(RichText::new(shortcut)
                        .color(Color32::from_rgb(200, 220, 255))
                        .size(12.0)
                        .strong());
                });
        });
    });
    
    // Description
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(RichText::new(desc)
            .color(Color32::from_rgb(120, 120, 140))
            .size(11.0));
    });
}

impl Default for CaptureOverlay {
    fn default() -> Self {
        Self::new()
    }
}