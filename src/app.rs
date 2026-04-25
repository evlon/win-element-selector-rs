// src/app.rs
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Color32, Frame, Key, Layout, Margin, RichText,
    Rounding, ScrollArea, Sense, Stroke, TextEdit, Ui, Vec2,
};
use log::info;

use crate::{
    capture,
    highlight,
    model::{AppConfig, HierarchyNode, Operator, ValidationResult},
    mouse_hook,
    xpath,
};

// ─── Palette ─────────────────────────────────────────────────────────────────
const C_TITLE_BG:    Color32 = Color32::from_rgb(30,  58, 100);
const C_TITLE_FG:    Color32 = Color32::from_rgb(226, 235, 246);
const C_SEL_BG:      Color32 = Color32::from_rgb(219, 234, 254);
const C_SEL_FG:      Color32 = Color32::from_rgb(30,  64, 175);
const C_PANEL_HDR:   Color32 = Color32::from_rgb(241, 245, 249);
const C_BORDER:      Color32 = Color32::from_rgb(203, 213, 225);
const C_TREE_LINE:   Color32 = Color32::from_rgb(180, 195, 215);
const C_TARGET_FG:   Color32 = Color32::from_rgb(30,  64, 175);
const C_OK:          Color32 = Color32::from_rgb(22,  163,  74);
const C_ERR:         Color32 = Color32::from_rgb(220,  38,  38);
const C_WARN:        Color32 = Color32::from_rgb(202, 138,   4);
const C_MUTED:       Color32 = Color32::from_rgb(107, 114, 128);
const C_MONO_FG:     Color32 = Color32::from_rgb(37,  99,  235);

// ─── Capture state machine ────────────────────────────────────────────────────
#[derive(Debug, PartialEq)]
enum CaptureState {
    Idle,
    /// F4 pressed; waiting for user to click target (3-second countdown).
    WaitingClick { deadline: Instant },
    Capturing,
}

// ─── App ─────────────────────────────────────────────────────────────────────
pub struct SelectorApp {
    // Data
    hierarchy:      Vec<HierarchyNode>,
    selected_node:  Option<usize>,

    // XPath
    xpath_text:     String,
    xpath_error:    Option<String>,
    custom_xpath:   bool,
    show_simplified: bool,

    // Validation
    validation:     ValidationResult,

    // Capture
    capture_state:  CaptureState,

    // UI state
    status_msg:     String,
    history:        Vec<String>,       // recent XPaths, newest-first, capped at 20

    // Config (persisted via egui storage)
    config:         AppConfig,

    // Countdown display
    countdown_str:  String,
}

impl SelectorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Restore config from egui persistent storage.
        let config: AppConfig = cc
            .storage
            .and_then(|s| {
                s.get_string("app_config")
                    .and_then(|json| serde_json::from_str(&json).ok())
            })
            .unwrap_or_default();

        let result   = capture::mock();
        let xpath    = xpath::generate(&result.hierarchy);

        Self {
            hierarchy:       result.hierarchy,
            selected_node:   Some(3),
            xpath_text:      xpath,
            xpath_error:     None,
            custom_xpath:    false,
            show_simplified: config.show_simplified,
            validation:      ValidationResult::Idle,
            capture_state:   CaptureState::Idle,
            status_msg:      "就绪 — 按 F4 开始捕获元素".to_string(),
            history:         config.last_xpaths.clone(),
            config,
            countdown_str:   String::new(),
        }
    }

    // ── XPath helpers ─────────────────────────────────────────────────────────

    fn rebuild_xpath(&mut self) {
        if self.custom_xpath { return; }
        self.xpath_text = if self.show_simplified {
            xpath::simplify(&self.hierarchy)
        } else {
            xpath::generate(&self.hierarchy)
        };
        self.xpath_error = xpath::lint(&self.xpath_text);
        self.validation  = ValidationResult::Idle;
    }

    fn push_history(&mut self) {
        let x = self.xpath_text.clone();
        if x.is_empty() { return; }
        self.history.retain(|h| h != &x);
        self.history.insert(0, x);
        self.history.truncate(20);
    }

    // ── Actions ───────────────────────────────────────────────────────────────

    fn start_capture(&mut self) {
        self.capture_state = CaptureState::WaitingClick {
            deadline: Instant::now() + Duration::from_secs(5),
        };
        self.status_msg    = "请在 5 秒内点击目标控件 …".to_string();
        // Activate global mouse hook with swallow mode to prevent click from reaching target.
        mouse_hook::activate_capture(true);
    }

    fn finish_capture_at(&mut self, x: i32, y: i32) {
        // Deactivate mouse hook first.
        mouse_hook::deactivate_capture();
        self.capture_state = CaptureState::Capturing;
        let result = capture::capture_at(x, y);
        if let Some(err) = &result.error {
            self.status_msg = format!("捕获失败: {}", err);
        } else {
            let n = result.hierarchy.len();
            self.selected_node = n.checked_sub(1);
            self.status_msg = format!(
                "已捕获 {} 层层级 — 坐标 ({}, {})",
                n, result.cursor_x, result.cursor_y
            );
            // Flash highlight on captured element.
            if let Some(last) = result.hierarchy.last() {
                highlight::flash(&last.rect, 800);
            }
        }
        self.hierarchy     = result.hierarchy;
        self.capture_state = CaptureState::Idle;
        self.custom_xpath  = false;
        self.validation    = ValidationResult::Idle;
        self.rebuild_xpath();
        info!("capture done: {}", self.xpath_text);
    }

    fn do_validate(&mut self) {
        self.xpath_error = xpath::lint(&self.xpath_text);
        if let Some(err) = &self.xpath_error {
            self.status_msg = format!("XPath 语法错误: {}", err);
            return;
        }
        self.validation = ValidationResult::Running;
        let result = capture::validate(&self.xpath_text);

        // Flash if found.
        if let ValidationResult::Found { ref first_rect, .. } = result {
            if let Some(r) = first_rect {
                highlight::flash(r, 1200);
            }
        }

        self.status_msg = match &result {
            ValidationResult::Found { count, .. } =>
                format!("校验通过 ✔ — 找到 {} 个匹配元素", count),
            ValidationResult::NotFound =>
                "校验失败 — 未找到匹配元素".to_string(),
            ValidationResult::Error(e) =>
                format!("校验错误: {}", e),
            _ => String::new(),
        };
        self.validation = result;
        self.push_history();
    }

    // ── Drawing helpers ───────────────────────────────────────────────────────

    fn draw_titlebar(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("titlebar")
            .exact_height(30.0)
            .frame(Frame::none().fill(C_TITLE_BG))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("🔍  Windows 元素选择器")
                            .color(C_TITLE_FG)
                            .size(13.5)
                            .strong(),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(10.0);
                        // Capture countdown badge
                        if let CaptureState::WaitingClick { deadline } = &self.capture_state {
                            let secs = deadline.saturating_duration_since(Instant::now()).as_secs();
                            ui.label(
                                RichText::new(format!("等待点击 {}s", secs))
                                    .color(Color32::from_rgb(252, 211, 77))
                                    .size(11.0)
                                    .strong(),
                            );
                        }
                        // History count
                        if !self.history.is_empty() {
                            ui.label(
                                RichText::new(format!("历史: {}", self.history.len()))
                                    .color(Color32::from_gray(160))
                                    .size(10.5),
                            );
                        }
                    });
                });
            });
    }

    fn draw_xpath_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("xpath_bar")
            .exact_height(44.0)
            .frame(Frame::none().fill(Color32::from_gray(250)).inner_margin(Margin::symmetric(8.0, 5.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("XPath").color(C_MUTED).size(11.0),
                    );
                    ui.add_space(4.0);

                    // XPath text box
                    let edit_resp = ui.add(
                        TextEdit::singleline(&mut self.xpath_text)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(ui.available_width() - 260.0)
                            .hint_text("//ControlType[@Attr='val']")
                            .text_color(C_MONO_FG),
                    );
                    if edit_resp.changed() {
                        self.custom_xpath = true;
                        self.xpath_error  = xpath::lint(&self.xpath_text);
                        self.validation   = ValidationResult::Idle;
                    }

                    // Error tooltip on hover
                    if let Some(err) = &self.xpath_error {
                        edit_resp.on_hover_text(
                            RichText::new(format!("⚠ {}", err)).color(C_ERR),
                        );
                    }

                    ui.add_space(6.0);

                    // Copy button
                    if ui.button("📋").on_hover_text("复制 XPath").clicked() {
                        ui.output_mut(|o| o.copied_text = self.xpath_text.clone());
                        self.status_msg = "XPath 已复制到剪贴板".to_string();
                    }

                    // Simplify toggle
                    let simp_txt = if self.show_simplified { "完整" } else { "精简" };
                    if ui.button(simp_txt).on_hover_text("切换精简/完整 XPath").clicked() {
                        self.show_simplified = !self.show_simplified;
                        self.config.show_simplified = self.show_simplified;
                        self.custom_xpath = false;
                        self.rebuild_xpath();
                    }

                    // Custom mode badge
                    if self.custom_xpath {
                        ui.label(RichText::new("[自定义]").color(C_WARN).size(10.5));
                        if ui.button("↺ 重置").on_hover_text("回到自动生成的 XPath").clicked() {
                            self.custom_xpath = false;
                            self.rebuild_xpath();
                        }
                    }

                    // History dropdown
                    if !self.history.is_empty() {
                        egui::ComboBox::from_id_source("history_combo")
                            .selected_text("历史")
                            .width(56.0)
                            .show_ui(ui, |ui| {
                                let mut chosen = None;
                                for (i, h) in self.history.iter().enumerate() {
                                    let label = if h.len() > 48 {
                                        format!("{}…", &h[..48])
                                    } else {
                                        h.clone()
                                    };
                                    if ui.selectable_label(false, label).clicked() {
                                        chosen = Some(i);
                                    }
                                }
                                if let Some(i) = chosen {
                                    self.xpath_text  = self.history[i].clone();
                                    self.custom_xpath = true;
                                    self.xpath_error  = xpath::lint(&self.xpath_text);
                                }
                            });
                    }
                });

                // Error bar under the text box
                if let Some(err) = &self.xpath_error.clone() {
                    ui.label(RichText::new(format!("  ⚠ {}", err)).color(C_ERR).size(10.5));
                }
            });
    }

    fn draw_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(48.0)
            .frame(Frame::none()
                .fill(Color32::from_gray(246))
                .inner_margin(Margin::symmetric(8.0, 6.0))
                .stroke(Stroke::new(1.0, C_BORDER)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    // Status message
                    let msg_color = match &self.validation {
                        ValidationResult::Found { .. } => C_OK,
                        ValidationResult::NotFound | ValidationResult::Error(_) => C_ERR,
                        ValidationResult::Running   => C_WARN,
                        _ => C_MUTED,
                    };
                    ui.label(RichText::new(&self.status_msg).color(msg_color).size(11.5));

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(4.0);

                        // Cancel / Confirm
                        if ui.add(btn("取消", 68.0)).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        ui.add_space(4.0);
                        if ui.add(
                            egui::Button::new(
                                RichText::new("✔ 确定").color(Color32::WHITE).size(12.0),
                            )
                            .fill(C_TITLE_BG)
                            .min_size(Vec2::new(68.0, 28.0)),
                        ).clicked() {
                            self.push_history();
                            self.status_msg = format!("已确认 XPath: {}", self.xpath_text);
                            info!("confirmed xpath: {}", self.xpath_text);
                        }

                        ui.add_space(10.0);

                        // Validate button
                        let val_label = self.validation.label();
                        let val_color = match &self.validation {
                            ValidationResult::Found { .. } => C_OK,
                            ValidationResult::NotFound | ValidationResult::Error(_) => C_ERR,
                            _ => Color32::from_gray(50),
                        };
                        if ui.add(
                            egui::Button::new(RichText::new(&val_label).color(val_color).size(11.5))
                                .stroke(Stroke::new(1.0, val_color))
                                .min_size(Vec2::new(110.0, 28.0)),
                        ).on_hover_text("F7 — 用当前 XPath 在屏幕上查找元素").clicked() {
                            self.do_validate();
                        }

                        ui.add_space(4.0);

                        // Capture button
                        let (cap_label, cap_color) = match &self.capture_state {
                            CaptureState::WaitingClick { deadline } => {
                                let secs = deadline.saturating_duration_since(Instant::now()).as_secs();
                                (format!("等待点击 {}s", secs), C_WARN)
                            }
                            CaptureState::Capturing => ("捕获中…".to_string(), C_WARN),
                            CaptureState::Idle      => ("重新捕获  F4".to_string(), Color32::from_gray(50)),
                        };
                        if ui.add(
                            egui::Button::new(RichText::new(&cap_label).color(cap_color).size(11.5))
                                .min_size(Vec2::new(110.0, 28.0)),
                        ).on_hover_text("F4 — 点击屏幕上任意控件进行捕获").clicked() {
                            if self.capture_state == CaptureState::Idle {
                                self.start_capture();
                            }
                        }
                    });
                });
            });
    }

    fn draw_hierarchy_panel(&mut self, ui: &mut Ui) {
        panel_header(ui, "📂  元素层级结构");

        ScrollArea::vertical()
            .id_source("tree_scroll")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                let n = self.hierarchy.len();
                for idx in 0..n {
                    let is_sel    = self.selected_node == Some(idx);
                    let is_target = idx == n - 1;
                    let depth     = idx;

                    let resp = draw_tree_row(ui, &self.hierarchy[idx], depth, is_sel, is_target);
                    if resp.clicked() {
                        self.selected_node = Some(idx);
                    }
                    
                    // Hover tooltip
                    resp.clone().on_hover_ui(|ui| {
                        node_tooltip(ui, &self.hierarchy[idx]);
                    });
                    
                    // Context menu
                    resp.context_menu(|ui| {
                        let node = &mut self.hierarchy[idx];
                        if ui.button(if node.included { "从 XPath 中排除此节点" } else { "将此节点加入 XPath" }).clicked() {
                            node.included = !node.included;
                            self.custom_xpath = false;
                            self.rebuild_xpath();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("高亮显示此元素").clicked() {
                            highlight::flash(&self.hierarchy[idx].rect, 1500);
                            ui.close_menu();
                        }
                    });
                }
                ui.add_space(16.0);
            });
    }

    fn draw_property_panel(&mut self, ui: &mut Ui) {
        panel_header(ui, "⚙  元素属性");

        let Some(sel_idx) = self.selected_node else {
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.label(RichText::new("← 点击左侧节点查看属性").color(C_MUTED).italics());
            });
            return;
        };
        if sel_idx >= self.hierarchy.len() { return; }

        // Node summary
        let node = &self.hierarchy[sel_idx];
        egui::Frame::none()
            .fill(Color32::from_rgb(239, 246, 255))
            .inner_margin(Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("控件类型").color(C_MUTED).size(11.0));
                    ui.add_space(6.0);
                    ui.label(RichText::new(&node.control_type.clone()).color(C_TARGET_FG).strong().size(13.0));
                    ui.add_space(16.0);
                    if node.rect.width > 0 {
                        ui.label(
                            RichText::new(format!(
                                "  {}×{}  @({},{})",
                                node.rect.width, node.rect.height,
                                node.rect.x, node.rect.y,
                            ))
                            .color(C_MUTED)
                            .size(10.5),
                        );
                    }
                    if node.process_id > 0 {
                        ui.label(
                            RichText::new(format!("  pid:{}", node.process_id))
                                .color(C_MUTED)
                                .size(10.5),
                        );
                    }
                });
            });
        ui.add_space(6.0);

        // Column headers
        ui.horizontal(|ui| {
            ui.add_space(28.0);
            col_label(ui, "属性名", 90.0);
            col_label(ui, "运算符", 76.0);
            col_label(ui, "值", 0.0);
        });
        ui.separator();
        ui.add_space(2.0);

        // Filter rows
        let filter_count = self.hierarchy[sel_idx].filters.len();
        let mut dirty = false;

        for fi in 0..filter_count {
            let filter = &mut self.hierarchy[sel_idx].filters[fi];
            ui.horizontal(|ui| {
                if ui.checkbox(&mut filter.enabled, "").changed() { dirty = true; }

                // Attribute name — fixed width read-only
                ui.add_sized(
                    Vec2::new(90.0, 20.0),
                    egui::Label::new(RichText::new(&filter.name.clone()).size(12.0)),
                );

                // Operator combo
                let old = filter.operator.clone();
                egui::ComboBox::from_id_source(format!("op_{}_{}", sel_idx, fi))
                    .selected_text(filter.operator.label())
                    .width(76.0)
                    .show_ui(ui, |ui| {
                        for op in Operator::all() {
                            ui.selectable_value(&mut filter.operator, op.clone(), op.label());
                        }
                    });
                if filter.operator != old { dirty = true; }

                // Value
                let edit = TextEdit::singleline(&mut filter.value)
                    .desired_width(ui.available_width() - 4.0)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("—");
                if ui.add(edit).changed() {
                    filter.enabled = !filter.value.is_empty();
                    dirty = true;
                }
            });
            ui.add_space(3.0);
        }

        if dirty { self.rebuild_xpath(); }

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(6.0);

        // Per-node XPath segment preview
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new("本节点 XPath 片段:").color(C_MUTED).size(11.0));
        });
        ui.add_space(2.0);
        let seg = self.hierarchy[sel_idx].xpath_segment();
        egui::Frame::none()
            .fill(Color32::from_rgb(248, 250, 252))
            .stroke(Stroke::new(0.5, C_BORDER))
            .rounding(Rounding::same(4.0))
            .inner_margin(Margin::symmetric(8.0, 4.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(&seg)
                        .font(egui::FontId::monospace(11.0))
                        .color(C_MONO_FG),
                );
            });

        // Node include toggle
        ui.add_space(8.0);
        let node_included = self.hierarchy[sel_idx].included;
        let toggle_txt = if node_included { "从 XPath 中排除此节点" } else { "将此节点加入 XPath" };
        if ui.add(btn(toggle_txt, 0.0)).clicked() {
            self.hierarchy[sel_idx].included = !node_included;
            self.custom_xpath = false;
            self.rebuild_xpath();
        }
    }
}

// ─── eframe::App impl ────────────────────────────────────────────────────────

impl eframe::App for SelectorApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.config.last_xpaths = self.history.clone();
        if let Ok(json) = serde_json::to_string(&self.config) {
            storage.set_string("app_config", json);
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Request continuous repaint while waiting (countdown timer).
        if self.capture_state != CaptureState::Idle {
            ctx.request_repaint_after(Duration::from_millis(200));
        }

        // ── Global keyboard handling ──────────────────────────────────────────
        let (f4, f7, escape) = ctx.input(|i| {
            let f4     = i.key_pressed(Key::F4);
            let f7     = i.key_pressed(Key::F7);
            let escape = i.key_pressed(Key::Escape);
            (f4, f7, escape)
        });

        if f4 && self.capture_state == CaptureState::Idle {
            self.start_capture();
        }
        if f7 { self.do_validate(); }

        // Handle click while in wait state using global mouse hook.
        if let CaptureState::WaitingClick { deadline } = &self.capture_state {
            if Instant::now() > *deadline {
                mouse_hook::deactivate_capture();
                self.capture_state = CaptureState::Idle;
                self.status_msg = "捕获超时，已取消".to_string();
            } else if escape {
                mouse_hook::deactivate_capture();
                self.capture_state = CaptureState::Idle;
                self.status_msg = "捕获已取消".to_string();
            } else if let Some(event) = mouse_hook::poll_click() {
                // Use the click event from global mouse hook (works across all windows).
                // Only process WM_LBUTTONDOWN events for capture.
                if event.is_down {
                    self.finish_capture_at(event.x, event.y);
                }
            }
        }

        // ── Panels ────────────────────────────────────────────────────────────
        self.draw_titlebar(ctx);
        self.draw_xpath_bar(ctx);
        self.draw_status_bar(ctx);

        egui::CentralPanel::default()
            .frame(Frame::none().fill(Color32::from_gray(252)))
            .show(ctx, |ui| {
                // Split into left (hierarchy) + right (properties).
                let avail = ui.available_size();
                let left_w = (avail.x * 0.44).max(280.0).min(440.0);

                ui.horizontal(|ui| {
                    // Left
                    ui.allocate_ui(Vec2::new(left_w, avail.y), |ui| {
                        self.draw_hierarchy_panel(ui);
                    });

                    // Divider
                    ui.add(
                        egui::Separator::default().vertical().spacing(0.0),
                    );

                    // Right
                    ui.allocate_ui(Vec2::new(ui.available_width(), avail.y), |ui| {
                        self.draw_property_panel(ui);
                    });
                });
            });
    }
}

// ─── Small helpers ────────────────────────────────────────────────────────────

fn panel_header(ui: &mut Ui, title: &str) {
    let avail = ui.available_width();
    Frame::none()
        .fill(C_PANEL_HDR)
        .inner_margin(Margin::symmetric(10.0, 5.0))
        .stroke(Stroke::new(0.5, C_BORDER))
        .show(ui, |ui| {
            ui.set_min_width(avail);
            ui.label(RichText::new(title).color(Color32::from_gray(80)).size(11.5));
        });
}

fn col_label(ui: &mut Ui, text: &str, width: f32) {
    if width > 0.0 {
        ui.add_sized(
            Vec2::new(width, 16.0),
            egui::Label::new(RichText::new(text).color(C_MUTED).size(10.5)),
        );
    } else {
        ui.label(RichText::new(text).color(C_MUTED).size(10.5));
    }
}

fn btn(label: &str, width: f32) -> egui::Button<'static> {
    let b = egui::Button::new(RichText::new(label.to_string()).size(12.0));
    if width > 0.0 { b.min_size(Vec2::new(width, 28.0)) } else { b }
}

/// Draw one tree row and return the response for click/hover handling.
fn draw_tree_row(
    ui: &mut Ui,
    node: &HierarchyNode,
    depth: usize,
    is_sel: bool,
    is_target: bool,
) -> egui::Response {
    let indent = depth as f32 * 16.0 + 8.0;

    let bg = if is_sel { C_SEL_BG } else { Color32::TRANSPARENT };
    let frame = Frame::none()
        .fill(bg)
        .rounding(Rounding::same(3.0))
        .inner_margin(Margin { left: 0.0, right: 4.0, top: 1.0, bottom: 1.0 });

    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(indent);

            // Tree connector
            let (line_rect, _) = ui.allocate_exact_size(Vec2::new(16.0, 22.0), Sense::hover());
            let painter = ui.painter();
            let lx = line_rect.left() + 7.0;
            let my = line_rect.center().y;
            painter.line_segment(
                [egui::pos2(lx, line_rect.top()), egui::pos2(lx, my)],
                Stroke::new(1.0, C_TREE_LINE),
            );
            painter.line_segment(
                [egui::pos2(lx, my), egui::pos2(line_rect.right() + 2.0, my)],
                Stroke::new(1.0, C_TREE_LINE),
            );
            if !is_target {
                painter.line_segment(
                    [egui::pos2(lx, my), egui::pos2(lx, line_rect.bottom())],
                    Stroke::new(1.0, C_TREE_LINE),
                );
            }

            // Excluded badge
            if !node.included {
                ui.label(RichText::new("⊖ ").color(C_MUTED).size(10.0));
            }

            // Label text
            let label_text = RichText::new(node.tree_label()).size(12.0);
            let label_text = if is_target {
                label_text.color(C_TARGET_FG).strong()
            } else if is_sel {
                label_text.color(C_SEL_FG)
            } else if !node.included {
                label_text.color(C_MUTED).strikethrough()
            } else {
                label_text.color(Color32::from_gray(40))
            };
            ui.label(label_text)
        })
        .inner
    })
    .inner
    .interact(Sense::click())
}

fn node_tooltip(ui: &mut Ui, node: &HierarchyNode) {
    ui.label(RichText::new("元素详情").strong().size(11.0));
    ui.separator();
    let rows = [
        ("ControlType",  node.control_type.as_str()),
        ("AutomationId", node.automation_id.as_str()),
        ("ClassName",    node.class_name.as_str()),
        ("Name",         node.name.as_str()),
    ];
    for (k, v) in rows {
        ui.horizontal(|ui| {
            ui.label(RichText::new(k).color(C_MUTED).size(10.0));
            ui.add_space(4.0);
            ui.label(RichText::new(v).monospace().size(10.0));
        });
    }
    if node.rect.width > 0 {
        ui.label(
            RichText::new(format!(
                "Rect  {}×{}  @ ({}, {})",
                node.rect.width, node.rect.height, node.rect.x, node.rect.y
            ))
            .size(10.0)
            .color(C_MUTED),
        );
    }
}
