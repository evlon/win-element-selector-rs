// src/app.rs
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Color32, Frame, Key, Layout, Margin, RichText,
    Rounding, ScrollArea, Sense, Stroke, TextEdit, Ui, Vec2,
};
use log::info;

use crate::{
    capture,
    capture_overlay::CaptureOverlay,
    highlight,
    model::{AppConfig, HierarchyNode, Operator, ValidationResult},
    mouse_hook::{self, CaptureMode},
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

/// Persisted capture data for restoring on restart.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PersistedCapture {
    hierarchy: Vec<HierarchyNode>,
    selected_node: Option<usize>,
    xpath_text: String,
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
    
    // Overlay
    overlay:       CaptureOverlay,

    // UI state
    status_msg:     String,
    history:        Vec<String>,       // recent XPaths, newest-first, capped at 20
    pending_save:   bool,               // flag to trigger save on next update

    // Config (persisted via egui storage)
    config:         AppConfig,

    // Countdown display (unused)
    #[allow(dead_code)]
    countdown_str:  String,
    
    // Real-time highlight tracking
    last_mouse_move: Option<Instant>,  // When mouse last moved
    last_highlight_pos: Option<(i32, i32)>,  // Last highlighted position
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

        // Try to restore last capture session from file
        let save_path = std::env::current_dir()
            .unwrap_or_default()
            .join("last_capture.json");
        
        let (hierarchy, selected_node, xpath_text) = if save_path.exists() {
            match std::fs::read_to_string(&save_path) {
                Ok(content) => {
                    match serde_json::from_str::<PersistedCapture>(&content) {
                        Ok(captured) => {
                            info!("Restored last capture from file: {} nodes", captured.hierarchy.len());
                            (captured.hierarchy, captured.selected_node, captured.xpath_text)
                        }
                        Err(e) => {
                            info!("Failed to parse last_capture.json: {}", e);
                            Self::mock_capture()
                        }
                    }
                }
                Err(e) => {
                    info!("Failed to read last_capture.json: {}", e);
                    Self::mock_capture()
                }
            }
        } else {
            info!("No last_capture.json found, using mock data");
            Self::mock_capture()
        };

        Self {
            hierarchy,
            selected_node,
            xpath_text,
            xpath_error:     None,
            custom_xpath:    false,
            show_simplified: config.show_simplified,
            validation:      ValidationResult::Idle,
            capture_state:   CaptureState::Idle,
            overlay:         CaptureOverlay::new(),
            status_msg:      "就绪 — 按 F4 开始捕获元素".to_string(),
            history:         config.last_xpaths.clone(),
            pending_save:    false,
            config,
            countdown_str:   String::new(),
            last_mouse_move: None,
            last_highlight_pos: None,
        }
    }

    fn mock_capture() -> (Vec<HierarchyNode>, Option<usize>, String) {
        let result = capture::mock();
        let xpath = xpath::generate(&result.hierarchy);
        (result.hierarchy, Some(3), xpath)
    }

    fn save_to_file(&self) {
        if self.hierarchy.is_empty() {
            return;
        }
        
        let save_path = std::env::current_dir()
            .unwrap_or_default()
            .join("last_capture.json");
        
        let captured = PersistedCapture {
            hierarchy: self.hierarchy.clone(),
            selected_node: self.selected_node,
            xpath_text: self.xpath_text.clone(),
        };
        
        match serde_json::to_string_pretty(&captured) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&save_path, json) {
                    info!("Failed to save to file: {}", e);
                } else {
                    info!("Saved capture to {}", save_path.display());
                }
            }
            Err(e) => {
                info!("Failed to serialize capture data: {}", e);
            }
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
            deadline: Instant::now() + Duration::from_secs(30),
        };  // Extended timeout for user convenience
        self.status_msg    = "请在 30 秒内点击目标控件 …".to_string();
        // Activate global mouse hook with swallow mode to prevent click from reaching target.
        mouse_hook::activate_capture(true);
        log::info!("Capture started - mouse hook activated with report_moves=true");
        // Show the capture guidance overlay.
        self.overlay.show();
    }

    fn finish_capture_at(&mut self, x: i32, y: i32, mode: CaptureMode) {
        // Deactivate mouse hook first.
        mouse_hook::deactivate_capture();
        // Hide the overlay.
        self.overlay.hide();
        self.capture_state = CaptureState::Capturing;
        
        // Handle different capture modes.
        match mode {
            CaptureMode::Batch => {
                self.status_msg = "批量捕获模式：正在分析相似元素…".to_string();
                // TODO: Implement batch capture logic for similar elements.
                // For now, fall back to single capture.
            }
            CaptureMode::Single | CaptureMode::None => {
                // Normal single capture.
            }
        }
        
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
                // Flash for 800ms so the user can see what was captured.
                highlight::flash(&last.rect, 800);
            }
        }
        self.hierarchy     = result.hierarchy;
        self.capture_state = CaptureState::Idle;
        self.custom_xpath  = false;
        self.validation    = ValidationResult::Idle;
        self.rebuild_xpath();
        self.pending_save  = true;  // Flag for eframe save
        self.save_to_file();  // Also save to file immediately
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
    
    /// Highlight element at the given screen position (for real-time preview).
    fn highlight_element_at(&mut self, x: i32, y: i32) {
        log::info!("Attempting to highlight element at ({}, {})", x, y);
        // Capture element at position (quick, non-blocking)
        let result = capture::capture_at(x, y);
        if result.error.is_none() {
            if let Some(last) = result.hierarchy.last() {
                // Short flash (300ms) for real-time preview
                log::info!("Highlighting element: {:?}", last.rect);
                highlight::flash(&last.rect, 300);
            }
        } else {
            log::warn!("Failed to capture element at ({}, {}): {:?}", x, y, result.error);
        }
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
        let line_count = self.xpath_text.lines().count().max(2).min(5);
        
        egui::TopBottomPanel::top("xpath_bar")
            .resizable(false)
            .frame(Frame::none().fill(Color32::from_gray(250)).inner_margin(Margin::symmetric(8.0, 2.0)))
            .show(ctx, |ui| {
                // Match all backgrounds to eliminate any visual gaps
                let style = ui.style_mut();
                style.visuals.widgets.inactive.bg_fill = Color32::from_gray(250);
                style.visuals.widgets.hovered.bg_fill = Color32::from_gray(250);
                style.visuals.widgets.active.bg_fill = Color32::from_gray(250);
                style.visuals.widgets.open.bg_fill = Color32::from_gray(250);
                style.visuals.extreme_bg_color = Color32::from_gray(250);  // TextEdit background
                style.visuals.panel_fill = Color32::from_gray(250);
                // Top row: Label + buttons
                ui.horizontal(|ui| {
                    ui.label(RichText::new("XPath").color(C_MUTED).size(11.0));
                    ui.add_space(4.0);
                    
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // History dropdown
                        if !self.history.is_empty() {
                            egui::ComboBox::from_id_salt("history_combo")
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
                        
                        // Custom mode badge
                        if self.custom_xpath {
                            if ui.button("↺ 重置").on_hover_text("回到自动生成的 XPath").clicked() {
                                self.custom_xpath = false;
                                self.rebuild_xpath();
                            }
                            ui.label(RichText::new("[自定义]").color(C_WARN).size(10.5));
                        }
                        
                        // Simplify toggle
                        let simp_txt = if self.show_simplified { "完整" } else { "精简" };
                        if ui.button(simp_txt).on_hover_text("切换精简/完整 XPath").clicked() {
                            self.show_simplified = !self.show_simplified;
                            self.config.show_simplified = self.show_simplified;
                            self.custom_xpath = false;
                            self.rebuild_xpath();
                        }
                        
                        // Copy button
                        if ui.button("📋").on_hover_text("复制 XPath").clicked() {
                            ui.output_mut(|o| o.copied_text = self.xpath_text.clone());
                            self.status_msg = "XPath 已复制到剪贴板".to_string();
                        }
                    });
                });
                
                // Multi-line XPath text box - wrap in Frame to cover any gaps
                Frame::none()
                    .fill(Color32::from_gray(250))
                    .stroke(Stroke::NONE)
                    .show(ui, |ui| {
                        // Override TextEdit stroke to match background
                        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::NONE;
                        ui.style_mut().visuals.widgets.hovered.bg_stroke = Stroke::NONE;
                        ui.style_mut().visuals.widgets.active.bg_stroke = Stroke::NONE;
                        
                        let edit_resp = ui.add(
                            TextEdit::multiline(&mut self.xpath_text)
                                .font(egui::TextStyle::Monospace)
                                .desired_rows(line_count)
                                .desired_width(ui.available_width())
                                .hint_text("//ControlType[@Attr='val']")
                                .text_color(C_MONO_FG)
                                .frame(false),
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
                    });
                
                // Error bar under the text box
                if let Some(err) = &self.xpath_error {
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

                        // Save / Exit buttons
                        if ui.add(btn("退出", 68.0)).clicked() {
                            self.save_to_file();  // Save before exit
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        ui.add_space(4.0);
                        if ui.add(
                            egui::Button::new(
                                RichText::new("💾 保存").color(Color32::WHITE).size(12.0),
                            )
                            .fill(C_TITLE_BG)
                            .min_size(Vec2::new(68.0, 28.0)),
                        ).clicked() {
                            self.push_history();
                            self.save_to_file();  // Save to file
                            self.status_msg = "已保存 XPath 到历史记录".to_string();
                            info!("saved xpath: {}", self.xpath_text);
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
            .id_salt("tree_scroll")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                let n = self.hierarchy.len();
                for idx in 0..n {
                    let is_sel    = self.selected_node == Some(idx);
                    let is_target = idx == n - 1;
                    let depth     = idx;
    
                    // Draw tree row - each call creates a new row vertically
                    let resp = draw_tree_row(ui, &self.hierarchy[idx], depth, is_sel, is_target, n);
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

        ScrollArea::vertical()
            .id_salt("property_scroll")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                let Some(sel_idx) = self.selected_node else {
                    ui.add_space(20.0);
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        ui.label(RichText::new("← 点击左侧节点查看属性").color(C_MUTED).italics());
                    });
                    return;
                };
                if sel_idx >= self.hierarchy.len() { return; }

        // Ancestors breadcrumb - simple vertical list
        ui.add_space(4.0);
        ui.label(RichText::new("层级追溯:").color(C_MUTED).size(11.0));
        ui.add_space(2.0);
        
        for i in 0..=sel_idx {
            let ancestor = &self.hierarchy[i];
            let is_current = i == sel_idx;
            let indent = 8.0 + (i as f32 * 10.0);
            
            ui.horizontal(|ui| {
                ui.add_space(indent);
                let label_text = format!("• {}", ancestor.tree_label());
                if is_current {
                    ui.label(RichText::new(&label_text).color(C_TARGET_FG).strong().size(10.5));
                } else {
                    ui.label(RichText::new(&label_text).color(C_MUTED).size(10.0));
                }
            });
        }
        
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);

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
                egui::ComboBox::from_id_salt(format!("op_{}_{}", sel_idx, fi))
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
        });
    }
}

// ─── eframe::App impl ────────────────────────────────────────────────────────

impl eframe::App for SelectorApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        info!("save() called - persisting data");
        // Save config
        self.config.last_xpaths = self.history.clone();
        if let Ok(json) = serde_json::to_string(&self.config) {
            storage.set_string("app_config", json);
            info!("Saved app_config");
        }

        // Save last capture session
        if !self.hierarchy.is_empty() {
            let captured = PersistedCapture {
                hierarchy: self.hierarchy.clone(),
                selected_node: self.selected_node,
                xpath_text: self.xpath_text.clone(),
            };
            if let Ok(json) = serde_json::to_string(&captured) {
                storage.set_string("last_capture", json);
                info!("Saved last_capture with {} nodes", self.hierarchy.len());
            } else {
                info!("Failed to serialize last_capture");
            }
        } else {
            info!("No hierarchy to save");
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
                self.overlay.hide();
                self.capture_state = CaptureState::Idle;
                self.status_msg = "捕获超时，已取消".to_string();
            } else if escape {
                mouse_hook::deactivate_capture();
                self.overlay.hide();
                self.capture_state = CaptureState::Idle;
                self.status_msg = "捕获已取消".to_string();
            } else if let Some(event) = mouse_hook::poll_click() {
                // Use the click event from global mouse hook (works across all windows).
                // Only process WM_LBUTTONDOWN events with modifier keys (Ctrl or Shift).
                if event.is_down {
                    let mode = event.capture_mode();
                    // Only capture if Ctrl or Shift is pressed
                    if mode != CaptureMode::None {
                        self.finish_capture_at(event.x, event.y, mode);
                    }
                    // If no modifier, ignore the click (allow normal clicking)
                }
            }
            
            // Handle mouse still/moved events for real-time highlight
            // Use debounce in main thread: check if mouse has been still for 500ms.
            let (mouse_x, mouse_y, mouse_time) = mouse_hook::get_mouse_state();
            
            if mouse_time > 0 {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                
                let elapsed = now_ms - mouse_time;
                
                if elapsed >= 500 {
                    // Mouse has been still for 500ms - trigger highlight.
                    if self.last_highlight_pos != Some((mouse_x, mouse_y)) {
                        log::info!("Mouse still at ({}, {}) for {}ms, triggering highlight", mouse_x, mouse_y, elapsed);
                        self.highlight_element_at(mouse_x, mouse_y);
                        self.last_highlight_pos = Some((mouse_x, mouse_y));
                    }
                } else {
                    // Mouse moved recently - clear highlight if showing a different position.
                    if self.last_highlight_pos.is_some() {
                        log::debug!("Mouse moved, clearing highlight");
                        self.last_highlight_pos = None;
                    }
                }
            }
        } else {
            // Not in capture mode, clear tracking
            self.last_mouse_move = None;
            self.last_highlight_pos = None;
        }

        // Draw the capture overlay if visible.
        self.overlay.draw(ctx);

        // ── Panels ────────────────────────────────────────────────────────────
        self.draw_titlebar(ctx);
        self.draw_xpath_bar(ctx);
        self.draw_status_bar(ctx);

        egui::CentralPanel::default()
            .frame(Frame::none().fill(Color32::from_gray(252)))
            .show(ctx, |ui| {
                // Use egui SidePanel API for proper layout
                // Left panel first
                egui::SidePanel::left("left_panel")
                    .resizable(true)
                    .default_width(350.0)
                    .width_range(280.0..=500.0)
                    .show_inside(ui, |ui| {
                        self.draw_hierarchy_panel(ui);
                    });
                
                // Right panel
                egui::SidePanel::right("right_panel")
                    .resizable(true)
                    .default_width(400.0)
                    .width_range(300.0..=600.0)
                    .show_inside(ui, |ui| {
                        self.draw_property_panel(ui);
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
/// Uses vertical layout with proper indentation and connecting lines.
fn draw_tree_row(
    ui: &mut Ui,
    node: &HierarchyNode,
    depth: usize,
    is_sel: bool,
    is_target: bool,
    _total_count: usize,
) -> egui::Response {
    // Each level gets 20px indentation
    let indent = depth as f32 * 20.0;
    // Row height is fixed for consistent alignment
    let row_height = 26.0;
    // Line width for tree connectors (unused)
    #[allow(unused_variables)]
    let line_width = 20.0;

    let bg = if is_sel { C_SEL_BG } else { Color32::TRANSPARENT }; 
    
    // Allocate full width for this row to ensure vertical stacking
    let (_, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_height), Sense::click());
    
    // Use the allocated rectangle for drawing
    let rect = resp.rect;
    let painter = ui.painter();
    
    // Draw background
    if is_sel {
        painter.rect_filled(rect, Rounding::same(3.0), bg);
    }
    
    // Calculate positions
    let line_x = rect.left() + indent + 10.0;
    let mid_y = rect.center().y;
    
    // Draw tree connecting lines
    painter.line_segment(
        [egui::pos2(line_x, mid_y), egui::pos2(line_x + 6.0, mid_y)],
        Stroke::new(1.0, C_TREE_LINE),
    );
    
    if depth > 0 {
        painter.line_segment(
            [egui::pos2(line_x, rect.top()), egui::pos2(line_x, mid_y)],
            Stroke::new(1.0, C_TREE_LINE),
        );
    }
    
    if !is_target {
        painter.line_segment(
            [egui::pos2(line_x, mid_y), egui::pos2(line_x, rect.bottom())],
            Stroke::new(1.0, C_TREE_LINE),
        );
    }
    
    // Node icon based on state
    let icon_x = rect.left() + indent + 16.0;
    let icon = if !node.included {
        "⊖"
    } else if is_target {
        "🎯"
    } else {
        "●"
    };
    let icon_color = if !node.included {
        C_MUTED
    } else if is_target {
        C_TARGET_FG
    } else {
        C_SEL_FG
    };
    painter.text(
        egui::pos2(icon_x, mid_y - 6.0),
        egui::Align2::LEFT_CENTER,
        icon,
        egui::FontId::default(),
        icon_color,
    );
    
    // Node label text (unused variable, only used for RichText)
    let label_x = icon_x + 18.0;
    #[allow(unused_variables)]
    let label_text = RichText::new(node.tree_label()).size(12.0);
    let label_color = if is_target {
        C_TARGET_FG
    } else if is_sel {
        C_SEL_FG
    } else if !node.included {
        C_MUTED
    } else {
        Color32::from_gray(40)
    };
    
    painter.text(
        egui::pos2(label_x, mid_y),
        egui::Align2::LEFT_CENTER,
        node.tree_label(),
        egui::FontId::proportional(12.0),
        label_color,
    );
    
    resp
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
