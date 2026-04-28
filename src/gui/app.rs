// src/gui/app.rs
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Color32, Frame, Key, Layout, Margin, RichText,
    Rounding, ScrollArea, Sense, Stroke, TextEdit, Ui, Vec2,
};
use log::info;

// 引用核心模块（通过 lib.rs 导出）
use element_selector::core::model::{
    AppConfig, DetailedValidationResult, ElementTab, HighlightInfo,
    HierarchyNode, Operator, PropertyFilter, SegmentValidationResult,
    ValidationResult, WindowInfo,
};
use element_selector::core::xpath;
use element_selector::capture;

// 引用同目录的 GUI 模块
use super::capture_overlay::CaptureOverlay;
use super::highlight;
use super::mouse_hook::{self, CaptureMode};

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
    window_info:   Option<WindowInfo>,
}

// ─── App ──────────────────────────────────────────────────────────────────────
pub struct SelectorApp {
    /// 元素名称（用户自定义，用于本地速查）
    element_name:      String,
    /// 当前激活的标签页
    active_tab:        ElementTab,
    
    hierarchy:      Vec<HierarchyNode>,
    selected_node:  Option<usize>,
    window_info:    Option<WindowInfo>,
    /// 窗口属性过滤器（用于窗口元素模式）
    window_filters: Vec<PropertyFilter>,

    available_windows: Vec<WindowInfo>,
    show_window_panel: bool,

    xpath_text:      String,         // Combined display: "window_selector, element_xpath"
    window_selector: String,         // Window selector only
    element_xpath:   String,         // Element XPath only
    xpath_error:     Option<String>,
    custom_xpath:    bool,
    custom_window_xpath: bool,       // 是否自定义窗口选择器
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

        let (hierarchy, selected_node, xpath_text, window_selector, element_xpath, window_info) = if save_path.exists() {
            match std::fs::read_to_string(&save_path)
                .ok()
                .and_then(|s| serde_json::from_str::<PersistedCapture>(&s).ok())
            {
                Some(c) => {
                    info!("Restored {} nodes from last_capture.json", c.hierarchy.len());
                    // 如果 window_info 为 None，从 hierarchy 的第一个节点提取
                    let window_info = c.window_info.or_else(|| {
                        c.hierarchy.first().map(|node| WindowInfo {
                            title: node.name.clone(),
                            class_name: node.class_name.clone(),
                            process_id: node.process_id,
                            process_name: String::new(), // 无法从 hierarchy 获取
                        })
                    });
                    let xpath_result = xpath::generate(&c.hierarchy, window_info.as_ref());
                    let xpath = format!("{}, {}", xpath_result.window_selector, xpath_result.element_xpath);
                    (c.hierarchy, c.selected_node, xpath, xpath_result.window_selector, xpath_result.element_xpath, window_info)
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
            element_name: String::new(),
            active_tab: ElementTab::Element,
            hierarchy,
            selected_node,
            window_info,
            window_filters: Vec::new(),
            available_windows: Vec::new(),
            show_window_panel: true,
            xpath_text,
            window_selector,
            element_xpath,
            xpath_error: None,
            custom_xpath: false,
            custom_window_xpath: false,
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

    fn mock_capture() -> (Vec<HierarchyNode>, Option<usize>, String, String, String, Option<WindowInfo>) {
        let result = capture::mock();
        let window_info = result.window_info.clone();
        let xpath_result = xpath::generate(&result.hierarchy, window_info.as_ref());
        // Combined for display
        let xpath = format!("{}, {}", xpath_result.window_selector, xpath_result.element_xpath);
        (result.hierarchy, Some(3), xpath, xpath_result.window_selector, xpath_result.element_xpath, window_info)
    }

    fn save_to_file(&self) {
        if self.hierarchy.is_empty() { return; }
        let path = std::env::current_dir().unwrap_or_default().join("last_capture.json");
        let data = PersistedCapture {
            hierarchy:     self.hierarchy.clone(),
            selected_node: self.selected_node,
            xpath_text:    self.xpath_text.clone(),
            window_info:   self.window_info.clone(),
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
            let mut conditions = Vec::new();
            
            // Add ClassName if available
            if !win.class_name.is_empty() {
                conditions.push(format!("@ClassName='{}'", win.class_name));
            }
            
            // Add Name if available
            if !win.title.is_empty() {
                conditions.push(format!("@Name='{}'", win.title));
            }
            
            // Add ProcessName if available
            if !win.process_name.is_empty() {
                conditions.push(format!("@ProcessName='{}'", win.process_name));
            }
            
            if conditions.is_empty() {
                "Window".to_string()
            } else {
                format!("Window[{}]", conditions.join(" and "))
            }
        } else {
            "Window".to_string()
        };
        
        // Generate element XPath from element hierarchy (nodes after Window)
        // 跳过 hierarchy[0]（窗口根节点），因为窗口选择器已经单独处理
        let element_nodes = if self.hierarchy.len() > 1 {
            &self.hierarchy[1..]
        } else {
            &self.hierarchy[0..0]  // empty slice
        };
        let element_xpath = if self.show_simplified {
            xpath::generate_simplified_elements(element_nodes)
        } else {
            xpath::generate_elements(element_nodes)
        };
        
        // Update all three fields
        self.window_selector = window_selector;
        self.element_xpath = element_xpath;
        self.xpath_text = format!("{}, {}", self.window_selector, self.element_xpath);
        self.xpath_error = xpath::lint(&self.xpath_text);
        self.validation  = ValidationResult::Idle;
    }

    /// 从窗口信息初始化过滤器
    fn init_window_filters_from_info(&mut self) {
        if let Some(ref win) = self.window_info {
            self.window_filters = vec![
                PropertyFilter::new("Name", &win.title),
                PropertyFilter::new("ClassName", &win.class_name),
                PropertyFilter::new("ProcessName", &win.process_name),
            ];        }
    }

    /// 从窗口过滤器重新生成窗口选择器
    fn rebuild_window_selector(&mut self) {
        let predicates: Vec<String> = self.window_filters
            .iter()
            .filter_map(|f| f.predicate())
            .collect();        
        if predicates.is_empty() {
            self.window_selector = "Window".to_string();
        } else {
            self.window_selector = format!("Window[{}]", predicates.join(" and "));
        }
        
        // 更新组合 XPath
        self.xpath_text = format!("{}, {}", self.window_selector, self.element_xpath);
        self.xpath_error = xpath::lint(&self.xpath_text);
        self.validation = ValidationResult::Idle;
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

    fn finish_capture_at(&mut self, x: i32, y: i32, mode: CaptureMode, ctx: &egui::Context) {
        // Immediately hide highlight window to prevent interference
        highlight::hide();
            
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
                info!("Window info extracted: class='{}', name='{}', process='{}', pid={}", 
                      win.class_name, win.title, win.process_name, win.process_id);
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
            // No highlight flash after capture - user requested highlight to disappear
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
            
        // Restore cursor to default and focus main window
        ctx.set_cursor_icon(egui::CursorIcon::Default);
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
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
                // Update highlight - persistent display, no flashing
                let highlight_info = HighlightInfo::new(last.rect.clone(), &last.control_type);
                highlight::update_highlight(&highlight_info);
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

    /// 绘制顶部控制栏：元素名称输入框 + 按钮
    fn draw_top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_bar")
            .exact_height(44.0)
            .frame(
                Frame::none()
                    .fill(Color32::from_gray(248))
                    .inner_margin(Margin::symmetric(12.0, 8.0))
                    .stroke(Stroke::new(1.0, C_BORDER)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // 元素名称输入框
                    ui.label(RichText::new("元素名称:").color(C_MUTED).size(11.5));
                    ui.add_space(4.0);
                    ui.add(
                        TextEdit::singleline(&mut self.element_name)
                            .desired_width(160.0)
                            .hint_text("输入名称便于速查"),
                    );
                    
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // 校验元素按钮
                        let val_label = self.validation.label();
                        let val_color = match &self.validation {
                            ValidationResult::Found { .. }              => C_OK,
                            ValidationResult::NotFound | ValidationResult::Error(_) => C_ERR,
                            _                                           => Color32::from_gray(50),
                        };
                        if ui.add(
                            egui::Button::new(RichText::new(&val_label).color(val_color).size(12.0))
                                .stroke(Stroke::new(1.0, val_color))
                                .min_size(Vec2::new(100.0, 28.0)),
                        ).on_hover_text("F7 — 校验当前XPath是否有效").clicked() {
                            self.do_validate();
                        }
                        
                        ui.add_space(8.0);
                        
                        // 重新捕获按钮
                        let (cap_label, cap_color) = match &self.capture_state {
                            CaptureState::WaitingClick { deadline } => {
                                let s = deadline.saturating_duration_since(Instant::now()).as_secs();
                                (format!("等待 {}s", s), C_WARN)
                            }
                            CaptureState::Capturing => ("捕获中…".to_string(), C_WARN),
                            CaptureState::Idle      => ("重新捕获 F4".to_string(), Color32::from_gray(50)),
                        };
                        if ui.add(
                            egui::Button::new(RichText::new(&cap_label).color(cap_color).size(12.0))
                                .min_size(Vec2::new(100.0, 28.0)),
                        ).on_hover_text("F4 — 点击屏幕控件进行捕获").clicked()
                            && self.capture_state == CaptureState::Idle
                        {
                            self.start_capture();
                        }
                    });
                });
            });
    }

    /// 绘制 XPath 预览内容（根据标签页切换，嵌入中央面板）
    fn draw_xpath_preview_content(&mut self, ui: &mut Ui) {
        // 使用 Frame 包裹预览区域
        Frame::none()
            .fill(Color32::from_gray(252))
            .inner_margin(Margin::symmetric(10.0, 6.0))
            .stroke(Stroke::new(1.0, C_BORDER))
            .rounding(Rounding::same(4.0))
            .show(ui, |ui| {
                match self.active_tab {
                    ElementTab::Element => self.draw_element_xpath_content(ui),
                    ElementTab::WindowElement => self.draw_window_xpath_content(ui),
                }
            });
    }

    /// 元素模式：XPath预览内容
    fn draw_element_xpath_content(&mut self, ui: &mut Ui) {
        // 元素 XPath 行（可编辑）
        ui.horizontal(|ui| {
            ui.label(RichText::new("元素 XPath:").color(C_MUTED).size(11.0));
            ui.add_space(4.0);
            if ui.small_button("复制").on_hover_text("复制元素 XPath").clicked() {
                ui.output_mut(|o| o.copied_text = self.element_xpath.clone());
                self.status_msg = "元素 XPath 已复制到剪贴板".to_string();
            }
            if self.custom_xpath {
                if ui.small_button("重置").on_hover_text("回到自动生成的 XPath").clicked() {
                    self.custom_xpath = false;
                    self.rebuild_xpath();
                }
            }
            if let Some(err) = &self.xpath_error {
                ui.add_space(4.0);
                ui.label(RichText::new(format!("⚠ {}", err)).color(C_ERR).size(10.5));
            }
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
            self.xpath_text = format!("{}, {}", self.window_selector, self.element_xpath);
            self.xpath_error = xpath::lint(&self.xpath_text);
            self.validation = ValidationResult::Idle;
        }
    }

    /// 窗口元素模式：XPath预览内容
    fn draw_window_xpath_content(&mut self, ui: &mut Ui) {
        // 窗口选择器（可编辑）
        ui.horizontal(|ui| {
            ui.label(RichText::new("窗口选择器:").color(C_MUTED).size(11.0));
            ui.add_space(4.0);
            if ui.small_button("复制").on_hover_text("复制窗口选择器").clicked() {
                ui.output_mut(|o| o.copied_text = self.window_selector.clone());
                self.status_msg = "窗口选择器已复制到剪贴板".to_string();
            }
            if self.custom_window_xpath {
                if ui.small_button("重置").on_hover_text("回到自动生成的窗口选择器").clicked() {
                    self.custom_window_xpath = false;
                    self.init_window_filters_from_info();
                    self.rebuild_xpath();
                }
                ui.label(RichText::new("[自定义]").color(C_WARN).size(10.0));
            }
        });
        
        let edit_resp = ui.add(
            TextEdit::multiline(&mut self.window_selector)
                .font(egui::TextStyle::Monospace)
                .desired_rows(2)
                .desired_width(ui.available_width())
                .hint_text("Window[@Name='...' and @ClassName='...']")
                .text_color(C_MONO_FG)
                .frame(true),
        );
        if edit_resp.changed() {
            self.custom_window_xpath = true;
            self.xpath_text = format!("{}, {}", self.window_selector, self.element_xpath);
            self.xpath_error = xpath::lint(&self.xpath_text);
            self.validation = ValidationResult::Idle;
        }
    }

    /// 绘制底部按钮区域
    fn draw_bottom_xpath_buttons(&mut self, ctx: &egui::Context) {
         
        egui::TopBottomPanel::bottom("bottom_buttons")
            .exact_height(52.0)
            .frame(
                Frame::none()
                    .fill(Color32::from_gray(245))
                    .inner_margin(Margin::symmetric(12.0, 10.0))
                    .stroke(Stroke::new(1.0, C_BORDER)),
            )
            .show(ctx, |ui| {

                ui.horizontal_centered(|ui| {
                    // 状态消息
                    let msg_color = match &self.validation {
                        ValidationResult::Found { .. }              => C_OK,
                        ValidationResult::NotFound | ValidationResult::Error(_) => C_ERR,
                        ValidationResult::Running                   => C_WARN,
                        _                                           => C_MUTED,
                    };
                    ui.label(RichText::new(&self.status_msg).color(msg_color).size(11.5));

                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        // 历史记录下拉
                        if !self.history.is_empty() {
                            egui::ComboBox::from_id_salt("history_combo_bottom")
                                .selected_text(format!("历史 ({})", self.history.len()))
                                .width(90.0)
                                .show_ui(ui, |ui| {
                                    let mut chosen = None;
                                    for (i, h) in self.history.iter().enumerate() {
                                        let label = if h.len() > 40 {
                                            format!("{}…", &h[..40])
                                        } else {
                                            h.clone()
                                        };                                        if ui.selectable_label(false, label).clicked() {
                                            chosen = Some(i);
                                        }
                                    }
                                    if let Some(i) = chosen {
                                        self.xpath_text = self.history[i].clone();
                                        self.custom_xpath = true;
                                        self.xpath_error = xpath::lint(&self.xpath_text);
                                    }
                                });
                        }
                        
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            // 确定按钮
                            if ui.add(
                                egui::Button::new(RichText::new("确定").color(Color32::WHITE).size(12.0))
                                    .fill(C_OK)
                                    .min_size(Vec2::new(80.0, 30.0)),
                            ).clicked() {
                                self.push_history();
                                self.save_to_file();
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                            
                            ui.add_space(8.0);
                            
                            // 取消按钮
                            if ui.add(
                                egui::Button::new(RichText::new("取消").color(Color32::from_gray(50)).size(12.0))
                                    .min_size(Vec2::new(80.0, 30.0)),
                            ).clicked() {
                                self.save_to_file();
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        });
                    });
                });
            });

                
             egui::TopBottomPanel::bottom("bottom_xpath")
            // .exact_height(52.0)
            .frame(
                Frame::none()
                    .fill(Color32::from_gray(245))
                    .inner_margin(Margin::symmetric(12.0, 10.0))
                    .stroke(Stroke::new(1.0, C_BORDER)),
            )
            .show(ctx, |ui| {

                // ── XPath 预览区域（根据标签页切换内容）──────────────────────────────
                self.draw_xpath_preview_content(ui);
                 });
    }


    // ── Left panel: 根据标签页显示不同内容 ────────────────────────────────────

    fn draw_left_panel(&mut self, ui: &mut Ui) {
        match self.active_tab {
            ElementTab::Element => self.draw_element_tree(ui),
            ElementTab::WindowElement => self.draw_window_tree(ui),
        }
    }

    /// 元素模式：显示元素层级树
    fn draw_element_tree(&mut self, ui: &mut Ui) {
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

    /// 窗口元素模式：显示窗口信息摘要
    fn draw_window_tree(&mut self, ui: &mut Ui) {
        panel_header(ui, "🪟  窗口信息");
        ui.add_space(4.0);

        if let Some(ref win) = self.window_info {
            // 窗口信息摘要
            egui::Frame::none()
                .fill(Color32::from_rgb(239, 246, 255))
                .rounding(Rounding::same(4.0))
                .inner_margin(Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        prop_row(ui, "标题", &win.title);
                        prop_row(ui, "类名", if win.class_name.is_empty() { "(空)" } else { &win.class_name });
                        prop_row(ui, "进程名", if win.process_name.is_empty() { "(空)" } else { &win.process_name });
                        prop_row(ui, "进程ID", &win.process_id.to_string());
                    });
                });
        } else {
            ui.label(RichText::new("尚未选择窗口").color(C_MUTED).italics());
            ui.add_space(4.0);
            ui.label(RichText::new("请先捕获元素").color(C_MUTED).size(10.5));
        }
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

    // ── Right panel: 根据标签页显示不同属性编辑器 ────────────────────────────────────

    fn draw_right_panel(&mut self, ui: &mut Ui) {
        match self.active_tab {
            ElementTab::Element => self.draw_element_properties(ui),
            ElementTab::WindowElement => self.draw_window_properties(ui),
        }
    }

    /// 元素模式：显示元素属性编辑
    fn draw_element_properties(&mut self, ui: &mut Ui) {
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

    /// 窗口元素模式：显示窗口属性过滤器编辑
    fn draw_window_properties(&mut self, ui: &mut Ui) {
        panel_header(ui, "⚙  窗口属性过滤器");
        
        ScrollArea::vertical()
            .id_salt("window_prop_scroll")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                // 确保窗口过滤器已初始化
                if self.window_filters.is_empty() && self.window_info.is_some() {
                    self.init_window_filters_from_info();
                }
                
                if self.window_info.is_none() {
                    ui.add_space(24.0);
                    ui.label(RichText::new("← 请先选择窗口").color(C_MUTED).italics());
                    return;
                }
                
                // ── 属性过滤器表格 ────────────────────────────────────
                egui::Frame::none()
                    .fill(C_PANEL_HDR)
                    .inner_margin(Margin::symmetric(4.0, 2.0))
                    .stroke(Stroke::new(0.5, C_BORDER))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(22.0);
                            col_label(ui, "属性名", 100.0);
                            col_label(ui, "运算符", 80.0);
                            col_label(ui, "值", 0.0);
                        });
                    });

                ui.add_space(2.0);

                let filter_count = self.window_filters.len();
                let mut dirty = false;
                for fi in 0..filter_count {
                    let alt = fi % 2 == 0;
                    let row_color = if alt {
                        Color32::from_rgb(252, 252, 255)
                    } else {
                        Color32::from_rgb(245, 247, 253)
                    };                    egui::Frame::none()
                        .fill(row_color)
                        .inner_margin(Margin::symmetric(4.0, 1.0))
                        .show(ui, |ui| {
                            let filter = &mut self.window_filters[fi];
                            ui.horizontal(|ui| {
                                if ui.checkbox(&mut filter.enabled, "").changed() { dirty = true; }

                                ui.add_sized(
                                    Vec2::new(100.0, 20.0),
                                    egui::Label::new(RichText::new(&filter.name.clone()).size(12.0)),
                                );

                                let old_op = filter.operator.clone();
                                egui::ComboBox::from_id_salt(format!("win_op_{}", fi))
                                    .selected_text(filter.operator.label())
                                    .width(76.0)
                                    .show_ui(ui, |ui| {
                                        for op in Operator::all() {
                                            ui.selectable_value(&mut filter.operator, op.clone(), op.label());
                                        }
                                    });
                                if filter.operator != old_op { dirty = true; }

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
                
                if dirty {
                    self.custom_window_xpath = true;
                    self.rebuild_window_selector();
                }
                
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                
                // ── 窗口选择器预览 ─────────────────────────────────────
                ui.label(RichText::new("窗口选择器:").color(C_MUTED).size(11.0));
                ui.add_space(2.0);
                egui::Frame::none()
                    .fill(Color32::from_rgb(248, 250, 252))
                    .stroke(Stroke::new(0.5, C_BORDER))
                    .rounding(Rounding::same(4.0))
                    .inner_margin(Margin::symmetric(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(&self.window_selector)
                                .font(egui::FontId::monospace(11.0))
                                .color(C_MONO_FG),
                        );
                    });
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
                window_info:   self.window_info.clone(),
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
                // Timeout: hide highlight, restore cursor
                highlight::hide();
                mouse_hook::deactivate_capture();
                self.overlay.hide();
                self.capture_state = CaptureState::Idle;
                self.status_msg = "捕获超时，已取消".to_string();
                ctx.set_cursor_icon(egui::CursorIcon::Default);
            } else if escape {
                // Cancel: hide highlight, restore cursor
                highlight::hide();
                mouse_hook::deactivate_capture();
                self.overlay.hide();
                self.capture_state = CaptureState::Idle;
                self.status_msg = "捕获已取消".to_string();
                ctx.set_cursor_icon(egui::CursorIcon::Default);
            } else if let Some(event) = mouse_hook::poll_click() {
                if event.is_down {
                    let mode = event.capture_mode();
                    if mode != CaptureMode::None {
                        self.finish_capture_at(event.x, event.y, mode, ctx);
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
            // Not in capture mode: ensure highlight is hidden
            highlight::hide();
            self.last_mouse_move = None;
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
        self.draw_top_bar(ctx);
        // draw_xpath_bar removed - XPath preview moved to bottom
        // ── Bottom panel: buttons only ──────────────────────────────
        self.draw_bottom_xpath_buttons(ctx);

        egui::CentralPanel::default()
            .frame(Frame::none().fill(Color32::from_gray(250)))
            .show(ctx, |ui| {
                
                ui.add_space(6.0);

                // ── 标签页切换区域 ──────────────────────────────────────────────────
                ui.horizontal(|ui| {
                    // 标签页切换
                    let element_active = self.active_tab == ElementTab::Element;
                    let window_active = self.active_tab == ElementTab::WindowElement;
                    
                    if ui.selectable_label(element_active, RichText::new("元素").size(12.0)).clicked() {
                        self.active_tab = ElementTab::Element;
                    }
                    ui.add_space(4.0);
                    if ui.selectable_label(window_active, RichText::new("窗口元素").size(12.0)).clicked() {
                        self.active_tab = ElementTab::WindowElement;
                    }
                    
                    ui.add_space(16.0);
                    
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // 自定义XPath开关
                        ui.checkbox(&mut self.custom_xpath, "使用自定义XPath")
                            .on_hover_text("启用自定义XPath编辑模式");
                    });
                });
                
                // ui.add_space(6.0);
                // ui.separator();
                // ui.add_space(4.0);
                
                // // ── XPath 预览区域（根据标签页切换内容）──────────────────────────────
                // self.draw_xpath_preview_content(ui);
                
                // ui.add_space(6.0);
                ui.separator();
                // ui.add_space(4.0);
                
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

                // ui.add_space(6.0);
                // ui.separator();
                // ui.add_space(4.0);
                
                // ── XPath 预览区域（根据标签页切换内容）──────────────────────────────
                //self.draw_xpath_preview_content(ui);
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