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
    model::{AppConfig, DetailedValidationResult, HierarchyNode, Operator, SegmentValidationResult, ValidationResult, WindowInfo},
    mouse_hook::{self, CaptureMode},
    xpath,
};

// ─── Palette ──────────────────────────────────────────────────────────────────
const C_TITLE_BG:  Color32 = Color32::from_rgb(30,  58, 100);
const C_TITLE_FG:  Color32 = Color32::from_rgb(226, 235, 246);
const C_SEL_BG:    Color32 = Color32::from_rgb(219, 234, 254);
const C_SEL_FG:    Color32 = Color32::from_rgb(30,  64, 175);
const C_PANEL_HDR: Color32 = Color32::from_rgb(241, 245, 249);
const C_BORDER:    Color32 = Color32::from_rgb(203, 213, 225);
const C_TARGET_FG: Color32 = Color32::from_rgb(30,  64, 175);
const C_OK:        Color32 = Color32::from_rgb(22,  163,  74);
const C_ERR:       Color32 = Color32::from_rgb(220,  38,  38);
const C_WARN:      Color32 = Color32::from_rgb(202, 138,   4);
const C_MUTED:     Color32 = Color32::from_rgb(107, 114, 128);
const C_MONO_FG:   Color32 = Color32::from_rgb(37,  99,  235);
const C_DIVIDER:   Color32 = Color32::from_rgb(220, 228, 240);

// ─── Capture state ────────────────────────────────────────────────────────────
#[derive(Debug, PartialEq)]
enum CaptureState {
    Idle,
    WaitingClick { deadline: Instant },
    Capturing,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PersistedCapture {
    hierarchy:     Vec<HierarchyNode>,
    selected_node: Option<usize>,
    xpath_text:    String,
}

// ─── App ──────────────────────────────────────────────────────────────────────
pub struct SelectorApp {
    hierarchy:      Vec<HierarchyNode>,
    selected_node:  Option<usize>,
    window_info:    Option<WindowInfo>,

    available_windows: Vec<WindowInfo>,
    show_window_panel: bool,

    xpath_text:      String,         // Combined display: "window_selector, element_xpath"
    window_selector: String,         // Window selector only
    element_xpath:   String,         // Element XPath only
    xpath_error:     Option<String>,
    custom_xpath:    bool,
    show_simplified: bool,

    validation:    ValidationResult,
    detailed_validation: Option<DetailedValidationResult>,
    capture_state: CaptureState,
    overlay:       CaptureOverlay,

    status_msg:  String,
    history:     Vec<String>,
    pending_save: bool,

    config:        AppConfig,
    #[allow(dead_code)]
    countdown_str: String,

    last_mouse_move:    Option<Instant>,
    last_highlight_pos: Option<(i32, i32)>,

    /// Which tree nodes are expanded (by index).  All start expanded.
    node_expanded: Vec<bool>,

    /// Width of the left panel in logical pixels (user-draggable).
    left_panel_width: f32,
    /// True while the user is dragging the divider.
    divider_dragging: bool,
}

impl SelectorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config: AppConfig = cc
            .storage
            .and_then(|s| {
                s.get_string("app_config")
                    .and_then(|json| serde_json::from_str(&json).ok())
            })
            .unwrap_or_default();

        let save_path = std::env::current_dir()
            .unwrap_or_default()
            .join("last_capture.json");

        let (hierarchy, selected_node, xpath_text, window_selector, element_xpath) = if save_path.exists() {
            match std::fs::read_to_string(&save_path)
                .ok()
                .and_then(|s| serde_json::from_str::<PersistedCapture>(&s).ok())
            {
                Some(c) => {
                    info!("Restored {} nodes from last_capture.json", c.hierarchy.len());
                    let xpath_result = xpath::generate(&c.hierarchy);
                    let xpath = format!("{}, {}", xpath_result.window_selector, xpath_result.element_xpath);
                    (c.hierarchy, c.selected_node, xpath, xpath_result.window_selector, xpath_result.element_xpath)
                }
                None => {
                    info!("Failed to parse last_capture.json, using mock");
                    Self::mock_capture()
                }
            }
        } else {
            info!("No last_capture.json, using mock");
            Self::mock_capture()
        };

        let n = hierarchy.len();
        Self {
            hierarchy,
            selected_node,
            window_info: None,
            available_windows: Vec::new(),
            show_window_panel: true,
            xpath_text,
            window_selector,
            element_xpath,
            xpath_error: None,
            custom_xpath: false,
            show_simplified: config.show_simplified,
            validation: ValidationResult::Idle,
            detailed_validation: None,
            capture_state: CaptureState::Idle,
            overlay: CaptureOverlay::new(),
            status_msg: "就绪 — 按 F4 开始捕获元素".to_string(),
            history: config.last_xpaths.clone(),
            pending_save: false,
            config,
            countdown_str: String::new(),
            last_mouse_move: None,
            last_highlight_pos: None,
            node_expanded: vec![true; n],
            left_panel_width: 300.0,
            divider_dragging: false,
        }
    }

    fn mock_capture() -> (Vec<HierarchyNode>, Option<usize>, String, String, String) {
        let result = capture::mock();
        let xpath_result = xpath::generate(&result.hierarchy);
        // Combined for display
        let xpath = format!("{}, {}", xpath_result.window_selector, xpath_result.element_xpath);
        (result.hierarchy, Some(3), xpath, xpath_result.window_selector, xpath_result.element_xpath)
    }

    fn save_to_file(&self) {
        if self.hierarchy.is_empty() { return; }
        let path = std::env::current_dir().unwrap_or_default().join("last_capture.json");
        let data = PersistedCapture {
            hierarchy:     self.hierarchy.clone(),
            selected_node: self.selected_node,
            xpath_text:    self.xpath_text.clone(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&data) {
            let _ = std::fs::write(&path, json);
            info!("Saved capture to {}", path.display());
        }
    }

    // ── XPath helpers ─────────────────────────────────────────────────────────

    fn rebuild_xpath(&mut self) {
        if self.custom_xpath { return; }
        
        // Generate window selector from window_info
        let window_selector = if let Some(ref win) = self.window_info {
            if !win.class_name.is_empty() && !win.title.is_empty() {
                format!("Window[@ClassName='{}' and @Name='{}']", win.class_name, win.title)
            } else if !win.class_name.is_empty() {
                format!("Window[@ClassName='{}']", win.class_name)
            } else if !win.title.is_empty() {
                format!("Window[@Name='{}']", win.title)
            } else {
                "Window".to_string()
            }
        } else {
            "Window".to_string()
        };
        
        // Generate element XPath from element hierarchy (nodes after Window)
        let element_xpath = if self.show_simplified {
            xpath::generate_simplified_elements(&self.hierarchy)
        } else {
            xpath::generate_elements(&self.hierarchy)
        };
        
        // Update all three fields
        self.window_selector = window_selector;
        self.element_xpath = element_xpath;
        self.xpath_text = format!("{}, {}", self.window_selector, self.element_xpath);
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
        };
        self.status_msg = "请在 30 秒内点击目标控件 …".to_string();
        mouse_hook::activate_capture(true);
        self.overlay.show();
    }

    fn finish_capture_at(&mut self, x: i32, y: i32, mode: CaptureMode) {
        mouse_hook::deactivate_capture();
        self.overlay.hide();
        self.capture_state = CaptureState::Capturing;

        if let CaptureMode::Batch = mode {
            self.status_msg = "批量捕获模式：正在分析相似元素…".to_string();
        }

        let result = capture::capture_at(x, y);
        if let Some(err) = &result.error {
            self.status_msg = format!("捕获失败: {}", err);
        } else {
            // Find Window node position
            let window_idx = result.hierarchy.iter()
                .position(|n| n.control_type == "Window")
                .unwrap_or(0);
            
            // Debug: log window info
            if let Some(ref win) = result.window_info {
                info!("Window info extracted: class='{}', name='{}', pid={}", 
                      win.class_name, win.title, win.process_id);
            } else {
                info!("No window info extracted from hierarchy");
            }
            
            // Extract element hierarchy (nodes after Window) for tree display
            let element_hierarchy: Vec<HierarchyNode> = if window_idx < result.hierarchy.len() - 1 {
                result.hierarchy[window_idx + 1..].to_vec()
            } else {
                Vec::new()
            };
            
            let n = element_hierarchy.len();
            
            self.selected_node = n.checked_sub(1);
            self.window_info = result.window_info.clone();
            let window_hint = result.window_info
                .as_ref()
                .map(|w| format!(" [窗口: {}]", w.title))
                .unwrap_or_default();
            self.status_msg = format!(
                "已捕获 {} 层层级 — 坐标 ({}, {}){}",
                n, result.cursor_x, result.cursor_y, window_hint
            );
            if let Some(last) = element_hierarchy.last() {
                highlight::flash(&last.rect, 800);
            }
            // Expand all nodes in the new tree
            self.node_expanded = vec![true; n];
            self.hierarchy = element_hierarchy;
        }
        self.capture_state = CaptureState::Idle;
        self.custom_xpath  = false;
        self.validation    = ValidationResult::Idle;
        self.rebuild_xpath();
        self.pending_save  = true;
        self.save_to_file();
        info!("capture done: {}", self.xpath_text);
    }

    fn do_validate(&mut self) {
        self.xpath_error = xpath::lint(&self.xpath_text);
        if let Some(err) = &self.xpath_error {
            self.status_msg = format!("XPath 语法错误: {}", err);
            return;
        }
        self.validation = ValidationResult::Running;
        
        // Use detailed validation API
        let detailed_result = capture::validate_selector_and_xpath_detailed(
            &self.window_selector,
            &self.element_xpath,
        );
        
        // Store detailed result for UI display
        self.detailed_validation = Some(detailed_result.clone());
        
        // Update overall validation state
        self.validation = detailed_result.overall.clone();
        
        if let ValidationResult::Found { ref first_rect, .. } = detailed_result.overall {
            if let Some(r) = first_rect { highlight::flash(r, 1200); }
        }
        self.status_msg = match &detailed_result.overall {
            ValidationResult::Found { count, .. } =>
                format!("校验通过 ✔ — 找到 {} 个匹配元素 (总用时: {}ms)", count, detailed_result.total_duration_ms),
            ValidationResult::NotFound => format!("校验失败 — 未找到匹配元素 (总用时: {}ms)", detailed_result.total_duration_ms),
            ValidationResult::Error(e) => format!("校验错误: {}", e),
            _ => String::new(),
        };
        self.push_history();
    }

    fn highlight_element_at(&mut self, x: i32, y: i32) {
        let result = capture::capture_at(x, y);
        if result.error.is_none() {
            if let Some(last) = result.hierarchy.last() {
                highlight::flash(&last.rect, 300);
            }
        }
    }

    // ── Panels ────────────────────────────────────────────────────────────────

    fn draw_titlebar(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("titlebar")
            .exact_height(32.0)
            .frame(Frame::none().fill(C_TITLE_BG))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("🔍  Windows 元素选择器")
                            .color(C_TITLE_FG)
                            .size(13.5)
                            .strong(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(12.0);
                        if let CaptureState::WaitingClick { deadline } = &self.capture_state {
                            let secs = deadline.saturating_duration_since(Instant::now()).as_secs();
                            ui.label(
                                RichText::new(format!("⏱ 等待点击 {}s", secs))
                                    .color(Color32::from_rgb(252, 211, 77))
                                    .size(11.5)
                                    .strong(),
                            );
                        }
                        if !self.history.is_empty() {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(format!("历史: {}", self.history.len()))
                                    .color(Color32::from_gray(150))
                                    .size(10.5),
                            );
                        }
                    });
                });
            });
    }

    fn draw_xpath_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("xpath_bar")
            .resizable(false)
            .frame(
                Frame::none()
                    .fill(Color32::from_gray(248))
                    .inner_margin(Margin::symmetric(10.0, 6.0))
                    .stroke(Stroke::new(1.0, C_BORDER)),
            )
            .show(ctx, |ui| {
                // Force TextEdit bg to match panel
                ui.style_mut().visuals.extreme_bg_color = Color32::from_gray(248);

                // Top row: label + controls
                ui.horizontal(|ui| {
                    ui.label(RichText::new("XPath").color(C_MUTED).size(11.0).strong());
                    if let Some(err) = &self.xpath_error {
                        ui.label(RichText::new(format!("⚠ {}", err)).color(C_ERR).size(10.5));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // History combo
                        if !self.history.is_empty() {
                            egui::ComboBox::from_id_salt("history_combo")
                                .selected_text("⏷ 历史")
                                .width(64.0)
                                .show_ui(ui, |ui| {
                                    let mut chosen = None;
                                    for (i, h) in self.history.iter().enumerate() {
                                        let label = if h.len() > 52 {
                                            format!("{}…", &h[..52])
                                        } else {
                                            h.clone()
                                        };
                                        if ui.selectable_label(false, label).clicked() {
                                            chosen = Some(i);
                                        }
                                    }
                                    if let Some(i) = chosen {
                                        self.xpath_text   = self.history[i].clone();
                                        self.custom_xpath = true;
                                        self.xpath_error  = xpath::lint(&self.xpath_text);
                                    }
                                });
                        }
                        // Reset custom
                        if self.custom_xpath {
                            ui.add_space(4.0);
                            if ui.small_button("↺ 重置").on_hover_text("回到自动生成的 XPath").clicked() {
                                self.custom_xpath = false;
                                self.rebuild_xpath();
                            }
                            ui.label(RichText::new("[自定义]").color(C_WARN).size(10.0));
                        }
                        // Simplified toggle
                        let simp_txt = if self.show_simplified { "🔲 完整" } else { "🔳 精简" };
                        if ui.small_button(simp_txt).on_hover_text("切换精简/完整 XPath").clicked() {
                            self.show_simplified        = !self.show_simplified;
                            self.config.show_simplified = self.show_simplified;
                            self.custom_xpath           = false;
                            self.rebuild_xpath();
                        }
                        // Validation details toggle
                        let val_txt = if self.config.show_validation_details { "📊 详情" } else { "📈 简洁" };
                        if ui.small_button(val_txt).on_hover_text("切换验证详情显示").clicked() {
                            self.config.show_validation_details = !self.config.show_validation_details;
                        }
                        // Three copy buttons
                        if ui.small_button("📋 窗口").on_hover_text("复制窗口选择器").clicked() {
                            ui.output_mut(|o| o.copied_text = self.window_selector.clone());
                            self.status_msg = "窗口选择器已复制到剪贴板".to_string();
                        }
                        if ui.small_button("📋 元素").on_hover_text("复制元素 XPath").clicked() {
                            ui.output_mut(|o| o.copied_text = self.element_xpath.clone());
                            self.status_msg = "元素 XPath 已复制到剪贴板".to_string();
                        }
                        if ui.small_button("📋 组合").on_hover_text("复制组合格式（逗号分隔）").clicked() {
                            ui.output_mut(|o| o.copied_text = self.xpath_text.clone());
                            self.status_msg = "组合 XPath 已复制到剪贴板".to_string();
                        }
                    });
                });

                ui.add_space(3.0);

                // Window selector row
                ui.horizontal(|ui| {
                    ui.label(RichText::new("🪟 窗口:").color(C_MUTED).size(10.0).monospace());
                    ui.add_space(4.0);
                    ui.label(RichText::new(&self.window_selector).font(egui::FontId::monospace(10.5)).color(Color32::from_gray(60)));
                });
                
                ui.add_space(2.0);

                // Element XPath row (editable)
                ui.horizontal(|ui| {
                    ui.label(RichText::new("🔗 元素:").color(C_MUTED).size(10.0).monospace());
                    ui.add_space(4.0);
                });
                
                let edit_resp = ui.add(
                    TextEdit::multiline(&mut self.element_xpath)
                        .font(egui::TextStyle::Monospace)
                        .desired_rows(2)
                        .desired_width(ui.available_width())
                        .hint_text("/ControlType[@Attr='val']")
                        .text_color(C_MONO_FG)
                        .frame(true),
                );
                if edit_resp.changed() {
                    self.custom_xpath = true;
                    // Update combined text
                    self.xpath_text = format!("{}, {}", self.window_selector, self.element_xpath);
                    self.xpath_error  = xpath::lint(&self.xpath_text);
                    self.validation   = ValidationResult::Idle;
                }
                if let Some(err) = &self.xpath_error {
                    edit_resp.on_hover_text(RichText::new(format!("⚠ {}", err)).color(C_ERR));
                }
            });
    }

    fn draw_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(46.0)
            .frame(
                Frame::none()
                    .fill(Color32::from_gray(245))
                    .inner_margin(Margin::symmetric(10.0, 7.0))
                    .stroke(Stroke::new(1.0, C_BORDER)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let msg_color = match &self.validation {
                        ValidationResult::Found { .. }              => C_OK,
                        ValidationResult::NotFound | ValidationResult::Error(_) => C_ERR,
                        ValidationResult::Running                   => C_WARN,
                        _                                           => C_MUTED,
                    };
                    ui.label(RichText::new(&self.status_msg).color(msg_color).size(11.5));

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // Exit
                        if ui.add(action_btn("退出", 64.0, Color32::from_gray(50))).clicked() {
                            self.save_to_file();
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        ui.add_space(4.0);
                        // Save
                        if ui.add(
                            egui::Button::new(RichText::new("💾 保存").color(Color32::WHITE).size(12.0))
                                .fill(C_TITLE_BG)
                                .min_size(Vec2::new(68.0, 28.0)),
                        ).clicked() {
                            self.push_history();
                            self.save_to_file();
                            self.status_msg = "已保存 XPath 到历史记录".to_string();
                        }
                        ui.add_space(10.0);
                        // Validate
                        let val_label = self.validation.label();
                        let val_color = match &self.validation {
                            ValidationResult::Found { .. }              => C_OK,
                            ValidationResult::NotFound | ValidationResult::Error(_) => C_ERR,
                            _                                           => Color32::from_gray(50),
                        };
                        if ui.add(
                            egui::Button::new(RichText::new(&val_label).color(val_color).size(11.5))
                                .stroke(Stroke::new(1.0, val_color))
                                .min_size(Vec2::new(110.0, 28.0)),
                        ).on_hover_text("F7 — 用当前 XPath 在屏幕上查找元素").clicked() {
                            self.do_validate();
                        }
                        ui.add_space(4.0);
                        // Capture
                        let (cap_label, cap_color) = match &self.capture_state {
                            CaptureState::WaitingClick { deadline } => {
                                let s = deadline.saturating_duration_since(Instant::now()).as_secs();
                                (format!("⏱ 等待点击 {}s", s), C_WARN)
                            }
                            CaptureState::Capturing => ("捕获中…".to_string(), C_WARN),
                            CaptureState::Idle      => ("重新捕获  F4".to_string(), Color32::from_gray(50)),
                        };
                        if ui.add(
                            egui::Button::new(RichText::new(&cap_label).color(cap_color).size(11.5))
                                .min_size(Vec2::new(110.0, 28.0)),
                        ).on_hover_text("F4 — 点击屏幕上任意控件进行捕获").clicked()
                            && self.capture_state == CaptureState::Idle
                        {
                            self.start_capture();
                        }
                    });
                });
            });
    }

    // ── Left panel: window list + element tree ─────────────────────────────────

    fn draw_left_panel(&mut self, ui: &mut Ui) {
        // ── Window selection ──────────────────────────────────────────────────
        if self.show_window_panel {
            panel_header(ui, "🪟  窗口选择");
            ui.add_space(2.0);

            ui.horizontal(|ui| {
                if ui.small_button("🔄 刷新").on_hover_text("重新加载窗口列表").clicked() {
                    self.available_windows = capture::list_windows();
                    self.status_msg = format!("已加载 {} 个窗口", self.available_windows.len());
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("✕").on_hover_text("隐藏窗口面板").clicked() {
                        self.show_window_panel = false;
                    }
                });
            });
            ui.add_space(2.0);

            if self.available_windows.is_empty() {
                ui.label(RichText::new("点击刷新加载窗口列表").color(C_MUTED).italics().size(10.5));
            } else {
                let max_h = (self.available_windows.len() as f32 * 22.0).min(160.0).max(44.0);
                ScrollArea::vertical()
                    .id_salt("win_scroll")
                    .max_height(max_h)
                    .show(ui, |ui| {
                        let wins: Vec<WindowInfo> = self.available_windows.clone();
                        for win in &wins {
                            let is_sel = self.window_info.as_ref() == Some(win);
                            let title  = truncate_str(&win.title, 36);
                            let resp   = ui.selectable_label(
                                is_sel,
                                RichText::new(format!("{} (pid:{})", title, win.process_id)).size(11.0),
                            );
                            if resp.clicked() {
                                self.window_info = Some(win.clone());
                                self.status_msg  = format!("已选择窗口: {}", win.title);
                            }
                            resp.on_hover_text(format!(
                                "标题: {}\n类名: {}\n进程ID: {}",
                                win.title, win.class_name, win.process_id
                            ));
                        }
                    });
            }

            // Current window
            ui.add_space(2.0);
            ui.separator();
            ui.add_space(1.0);
            if let Some(ref w) = self.window_info {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("当前:").color(C_MUTED).size(9.5));
                    ui.label(RichText::new(truncate_str(&w.title, 28)).color(C_TARGET_FG).strong().size(10.0));
                });
            } else {
                ui.label(RichText::new("当前窗口: 未设置").color(C_MUTED).italics().size(9.5));
            }
            ui.add_space(4.0);
        } else {
            // Show a small button to re-open the window panel
            if ui.small_button("🪟").on_hover_text("显示窗口面板").clicked() {
                self.show_window_panel = true;
            }
            ui.add_space(2.0);
        }

        // ── Element tree ──────────────────────────────────────────────────────
        panel_header(ui, "📂  元素层级结构");
        ui.add_space(2.0);

        // Ensure node_expanded vec is in sync
        if self.node_expanded.len() != self.hierarchy.len() {
            self.node_expanded.resize(self.hierarchy.len(), true);
        }

        // We render the hierarchy as a flat list but use indentation and
        // egui's CollapsingHeader to give true tree collapse behaviour.
        // Because egui's CollapsingHeader nests via closures, and we have
        // a flat Vec, we build a recursive render here.
        ScrollArea::vertical()
            .id_salt("tree_scroll")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                // We'll use a simple recursive closure approach via an index stack.
                // Each node at depth N is the child of the node at depth N-1.
                // Since hierarchy is strictly linear (each node is parent of next),
                // we render node[0] as root, node[1] as its child, etc.
                let n = self.hierarchy.len();
                if n == 0 {
                    ui.label(RichText::new("尚未捕获元素").color(C_MUTED).italics());
                    return;
                }

                // Tight spacing for tree rows
                ui.spacing_mut().item_spacing.y = 0.0;

                let validation_segments = self.detailed_validation.as_ref().map(|d| &d.segments);
                let show_validation_details = self.config.show_validation_details;
                Self::draw_tree_recursive(
                    ui,
                    &mut self.hierarchy,
                    &mut self.selected_node,
                    &mut self.node_expanded,
                    0,
                    n,
                    0,
                    validation_segments,
                    show_validation_details,
                );
                ui.add_space(12.0);
            });
    }

    /// Recursive tree drawing using egui CollapsingHeader.
    /// `start` is the current node index; all nodes from start..end are in the subtree
    /// rooted at the node at depth `depth_of_start`.
    /// Since the hierarchy is a strict ancestor chain (each node is parent of all following),
    /// node[i] is parent of node[i+1], so the "children" of node[i] is just {node[i+1]}.
    fn draw_tree_recursive(
        ui: &mut Ui,
        hierarchy: &mut Vec<HierarchyNode>,
        selected_node: &mut Option<usize>,
        expanded: &mut Vec<bool>,
        idx: usize,
        total: usize,
        _depth: usize,
        validation_segments: Option<&Vec<SegmentValidationResult>>,
        show_validation_details: bool,
    ) {
        let is_leaf   = idx + 1 >= total;
        let is_target = idx + 1 == total;
        let is_sel    = *selected_node == Some(idx);
        let included  = hierarchy[idx].included;

        let label_text = hierarchy[idx].tree_label();
        let icon = if is_target { "🎯" } else if included { "●" } else { "⊖" };
        let label_color = if is_target { C_TARGET_FG }
                          else if is_sel { C_SEL_FG }
                          else if !included { C_MUTED }
                          else { Color32::from_gray(35) };

        // Add validation marker if available and enabled
        let validation_marker = if show_validation_details {
            validation_segments
                .and_then(|segments| segments.get(idx))
                .map(|seg| {
                    if seg.matched {
                        format!(" ✅ {}ms", seg.duration_ms)
                    } else {
                        format!(" ❌ {}ms", seg.duration_ms)
                    }
                })
                .unwrap_or_default()
        } else {
            String::new()
        };

        let header_text = RichText::new(format!("{} {}{}", icon, label_text, validation_marker))
            .size(12.0)
            .color(label_color)
            .strong_if(is_sel || is_target);

        if is_leaf {
            // Leaf node — draw as selectable row
            let row_bg = if is_sel { C_SEL_BG } else { Color32::TRANSPARENT };
            egui::Frame::none()
                .fill(row_bg)
                .inner_margin(Margin::symmetric(4.0, 1.0))
                .show(ui, |ui| {
                    let resp = ui.add(
                        egui::Label::new(header_text)
                            .sense(Sense::click())
                            .truncate(),
                    );
                    if resp.clicked() {
                        *selected_node = Some(idx);
                    }
                    // Context menu
                    resp.context_menu(|ui| {
                        Self::node_context_menu(ui, hierarchy, idx, selected_node);
                    });
                    resp.on_hover_ui(|ui| {
                        node_tooltip(ui, &hierarchy[idx]);
                    });
                });
        } else {
            // Non-leaf: use CollapsingHeader for expand/collapse
            // We need to track open state manually to sync with `expanded`.
            let id = egui::Id::new(("tree_node", idx));

            let row_bg = if is_sel { C_SEL_BG } else { Color32::TRANSPARENT };
            let frame  = egui::Frame::none()
                .fill(row_bg)
                .inner_margin(Margin::symmetric(2.0, 0.0));

            frame.show(ui, |ui| {
                // Use CollapsingHeader with the expanded state driven by our vec.
                let ch = egui::CollapsingHeader::new(header_text)
                    .id_salt(id)
                    .open(Some(expanded[idx]))
                    .show(ui, |ui| {
                        // Child = next node in chain
                        Self::draw_tree_recursive(
                            ui,
                            hierarchy,
                            selected_node,
                            expanded,
                            idx + 1,
                            total,
                            _depth + 1,
                            validation_segments,
                            show_validation_details,
                        );
                    });

                // Update expanded state from header interaction
                expanded[idx] = ch.openness > 0.5;

                // Clicking the header label selects this node
                if ch.header_response.clicked() {
                    *selected_node = Some(idx);
                }

                // Context menu
                ch.header_response.context_menu(|ui| {
                    Self::node_context_menu(ui, hierarchy, idx, selected_node);
                });

                ch.header_response.on_hover_ui(|ui| {
                    node_tooltip(ui, &hierarchy[idx]);
                });
            });
        }
    }

    fn node_context_menu(
        ui: &mut Ui,
        hierarchy: &mut Vec<HierarchyNode>,
        idx: usize,
        selected_node: &mut Option<usize>,
    ) {
        let included = hierarchy[idx].included;
        let toggle_label = if included { "从 XPath 中排除此节点" } else { "将此节点加入 XPath" };
        if ui.button(toggle_label).clicked() {
            hierarchy[idx].included = !included;
            ui.close_menu();
        }
        ui.separator();
        if ui.button("高亮显示此元素").clicked() {
            highlight::flash(&hierarchy[idx].rect, 1500);
            ui.close_menu();
        }
        ui.separator();
        if ui.button("选中此节点").clicked() {
            *selected_node = Some(idx);
            ui.close_menu();
        }
    }

    // ── Right panel: properties ───────────────────────────────────────────────

    fn draw_right_panel(&mut self, ui: &mut Ui) {
        panel_header(ui, "⚙  元素属性");

        ScrollArea::vertical()
            .id_salt("prop_scroll")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                let Some(sel_idx) = self.selected_node else {
                    ui.add_space(24.0);
                    ui.label(RichText::new("← 点击左侧节点查看属性").color(C_MUTED).italics());
                    return;
                };
                if sel_idx >= self.hierarchy.len() { return; }

                // ── Window info ───────────────────────────────────────────────
                ui.add_space(6.0);
                egui::CollapsingHeader::new(
                    RichText::new("🪟  窗口信息").color(C_MUTED).size(11.0)
                )
                .default_open(true)
                .show(ui, |ui| {
                    egui::Frame::none()
                        .fill(Color32::from_rgb(248, 250, 252))
                        .stroke(Stroke::new(0.5, C_BORDER))
                        .rounding(Rounding::same(4.0))
                        .inner_margin(Margin::symmetric(10.0, 6.0))
                        .show(ui, |ui| {
                            if let Some(ref win) = self.window_info {
                                prop_row(ui, "标题", &win.title);
                                prop_row(ui, "类名", if win.class_name.is_empty() { "(空)" } else { &win.class_name });
                                prop_row(ui, "PID",  &win.process_id.to_string());
                            } else {
                                ui.label(RichText::new("未设置").color(C_MUTED).italics().size(10.5));
                            }
                        });
                });

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                // ── Breadcrumb / ancestor trail ───────────────────────────────
                egui::CollapsingHeader::new(
                    RichText::new("🔗  层级追溯").color(C_MUTED).size(11.0)
                )
                .default_open(true)
                .show(ui, |ui| {
                    for i in 0..=sel_idx {
                        let ancestor = &self.hierarchy[i];
                        let is_current = i == sel_idx;
                        let indent = 4.0 + (i as f32 * 12.0);
                        ui.horizontal(|ui| {
                            ui.add_space(indent);
                            let txt = format!("• {}", ancestor.tree_label());
                            if is_current {
                                ui.label(RichText::new(&txt).color(C_TARGET_FG).strong().size(10.5));
                            } else {
                                ui.label(RichText::new(&txt).color(C_MUTED).size(10.0));
                            }
                        });
                    }
                });

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                // ── Node summary ──────────────────────────────────────────────
                {
                    let node = &self.hierarchy[sel_idx];
                    egui::Frame::none()
                        .fill(Color32::from_rgb(239, 246, 255))
                        .rounding(Rounding::same(4.0))
                        .inner_margin(Margin::symmetric(10.0, 6.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("控件类型").color(C_MUTED).size(11.0));
                                ui.add_space(6.0);
                                ui.label(RichText::new(&node.control_type).color(C_TARGET_FG).strong().size(13.0));
                                if node.rect.width > 0 {
                                    ui.add_space(12.0);
                                    ui.label(
                                        RichText::new(format!(
                                            "{}×{}  @({},{})",
                                            node.rect.width, node.rect.height,
                                            node.rect.x, node.rect.y,
                                        ))
                                        .color(C_MUTED).size(10.5),
                                    );
                                }
                                if node.process_id > 0 {
                                    ui.label(
                                        RichText::new(format!("pid:{}", node.process_id))
                                            .color(C_MUTED).size(10.5),
                                    );
                                }
                            });
                        });
                }

                ui.add_space(6.0);

                // ── Attribute filter table ────────────────────────────────────
                // Column header row
                egui::Frame::none()
                    .fill(C_PANEL_HDR)
                    .inner_margin(Margin::symmetric(4.0, 2.0))
                    .stroke(Stroke::new(0.5, C_BORDER))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(22.0);    // checkbox width
                            col_label(ui, "属性名", 100.0);
                            col_label(ui, "运算符",  80.0);
                            col_label(ui, "值",      0.0);
                        });
                    });

                ui.add_space(2.0);

                let filter_count = self.hierarchy[sel_idx].filters.len();
                let mut dirty = false;
                for fi in 0..filter_count {
                    let alt = fi % 2 == 0;
                    let row_color = if alt {
                        Color32::from_rgb(252, 252, 255)
                    } else {
                        Color32::from_rgb(245, 247, 253)
                    };

                    egui::Frame::none()
                        .fill(row_color)
                        .inner_margin(Margin::symmetric(4.0, 1.0))
                        .show(ui, |ui| {
                            let filter = &mut self.hierarchy[sel_idx].filters[fi];
                            ui.horizontal(|ui| {
                                // Enable/disable checkbox
                                if ui.checkbox(&mut filter.enabled, "").changed() { dirty = true; }

                                // Attribute name (fixed width, read-only)
                                ui.add_sized(
                                    Vec2::new(100.0, 20.0),
                                    egui::Label::new(RichText::new(&filter.name.clone()).size(12.0)),
                                );

                                // Operator combo (fixed width)
                                let old_op = filter.operator.clone();
                                egui::ComboBox::from_id_salt(format!("op_{}_{}", sel_idx, fi))
                                    .selected_text(filter.operator.label())
                                    .width(76.0)
                                    .show_ui(ui, |ui| {
                                        for op in Operator::all() {
                                            ui.selectable_value(&mut filter.operator, op.clone(), op.label());
                                        }
                                    });
                                if filter.operator != old_op { dirty = true; }

                                // Value text field
                                let edit = TextEdit::singleline(&mut filter.value)
                                    .desired_width(ui.available_width() - 4.0)
                                    .font(egui::TextStyle::Monospace)
                                    .hint_text("—");
                                if ui.add(edit).changed() {
                                    filter.enabled = !filter.value.is_empty();
                                    dirty = true;
                                }
                            });
                        });
                }
                if dirty { self.rebuild_xpath(); }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                // ── XPath segment preview ─────────────────────────────────────
                ui.label(RichText::new("本节点 XPath 片段:").color(C_MUTED).size(11.0));
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

                // ── Include / exclude toggle ──────────────────────────────────
                ui.add_space(8.0);
                let included = self.hierarchy[sel_idx].included;
                let toggle_txt = if included { "⊖ 从 XPath 中排除此节点" } else { "⊕ 将此节点加入 XPath" };
                if ui.add(action_btn(toggle_txt, 0.0, if included { C_ERR } else { C_OK })).clicked() {
                    self.hierarchy[sel_idx].included = !included;
                    self.custom_xpath = false;
                    self.rebuild_xpath();
                }
                ui.add_space(8.0);
            });
    }
}

// ─── eframe::App impl ────────────────────────────────────────────────────────

impl eframe::App for SelectorApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.config.last_xpaths = self.history.clone();
        if let Ok(json) = serde_json::to_string(&self.config) {
            storage.set_string("app_config", json);
        }
        if !self.hierarchy.is_empty() {
            let data = PersistedCapture {
                hierarchy:     self.hierarchy.clone(),
                selected_node: self.selected_node,
                xpath_text:    self.xpath_text.clone(),
            };
            if let Ok(json) = serde_json::to_string(&data) {
                storage.set_string("last_capture", json);
            }
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.capture_state != CaptureState::Idle {
            ctx.request_repaint_after(Duration::from_millis(200));
        }

        // Global key handling
        let (f4, f7, escape) = ctx.input(|i| {
            (i.key_pressed(Key::F4), i.key_pressed(Key::F7), i.key_pressed(Key::Escape))
        });
        if f4 && self.capture_state == CaptureState::Idle { self.start_capture(); }
        if f7 { self.do_validate(); }

        // Capture wait logic
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
                if event.is_down {
                    let mode = event.capture_mode();
                    if mode != CaptureMode::None {
                        self.finish_capture_at(event.x, event.y, mode);
                    }
                }
            }

            // Real-time hover highlight (debounced 500ms)
            let (mx, my, mt) = mouse_hook::get_mouse_state();
            if mt > 0 {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                if now_ms.saturating_sub(mt) >= 500 {
                    if self.last_highlight_pos != Some((mx, my)) {
                        self.highlight_element_at(mx, my);
                        self.last_highlight_pos = Some((mx, my));
                    }
                } else {
                    self.last_highlight_pos = None;
                }
            }
        } else {
            self.last_mouse_move    = None;
            self.last_highlight_pos = None;
        }

        self.overlay.draw(ctx);

        // Global style — compact but not pathological
        ctx.set_style({
            let mut s = (*ctx.style()).clone();
            s.visuals.panel_fill         = Color32::WHITE;
            s.visuals.window_fill        = Color32::WHITE;
            s.spacing.item_spacing       = egui::vec2(4.0, 2.0);
            s.spacing.window_margin      = egui::Margin::same(0.0);
            s.spacing.button_padding     = egui::vec2(6.0, 3.0);
            s.spacing.indent             = 14.0;
            s.spacing.interact_size      = egui::vec2(18.0, 18.0);
            // Make the panel separator/divider thin (1 px) with a subtle colour
            s.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, C_DIVIDER);
            s
        });

        // ── Panels ────────────────────────────────────────────────────────────
        self.draw_titlebar(ctx);
        self.draw_xpath_bar(ctx);
        self.draw_status_bar(ctx);

        egui::CentralPanel::default()
            .frame(Frame::none().fill(Color32::from_gray(250)))
            .show(ctx, |ui| {
                // ── Manual two-column split with a 1-px draggable divider ──────
                // This avoids egui SidePanel's wide interactive resize handle.

                const DIVIDER_W:   f32 = 1.0;  // visible line width (px)
                const DRAG_ZONE_W: f32 = 5.0;  // invisible grab zone half-width each side
                const GAP:         f32 = 8.0;  // breathing space on each side of divider
                const LEFT_MIN:    f32 = 220.0;
                const LEFT_MAX:    f32 = 480.0;

                let full_rect = ui.available_rect_before_wrap();
                let left_w    = self.left_panel_width.clamp(LEFT_MIN, LEFT_MAX);

                // Pixel position of the divider centre line
                let div_x = full_rect.min.x + left_w;

                // Content rects: each side steps back GAP px from the divider
                let left_rect = egui::Rect::from_min_max(
                    full_rect.min,
                    egui::pos2(div_x - GAP, full_rect.max.y),
                );
                let right_rect = egui::Rect::from_min_max(
                    egui::pos2(div_x + GAP, full_rect.min.y),
                    full_rect.max,
                );

                // ── Drag zone — transparent, straddles the divider line ────────
                let drag_rect = egui::Rect::from_center_size(
                    egui::pos2(div_x, full_rect.center().y),
                    egui::vec2(DRAG_ZONE_W * 2.0, full_rect.height()),
                );
                let drag_id   = egui::Id::new("panel_divider_drag");
                let drag_resp = ui.interact(drag_rect, drag_id, Sense::drag());

                if drag_resp.dragged() {
                    self.divider_dragging = true;
                    self.left_panel_width =
                        (self.left_panel_width + drag_resp.drag_delta().x)
                            .clamp(LEFT_MIN, LEFT_MAX);
                }
                if drag_resp.drag_stopped() {
                    self.divider_dragging = false;
                }
                if drag_resp.hovered() || self.divider_dragging {
                    ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }

                // ── Draw the 1-px divider line ─────────────────────────────────
                let line_color = if self.divider_dragging { C_SEL_FG } else { C_BORDER };
                ui.painter().line_segment(
                    [
                        egui::pos2(div_x, full_rect.min.y),
                        egui::pos2(div_x, full_rect.max.y),
                    ],
                    Stroke::new(DIVIDER_W, line_color),
                );

                // ── Left panel ────────────────────────────────────────────────
                // Paint background directly, then shrink rect for content margin.
                // This avoids Frame inner_margin causing width overflow beyond clip.
                ui.painter().rect_filled(left_rect, 0.0, Color32::WHITE);
                let left_content_rect = left_rect.shrink2(egui::vec2(6.0, 4.0));
                let mut left_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(left_content_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                left_ui.set_clip_rect(left_rect);
                self.draw_left_panel(&mut left_ui);

                // ── Right panel ───────────────────────────────────────────────
                ui.painter().rect_filled(right_rect, 0.0, Color32::WHITE);
                let right_content_rect = right_rect.shrink2(egui::vec2(8.0, 4.0));
                let mut right_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(right_content_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                right_ui.set_clip_rect(right_rect);
                self.draw_right_panel(&mut right_ui);
            });
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn panel_header(ui: &mut Ui, title: &str) {
    // Constrain to available width so Frame inner_margin never causes overflow.
    let w = ui.available_width();
    Frame::none()
        .fill(C_PANEL_HDR)
        .inner_margin(Margin::symmetric(8.0, 3.0))
        .stroke(Stroke::new(0.5, C_BORDER))
        .show(ui, |ui| {
            ui.set_max_width(w);
            ui.label(RichText::new(title).color(Color32::from_gray(70)).size(11.5).strong());
        });
    ui.add_space(2.0);
}

fn col_label(ui: &mut Ui, text: &str, width: f32) {
    if width > 0.0 {
        ui.add_sized(
            Vec2::new(width, 16.0),
            egui::Label::new(RichText::new(text).color(C_MUTED).size(10.5).strong()),
        );
    } else {
        ui.label(RichText::new(text).color(C_MUTED).size(10.5).strong());
    }
}

fn action_btn(label: &str, width: f32, color: Color32) -> egui::Button<'static> {
    let b = egui::Button::new(RichText::new(label.to_string()).size(11.5).color(color));
    if width > 0.0 { b.min_size(Vec2::new(width, 26.0)) } else { b }
}

fn prop_row(ui: &mut Ui, key: &str, val: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).color(C_MUTED).size(10.5).monospace());
        ui.add_space(6.0);
        ui.label(RichText::new(val).color(Color32::from_gray(35)).size(10.5).monospace());
    });
    ui.add_space(2.0);
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    let mut cs = s.chars();
    let collected: String = cs.by_ref().take(max_chars).collect();
    if cs.next().is_some() { format!("{}…", collected) } else { collected }
}

fn node_tooltip(ui: &mut Ui, node: &HierarchyNode) {
    ui.label(RichText::new("元素详情").strong().size(11.0));
    ui.separator();
    for (k, v) in [
        ("ControlType",  node.control_type.as_str()),
        ("AutomationId", node.automation_id.as_str()),
        ("ClassName",    node.class_name.as_str()),
        ("Name",         node.name.as_str()),
    ] {
        ui.horizontal(|ui| {
            ui.label(RichText::new(k).color(C_MUTED).size(10.0));
            ui.add_space(4.0);
            ui.label(RichText::new(v).monospace().size(10.0));
        });
    }
    if node.rect.width > 0 {
        ui.label(
            RichText::new(format!(
                "Rect {}×{}  @({},{})",
                node.rect.width, node.rect.height, node.rect.x, node.rect.y,
            ))
            .size(10.0).color(C_MUTED),
        );
    }
}

// Extension trait for RichText to conditionally bold
trait RichTextExt {
    fn strong_if(self, cond: bool) -> Self;
}
impl RichTextExt for RichText {
    fn strong_if(self, cond: bool) -> Self {
        if cond { self.strong() } else { self }
    }
}