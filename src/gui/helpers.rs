// src/gui/helpers.rs
//
// 独立的工具函数

use eframe::egui::{self, Color32, Frame, Margin, RichText, Stroke, Ui, Vec2};

use super::theme::Theme;
use element_selector::core::model::HierarchyNode;

pub fn panel_header(ui: &mut Ui, title: &str, t: Theme) {
    let w = ui.available_width();
    Frame::NONE
        .fill(t.panel_hdr)
        .inner_margin(Margin::symmetric(8, 3))
        .stroke(Stroke::new(0.5, t.border))
        .show(ui, |ui| {
            ui.set_max_width(w);
            ui.label(RichText::new(title).color(t.hdr_text).size(11.5).strong());
        });
    ui.add_space(2.0);
}

pub fn col_label(ui: &mut Ui, text: &str, width: f32, t: Theme) {
    if width > 0.0 {
        ui.add_sized(
            Vec2::new(width, 16.0),
            egui::Label::new(RichText::new(text).color(t.muted).size(10.5).strong()),
        );
    } else {
        ui.label(RichText::new(text).color(t.muted).size(10.5).strong());
    }
}

pub fn action_btn(label: &str, width: f32, color: Color32) -> egui::Button<'static> {
    let b = egui::Button::new(RichText::new(label.to_string()).size(11.5).color(color));
    if width > 0.0 { b.min_size(Vec2::new(width, 26.0)) } else { b }
}

pub fn prop_row(ui: &mut Ui, key: &str, val: &str, t: Theme) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).color(t.muted).size(10.5).monospace());
        ui.add_space(6.0);
        ui.label(RichText::new(val).color(t.text).size(10.5).monospace());
    });
    ui.add_space(2.0);
}

pub fn node_tooltip(ui: &mut Ui, node: &HierarchyNode, t: Theme) {
    ui.label(RichText::new("元素详情").strong().size(11.0));
    ui.separator();
    // 核心属性（始终显示）
    for (k, v) in [
        ("ControlType",  node.control_type.as_str()),
        ("AutomationId", node.automation_id.as_str()),
        ("ClassName",    node.class_name.as_str()),
        ("Name",         node.name.as_str()),
    ] {
        ui.horizontal(|ui| {
            ui.label(RichText::new(k).color(t.muted).size(10.0));
            ui.add_space(4.0);
            ui.label(RichText::new(v).monospace().size(10.0));
        });
    }
    // 扩展属性（有值时显示）
    for (k, v) in [
        ("FrameworkId",          node.framework_id.as_str()),
        ("HelpText",             node.help_text.as_str()),
        ("LocalizedControlType", node.localized_control_type.as_str()),
        ("AcceleratorKey",      node.accelerator_key.as_str()),
        ("AccessKey",            node.access_key.as_str()),
        ("ItemType",             node.item_type.as_str()),
        ("ItemStatus",           node.item_status.as_str()),
    ] {
        if !v.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(k).color(t.muted).size(10.0));
                ui.add_space(4.0);
                ui.label(RichText::new(v).monospace().size(10.0));
            });
        }
    }
    // 布尔属性（特殊值时显示）
    if node.is_password {
        ui.horizontal(|ui| {
            ui.label(RichText::new("IsPassword").color(t.muted).size(10.0));
            ui.add_space(4.0);
            ui.label(RichText::new("true").monospace().size(10.0));
        });
    }
    if !node.is_enabled {
        ui.horizontal(|ui| {
            ui.label(RichText::new("IsEnabled").color(t.muted).size(10.0));
            ui.add_space(4.0);
            ui.label(RichText::new("false").monospace().size(10.0));
        });
    }
    if node.is_offscreen {
        ui.horizontal(|ui| {
            ui.label(RichText::new("IsOffscreen").color(t.muted).size(10.0));
            ui.add_space(4.0);
            ui.label(RichText::new("true").monospace().size(10.0));
        });
    }
    if node.rect.width > 0 {
        ui.label(
            RichText::new(format!(
                "Rect {}×{}  @({},{})",
                node.rect.width, node.rect.height, node.rect.x, node.rect.y,
            ))
            .size(10.0)
            .color(t.muted),
        );
    }
}

/// RichText 扩展：条件加粗
pub trait RichTextExt {
    fn strong_if(self, cond: bool) -> Self;
}

impl RichTextExt for RichText {
    fn strong_if(self, cond: bool) -> Self {
        if cond { self.strong() } else { self }
    }
}

/// 转义 JavaScript 字符串中的特殊字符
/// Escape backticks for JavaScript template literals
pub fn escape_backtick(s: &str) -> String {
    s.replace('`', "\\`")
}

#[allow(dead_code)]
pub fn escape_js_string(s: &str) -> String {
    s.replace('\\', "\\\\")
     .replace('\'', "\\'")
     .replace('\n', "\\n")
     .replace('\r', "\\r")
     .replace('\t', "\\t")
}

/// 截断字符串（预留功能）
#[allow(dead_code)]
pub fn truncate_str(s: &str, max_chars: usize) -> String {
    let mut cs = s.chars();
    let collected: String = cs.by_ref().take(max_chars).collect();
    if cs.next().is_some() { format!("{}…", collected) } else { collected }
}
