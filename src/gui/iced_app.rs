// src/gui/iced_app.rs
//
// iced 0.13 应用主体（替代 SelectorApp + eframe::App）
// 使用 builder API: iced::application("title", update, view).subscription(sub).run()

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text,
    text_input, text_editor, Space,
};
use iced::{
    Alignment, Color, Element, Length, Subscription, Task,
    event, keyboard, stream,
};
use iced::futures::SinkExt;
use iced::widget::pane_grid::{self, PaneGrid, Axis, Configuration};
use log::Level;
use log::info;

use element_selector::core::model::{
    AppConfig, DetailedValidationResult, ElementTab, HistoryEntry,
    HierarchyNode, Operator, PropertyFilter, ValidationResult, WindowInfo,
    SimilarElementSample,
};
use element_selector::core::xpath;
use element_selector::core::{XPathOptimizer, OptimizationResult};
use element_selector::capture;

use super::highlight;
use super::multi_highlight::MultiHighlightManager;
use super::logger::{GuiLogger, init_gui_logger};
use super::input_hook::{self, MouseEvent};
use super::iced_style::ThemeColors;
use super::persistence;
use super::types::*;
use super::capture_overlay::CaptureOverlay;

// ─── Message ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Message {
    // Top bar
    ValidatePressed,
    CapturePressed,
    CancelCapture,
    EscapePressed,
    LogPanelToggled,

    // Tab & options
    TabChanged(ElementTab),

    // XPath editing
    CopyXPath,
    WindowSelectorChanged(String),
    ElementXPathChanged(String),
    XPathTextInput(String),
    XPathSubmitted,
    XPathRestorePressed,
    XPathSyncPressed,

    // Tree interaction
    TreeNodeSelected(usize),
    TreeNodeIncludedToggled(usize),

    // Properties interaction
    FilterOperatorChanged(usize, usize, Operator),
    FilterValueChanged(usize, usize, String),
    FilterEnabledToggled(usize, usize, bool),
    IncludeTogglePressed(usize),

    // Window filters
    WindowFilterEnabled(usize, bool),
    WindowFilterOperatorChanged(usize, Operator),
    WindowFilterValueChanged(usize, String),

    // Actions
    OptimizePressed,
    MinimalOptimizePressed,
    CancelOptimize,
    SimilarSearchPressed,
    CodeDialogOpened,
    CodeDialogClosed,
    CodeFormatChanged(CodeFormat),
    CopyAllCode,
    // History
    ToggleHistoryPanel,
    HistorySearchChanged(String),
    HistoryEntrySelected(usize),
    HistoryEntryDeleted(usize),
    ShowNamingDialog,
    NamingDialogInput(String),
    NamingDialogConfirm,
    NamingDialogCancel,
    ConfirmAndClose,
    CancelAndClose,

    // Async results
    PollBackgroundTasks,
    MouseHookEvent(MouseEvent),
    MouseMoved { x: i32, y: i32 },

    // Similar elements
    SimilarSearchComplete(Vec<capture::CaptureResult>),
    ForceRefreshHighlight,
    CodeEdited(text_editor::Action),
}

// ─── Pane grid content type ──────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaneContent {
    Left,
    Right,
}

// ─── App State ───────────────────────────────────────────────────────────────

pub struct State {
    // UI state
    pub active_tab: ElementTab,
    pub show_code_dialog: bool,
    pub show_log_panel: bool,
    pub generated_ts_code: String,
    pub code_content: text_editor::Content,
    pub code_format: CodeFormat,
    pub copy_status_hint: String,

    // Hierarchy & selection
    pub hierarchy: Vec<HierarchyNode>,
    pub selected_node: Option<usize>,
    pub window_info: Option<WindowInfo>,
    pub window_filters: Vec<PropertyFilter>,
    pub window_filters_initialized: bool,

    // XPath
    pub xpath_text: String,
    pub window_selector: String,
    pub element_xpath: String,
    pub xpath_error: Option<String>,
    pub xpath_source: XPathSource,

    // Validation
    pub validation: ValidationResult,
    pub detailed_validation: Option<DetailedValidationResult>,

    // Capture
    pub capture_state: CaptureState,
    pub overlay: CaptureOverlay,

    // Status & history
    pub status_msg: String,
    pub history: Vec<HistoryEntry>,
    pub show_history_panel: bool,
    pub show_naming_dialog: bool,
    pub naming_dialog_input: String,
    pub pending_history_entry: Option<HistoryEntry>,
    pub history_search_query: String,

    // Layout
    pub panes: pane_grid::State<PaneContent>,

    // Theme
    pub colors: ThemeColors,

    // Logging
    pub gui_logger: GuiLogger,
    pub optimization_in_progress: std::sync::Arc<AtomicBool>,
    pub optimization_rx: Option<std::sync::mpsc::Receiver<Option<OptimizationResult>>>,
    pub optimization_start_time: Option<Instant>,

    // Similar elements
    pub similar_samples: Vec<SimilarElementSample>,
    pub similar_mode_active: bool,
    pub found_similar_elements: Vec<capture::CaptureResult>,
    pub similar_search_rx: Option<std::sync::mpsc::Receiver<Vec<capture::CaptureResult>>>,
    pub similar_search_start_time: Option<Instant>,

    // Common elements (multi-sample)
    pub common_path: Option<element_selector::core::CommonAncestorPath>,
    pub common_search_in_progress: std::sync::Arc<AtomicBool>,
    pub common_search_rx: Option<std::sync::mpsc::Receiver<(u64, Vec<element_selector::api::types::ElementInfo>)>>,
    pub found_common_elements: Vec<element_selector::api::types::ElementInfo>,
    pub common_search_sequence: u64,

    pub multi_highlight_manager: Option<MultiHighlightManager>,
    pub multi_highlight_create_time: Option<Instant>,

    // Validation async
    pub validation_in_progress: std::sync::Arc<AtomicBool>,
    pub validation_rx: Option<std::sync::mpsc::Receiver<Option<DetailedValidationResult>>>,
    pub validation_start_time: Option<Instant>,

    // Highlight async
    pub pending_highlight_rx: Option<std::sync::mpsc::Receiver<(i32, i32, capture::CaptureResult)>>,
    pub cached_hover_result: Option<capture::CaptureResult>,
    pub last_highlighted_element_id: Option<String>,
    pub highlight_query_sequence: u64,
    pub last_highlight_pos: Option<(i32, i32)>,
    pub last_highlight_time: Option<u64>,
    // For debounce: track mouse position stability
    pub last_known_mouse_pos: Option<(i32, i32)>,
    pub last_position_change_ms: Option<u64>,

    // Raw Input ESC 检测
    pub escape_rx: Option<crossbeam_channel::Receiver<()>>,

    // Config
    pub config: AppConfig,
}

impl Default for State {
    fn default() -> Self {
        Self::init().0
    }
}

impl State {
    fn init() -> (Self, Task<Message>) {
        let config = persistence::load_config();

        // Try to restore last capture, track source for status message
        let (hierarchy, selected_node, xpath_text, window_selector, element_xpath, window_info, xpath_source, window_filters_enabled, load_source) =
            if let Some(json) = persistence::load_capture_json() {
                match serde_json::from_str::<PersistedCapture>(&json) {
                    Ok(c) => {
                        let result = Self::restore_from_persisted(c);
                        (result.0, result.1, result.2, result.3, result.4, result.5, result.6, result.7, "loaded")
                    }
                    Err(_) => {
                        let result = Self::mock_capture();
                        (result.0, result.1, result.2, result.3, result.4, result.5, result.6, result.7, "corrupt")
                    }
                }
            } else {
                let result = Self::mock_capture();
                (result.0, result.1, result.2, result.3, result.4, result.5, result.6, result.7, "empty")
            };

        let status_msg = match load_source {
            "loaded" => format!("✓ 已加载上次捕获: {} 层", hierarchy.len()),
            "corrupt" => "上次捕获数据无效，已使用示例数据".to_string(),
            _ => "就绪 — 按 F4 开始捕获元素".to_string(),
        };

        let _n = hierarchy.len();

        // Initialize window filters from restored data
        let (window_filters, window_filters_initialized) = if let Some(ref win) = window_info {
            let mut filters = vec![
                PropertyFilter::new("Name", &win.title),
                PropertyFilter::new("ClassName", &win.class_name),
                PropertyFilter::new("ProcessName", &win.process_name),
            ];
            for (i, f) in filters.iter_mut().enumerate() {
                if i < window_filters_enabled.len() {
                    f.enabled = window_filters_enabled[i];
                }
            }
            (filters, true)
        } else {
            (Vec::new(), false)
        };

        let app = Self {
            active_tab: ElementTab::Element,
            show_code_dialog: false,
            show_log_panel: false,
            generated_ts_code: String::new(),
            code_content: text_editor::Content::new(),
            code_format: CodeFormat::FullChain,
            copy_status_hint: String::new(),

            hierarchy,
            selected_node,
            window_info,
            window_filters,
            window_filters_initialized,

            xpath_text,
            window_selector,
            element_xpath,
            xpath_error: None,
            xpath_source,

            validation: ValidationResult::Idle,
            detailed_validation: None,

            capture_state: CaptureState::Idle,
            overlay: CaptureOverlay::new(),

            status_msg,
            history: config.history.clone(),
            show_history_panel: false,
            show_naming_dialog: false,
            naming_dialog_input: String::new(),
            pending_history_entry: None,
            history_search_query: String::new(),

            panes: pane_grid::State::with_configuration(
                Configuration::Split {
                    axis: Axis::Vertical,
                    ratio: 0.5,
                    a: Box::new(Configuration::Pane(PaneContent::Left)),
                    b: Box::new(Configuration::Pane(PaneContent::Right)),
                },
            ),

            colors: ThemeColors::light(),
            gui_logger: init_gui_logger(1000),
            optimization_in_progress: std::sync::Arc::new(AtomicBool::new(false)),
            optimization_rx: None,
            optimization_start_time: None,

            similar_samples: Vec::new(),
            similar_mode_active: false,
            found_similar_elements: Vec::new(),
            similar_search_rx: None,
            similar_search_start_time: None,

            common_path: None,
            common_search_in_progress: std::sync::Arc::new(AtomicBool::new(false)),
            common_search_rx: None,
            found_common_elements: Vec::new(),
            common_search_sequence: 0,

            multi_highlight_manager: None,
            multi_highlight_create_time: None,

            validation_in_progress: std::sync::Arc::new(AtomicBool::new(false)),
            validation_rx: None,
            validation_start_time: None,

            pending_highlight_rx: None,
            cached_hover_result: None,
            last_highlighted_element_id: None,
            highlight_query_sequence: 0,
            last_highlight_pos: None,
            last_highlight_time: None,
            last_known_mouse_pos: None,
            last_position_change_ms: None,

            escape_rx: Some(super::raw_input::init()),

            config,
        };

        (app, Task::none())
    }

    fn mock_capture() -> (Vec<HierarchyNode>, Option<usize>, String, String, String, Option<WindowInfo>, XPathSource, Vec<bool>) {
        let result = capture::mock();
        let window_info = result.window_info.clone();
        let xpath_result = xpath::generate(&result.hierarchy, window_info.as_ref());
        let xpath = format!("{}, {}", xpath_result.window_selector, xpath_result.element_xpath);
        (result.hierarchy, Some(3), xpath, xpath_result.window_selector, xpath_result.element_xpath, window_info, XPathSource::AutoGenerated, Vec::new())
    }

    fn restore_from_persisted(c: PersistedCapture) -> (Vec<HierarchyNode>, Option<usize>, String, String, String, Option<WindowInfo>, XPathSource, Vec<bool>) {
        let hierarchy = c.hierarchy;
        let window_info = c.window_info.or_else(|| {
            hierarchy.first().map(|node| WindowInfo {
                title: node.name.clone(),
                class_name: node.class_name.clone(),
                process_id: node.process_id,
                process_name: String::new(),
            })
        });
        let xpath_source = match c.xpath_source_kind.as_str() {
            "Optimized" => {
                if let Some(summary) = c.optimization_summary {
                    XPathSource::Optimized(element_selector::core::OptimizationSummary {
                        removed_dynamic_attrs: summary.removed_dynamic_attrs,
                        simplified_attrs: summary.simplified_attrs,
                        used_anchor: summary.used_anchor,
                        anchor_description: None,
                        compression_ratio: 0.0,
                    })
                } else {
                    XPathSource::AutoGenerated
                }
            }
            "Manual" => XPathSource::Manual,
            _ => XPathSource::AutoGenerated,
        };
        let xpath_text = c.xpath_text.clone();
        let (window_selector, element_xpath) = if let Some(comma_pos) = xpath_text.find(", ") {
            (xpath_text[..comma_pos].to_string(), xpath_text[comma_pos + 2..].to_string())
        } else {
            let xpath_result = xpath::generate(&hierarchy, window_info.as_ref());
            (xpath_result.window_selector, xpath_result.element_xpath)
        };
        (hierarchy, c.selected_node, xpath_text, window_selector, element_xpath, window_info, xpath_source, c.window_filters_enabled)
    }

    pub fn save_to_file(&self) {
        if self.hierarchy.is_empty() { return; }
        let window_filters_enabled: Vec<bool> = self.window_filters.iter().map(|f| f.enabled).collect();
        let xpath_source_kind = match &self.xpath_source {
            XPathSource::AutoGenerated => "Auto",
            XPathSource::Optimized(_) => "Optimized",
            XPathSource::Manual => "Manual",
        }.to_string();
        let optimization_summary = self.xpath_source.optimization_summary().map(|s| {
            OptimizationSummaryPersisted {
                removed_dynamic_attrs: s.removed_dynamic_attrs,
                simplified_attrs: s.simplified_attrs,
                used_anchor: s.used_anchor,
            }
        });
        let data = PersistedCapture {
            hierarchy: self.hierarchy.clone(),
            selected_node: self.selected_node,
            xpath_text: self.xpath_text.clone(),
            window_info: self.window_info.clone(),
            window_filters_enabled,
            xpath_source_kind,
            optimization_summary,
        };
        if let Ok(json) = serde_json::to_string_pretty(&data) {
            persistence::save_capture(&json);
        }
    }

    /// Save app config (app_config.json) on exit — mirrors egui `save()`
    pub fn save_config_on_exit(&self) {
        let mut config = self.config.clone();
        config.history = self.history.clone();
        persistence::save_config(&config);
    }

    // ─── XPath helpers ───

    fn rebuild_xpath(&mut self) {
        let window_selector = self.build_window_selector_from_info();
        let element_xpath = xpath::generate_elements(&self.hierarchy);
        self.window_selector = window_selector;
        self.element_xpath = element_xpath;
        self.xpath_text = format!("{}, {}", self.window_selector, self.element_xpath);
        self.xpath_error = xpath::lint(&self.xpath_text);
        self.validation = ValidationResult::Idle;
    }

    /// 解析手动编辑的 XPath，同步到 hierarchy 的 included 状态。
    /// 返回同步结果描述（成功/失败原因）。
    fn sync_xpath_to_tree(&mut self) -> String {
        // 全部重置为 included=false
        for node in &mut self.hierarchy {
            node.included = false;
        }

        let xpath = self.element_xpath.trim();
        if xpath.is_empty() {
            return "XPath 不能为空".to_string();
        }

        // 按 `/` 分割 XPath，提取 segment 列表
        let parts: Vec<&str> = xpath.split('/').collect();
        let mut segments: Vec<String> = Vec::new();
        for part in parts {
            let part = part.trim();
            if part.is_empty() { continue; }
            segments.push(part.to_string());
        }

        if segments.is_empty() {
            return "XPath 格式无效，无法解析任何节点".to_string();
        }

        // 将 segments 按顺序匹配到 hierarchy 节点
        let seg_count = segments.len();
        let mut matched = 0;
        let mut seg_idx = 0;
        for node in &mut self.hierarchy {
            if seg_idx >= segments.len() { break; }

            let seg = &segments[seg_idx];
            // 提取 tag（`[` 之前的部分）
            let tag_end = seg.find(|c| c == '[').unwrap_or(seg.len());
            let tag = seg[..tag_end].trim();

            if tag.is_empty() || node.control_type != tag {
                continue;
            }

            // tag 匹配成功
            node.included = true;
            matched += 1;

            // 提取 predicates 中的属性，更新 filters
            let mut search_from = tag_end;
            while let Some(open) = seg[search_from..].find('[') {
                let open = open + search_from;
                if let Some(close) = seg[open..].find(']') {
                    let close = close + open;
                    let predicate = &seg[open + 1..close];
                    // 提取 @Attr='Value'
                    if let Some(eq_pos) = predicate.find('=') {
                        let attr_part = predicate[..eq_pos].trim();
                        let val_part = predicate[eq_pos + 1..].trim();
                        let attr_name = attr_part.trim_start_matches('@');
                        let val = val_part.trim_matches('\'').trim_matches('"');

                        // 更新对应的 filter
                        for filter in &mut node.filters {
                            if filter.name == attr_name {
                                filter.enabled = true;
                                filter.value = val.to_string();
                                break;
                            }
                        }
                    }
                    search_from = close + 1;
                } else {
                    break;
                }
            }

            seg_idx += 1;
        }

        if matched == 0 {
            return format!("无法匹配任何节点（{} 个 segment 均未找到对应 control_type）", seg_count);
        } else if matched < seg_count {
            return format!("部分同步：匹配 {} / {} 个节点", matched, seg_count);
        } else {
            format!("已同步：匹配 {} 个节点", matched)
        }
    }

    fn build_window_selector_from_info(&self) -> String {
        let Some(ref win) = self.window_info else {
            return "Window".to_string();
        };
        let mut conditions = Vec::new();
        if !win.class_name.is_empty() {
            conditions.push(format!("@ClassName='{}'", win.class_name));
        }
        if !win.title.is_empty() {
            conditions.push(format!("@Name='{}'", win.title));
        }
        if !win.process_name.is_empty() {
            conditions.push(format!("@ProcessName='{}'", win.process_name));
        }
        if conditions.is_empty() {
            "Window".to_string()
        } else {
            format!("Window[{}]", conditions.join(" and "))
        }
    }

    fn build_filters_from_info(win: &WindowInfo) -> Vec<PropertyFilter> {
        vec![
            PropertyFilter::new("Name", &win.title),
            PropertyFilter::new("ClassName", &win.class_name),
            PropertyFilter::new("ProcessName", &win.process_name),
        ]
    }

    fn init_window_filters(&mut self) {
        if let Some(ref win) = self.window_info {
            self.window_filters = Self::build_filters_from_info(win);
            self.window_filters_initialized = true;
        }
    }

    fn rebuild_window_selector(&mut self) {
        let predicates: Vec<String> = self.window_filters
            .iter()
            .filter_map(|f| f.predicate())
            .collect();
        self.window_selector = if predicates.is_empty() {
            "Window".to_string()
        } else {
            format!("Window[{}]", predicates.join(" and "))
        };
        self.xpath_text = format!("{}, {}", self.window_selector, self.element_xpath);
        self.xpath_error = xpath::lint(&self.xpath_text);
        self.validation = ValidationResult::Idle;
    }

    fn prepare_history_entry(&self) -> HistoryEntry {
        HistoryEntry::from_capture(
            &self.xpath_text,
            self.window_info.as_ref(),
            &self.hierarchy,
        )
    }

    fn add_history_entry(&mut self, entry: HistoryEntry) {
        self.history.retain(|h| h.xpath_text != entry.xpath_text);
        self.history.insert(0, entry);
        self.history.truncate(50);
    }

    fn filtered_history(&self) -> Vec<&HistoryEntry> {
        if self.history_search_query.is_empty() {
            self.history.iter().collect()
        } else {
            let q = self.history_search_query.to_lowercase();
            self.history.iter().filter(|e| e.matches_search(&q)).collect()
        }
    }

    // ─── Actions ───

    fn start_capture(&mut self) {
        highlight::hide();
        self.validation = ValidationResult::Idle;
        self.detailed_validation = None;
        self.pending_highlight_rx = None;
        self.last_highlighted_element_id = None;
        self.cached_hover_result = None;
        self.capture_state = CaptureState::WaitingClick {
            deadline: Instant::now() + Duration::from_secs(30),
        };
        self.status_msg = String::from("就绪");
        input_hook::activate_capture();
        self.overlay.show();
    }

    fn cancel_capture(&mut self) {
        highlight::hide();
        if let Some(ref mut manager) = self.multi_highlight_manager {
            manager.clear();
        }
        self.multi_highlight_create_time = None;
        input_hook::deactivate_capture();
        self.overlay.hide();
        self.capture_state = CaptureState::Idle;
        self.status_msg = String::from("捕获已取消");
        self.cached_hover_result = None;
        self.pending_highlight_rx = None;
        self.last_highlighted_element_id = None;
        self.highlight_query_sequence = 0;
        self.last_highlight_pos = None;
        self.last_highlight_time = None;
        self.validation = ValidationResult::Idle;
        self.detailed_validation = None;
        self.similar_mode_active = false;
        self.similar_samples.clear();
        self.found_similar_elements.clear();
        self.similar_search_rx = None;
    }

    fn do_validate(&mut self) {
        // 清除旧的高亮
        if let Some(ref mut manager) = self.multi_highlight_manager {
            manager.clear();
        }
        self.multi_highlight_create_time = None;

        self.xpath_error = xpath::lint(&self.xpath_text);
        if let Some(ref err) = self.xpath_error {
            self.status_msg = format!("XPath 语法错误: {}", err);
            return;
        }
        self.validation = ValidationResult::Running;
        self.status_msg = String::from("正在校验...");
        let window_selector = self.window_selector.clone();
        let element_xpath = self.element_xpath.clone();
        let hierarchy = self.hierarchy.clone();
        let in_progress = self.validation_in_progress.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.validation_rx = Some(rx);
        self.validation_start_time = Some(Instant::now());
        in_progress.store(true, Ordering::SeqCst);
        std::thread::spawn(move || {
            let result = capture::validate_selector_and_xpath_detailed(
                &window_selector, &element_xpath, &hierarchy,
            );
            let _ = tx.send(Some(result));
            in_progress.store(false, Ordering::SeqCst);
        });
    }

    fn do_optimize(&mut self) {

        if self.hierarchy.is_empty() {
            self.status_msg = String::from("没有元素可优化");
            return;
        }
        let optimizer = XPathOptimizer::new();
        let result = optimizer.optimize(&self.hierarchy);
        self.hierarchy = result.optimized_hierarchy.clone();
        let window_selector = self.build_window_selector_from_info();
        let element_xpath = xpath::generate_elements(&self.hierarchy);
        self.window_selector = window_selector;
        self.element_xpath = element_xpath;
        self.xpath_text = format!("{}, {}", self.window_selector, self.element_xpath);
        self.xpath_error = xpath::lint(&self.xpath_text);
        self.validation = ValidationResult::Idle;
        self.xpath_source = XPathSource::Optimized(result.summary.clone());
        let summary = &result.summary;
        self.status_msg = format!(
            "智能优化完成：移除 {} 个动态属性，简化 {} 个属性{}",
            summary.removed_dynamic_attrs,
            summary.simplified_attrs,
            if summary.used_anchor {
                format!("，使用锚点 {}", summary.anchor_description.as_ref().unwrap_or(&String::new()))
            } else {
                String::new()
            }
        );
        self.do_validate();
        self.save_to_file();
    }

    fn do_minimal_optimize(&mut self) {
        if self.hierarchy.is_empty() {
            self.status_msg = String::from("没有可优化的元素层级");
            return;
        }
        self.optimization_in_progress.store(true, Ordering::SeqCst);
        self.status_msg = String::from("正在执行极简优化...");
        self.show_log_panel = true;
        self.gui_logger.clear();
        let start_time = Instant::now();
        let (tx, rx) = std::sync::mpsc::channel();
        let hierarchy = self.hierarchy.clone();
        let window_selector = self.window_selector.clone();
        let optimization_in_progress = self.optimization_in_progress.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let optimizer = XPathOptimizer::new();
                optimizer.optimize_minimal(&hierarchy, &window_selector)
            }));
            optimization_in_progress.store(false, Ordering::SeqCst);
            match result {
                Ok(opt_result) => {
                    let _ = start_time;
                    let _ = tx.send(opt_result);
                }
                Err(_) => {
                    let _ = tx.send(None);
                }
            }
        });
        self.optimization_rx = Some(rx);
        self.optimization_start_time = Some(start_time);
    }

    fn finish_capture_at(&mut self, x: i32, y: i32) {
        highlight::hide();
        if let Some(ref mut manager) = self.multi_highlight_manager {
            manager.clear();
        }
        self.last_highlight_pos = None;
        self.last_highlight_time = None;
        input_hook::deactivate_capture();
        self.overlay.hide();
        self.capture_state = CaptureState::Capturing;

        let result = if let Some(cached) = self.cached_hover_result.take() {
            cached
        } else {
            capture::capture_at(x, y)
        };

        if let Some(err) = &result.error {
            self.status_msg = format!("捕获失败: {}", err);
            self.capture_state = CaptureState::Idle;
            self.pending_highlight_rx = None;
            self.last_highlighted_element_id = None;
            self.validation = ValidationResult::Idle;
            self.detailed_validation = None;
        } else {
            let window_idx = Self::find_window_idx(&result);
            let element_hierarchy: Vec<HierarchyNode> = if window_idx < result.hierarchy.len() - 1 {
                result.hierarchy[window_idx + 1..].to_vec()
            } else {
                Vec::new()
            };
            let n = element_hierarchy.len();
            self.selected_node = n.checked_sub(1);
            self.window_info = result.window_info.clone();
            self.init_window_filters();
            self.status_msg = format!(
                "✓ 已捕获 {} 层 — ({}, {})",
                n, result.cursor_x, result.cursor_y
            );
            self.hierarchy = element_hierarchy;
            self.capture_state = CaptureState::Idle;
            self.xpath_source = XPathSource::AutoGenerated;
            self.validation = ValidationResult::Idle;
            self.detailed_validation = None;
            self.pending_highlight_rx = None;
            self.last_highlighted_element_id = None;
            self.rebuild_xpath();
            self.save_to_file();
            info!("capture done: {}", self.xpath_text);
        }
        self.cached_hover_result = None;
    }

    fn find_window_idx(result: &capture::CaptureResult) -> usize {
        if let Some(ref win) = result.window_info {
            result.hierarchy.iter()
                .position(|n| n.class_name == win.class_name && n.process_id == win.process_id)
                .or_else(|| result.hierarchy.iter().position(|n| n.process_id == win.process_id))
                .or_else(|| result.hierarchy.iter().position(|n| n.control_type == "Window"))
                .unwrap_or(0)
        } else {
            result.hierarchy.iter()
                .position(|n| n.control_type == "Window")
                .unwrap_or(0)
        }
    }

    fn generate_typescript_code(&self) -> String {
        let window_obj = self.build_window_selector_object();
        let element_xpath = &self.element_xpath;
        match self.code_format {
            CodeFormat::FullChain => {
                format!(
                    "const sdk = new SDK();\nconst flow = sdk.flow();\n\n// 激活窗口\nawait flow.window({});\n\n// 查找元素\nconst element = await flow.find(`{}`);",
                    window_obj,
                    escape_backtick(element_xpath)
                )
            }
            CodeFormat::ParamsObject => {
                format!(
                    "// 窗口选择器\nconst windowSelector = {};\n\n// XPath\nconst xpath = `{}`;",
                    window_obj,
                    escape_backtick(element_xpath)
                )
            }
            CodeFormat::XPathOnly => self.element_xpath.clone(),
        }
    }

    fn build_window_selector_object(&self) -> String {
        if let Some(ref _win) = self.window_info {
            let mut fields = Vec::new();
            for filter in &self.window_filters {
                if filter.enabled && !filter.value.is_empty() {
                    let js_key = match filter.name.as_str() {
                        "Name" => "title",
                        "ClassName" => "className",
                        "ProcessName" => "processName",
                        _ => &filter.name.to_lowercase(),
                    };
                    fields.push(format!("{}: `{}`", js_key, escape_backtick(&filter.value)));
                }
            }
            if fields.is_empty() {
                "{}".to_string()
            } else {
                format!("{{ {} }}", fields.join(", "))
            }
        } else {
            "{}".to_string()
        }
    }

    fn highlight_element_at(&mut self, x: i32, y: i32) {
        // 【不再调用 hide_sync】让 update_highlight 在结果到达时无缝切换高亮。
        // hide_sync() 会立即销毁当前高亮，造成 50-100ms 的空白间隙。
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = capture::capture_at(x, y);
            let _ = tx.send((x, y, result));
        });
        self.pending_highlight_rx = Some(rx);
    }

    fn handle_mouse_hook_event(&mut self, event: MouseEvent) {
        if let CaptureState::WaitingClick { deadline } = self.capture_state {
            if Instant::now() > deadline {
                self.status_msg = String::from("捕获超时，已取消");
                self.cancel_capture();
                return;
            }
            match event {
                MouseEvent::LeftClick(x, y) => {
                    self.finish_capture_at(x, y);
                }
                MouseEvent::RightClick(_x, _y) => {
                    // 多选：添加/移除当前高亮元素为样本
                    self.toggle_multi_sample();
                }
                MouseEvent::MiddleClick(_x, _y) => {
                    self.force_refresh_highlight();
                }
                MouseEvent::RightDoubleClick => {
                    self.reset_and_exit();
                }
            }
        }
    }

    fn toggle_multi_sample(&mut self) {
        const MAX_SAMPLES: usize = 5;
        if let Some(result) = self.cached_hover_result.clone() {
            if result.hierarchy.is_empty() {
                return;
            }
            let target_node = result.hierarchy.last().cloned().unwrap();

            // 检查是否已存在（通过 rect + control_type 匹配）
            let existing_idx = self.similar_samples.iter().position(|existing| {
                existing.hierarchy_node.control_type == target_node.control_type
                    && existing.hierarchy_node.rect.x == target_node.rect.x
                    && existing.hierarchy_node.rect.y == target_node.rect.y
            });

            if let Some(idx) = existing_idx {
                // 移除样本
                self.similar_samples.remove(idx);
                if self.similar_samples.is_empty() {
                    self.similar_mode_active = false;
                    if let Some(manager) = &mut self.multi_highlight_manager {
                        manager.clear();
                    }
                    self.found_common_elements.clear();
                    self.common_path = None;
                }
                self.status_msg = format!("移除样本，剩余 {} 个", self.similar_samples.len());

                // 如果样本数 < 2，清除共同元素搜索
                if self.similar_samples.len() < 2 {
                    self.found_common_elements.clear();
                    self.common_path = None;
                    if let Some(manager) = &mut self.multi_highlight_manager {
                        manager.clear();
                    }
                }
            } else {
                // 添加样本
                if self.similar_samples.len() >= MAX_SAMPLES {
                    self.status_msg = format!("样本已达上限（最多 {} 个）", MAX_SAMPLES);
                    return;
                }
                let ancestor_chain = result.hierarchy[..result.hierarchy.len() - 1].to_vec();
                self.similar_samples.push(SimilarElementSample {
                    hierarchy_node: target_node.clone(),
                    ancestor_chain,
                    children_structure: vec![],
                });
                self.similar_mode_active = true;
                self.status_msg = format!("已添加多选样本，共 {} 个", self.similar_samples.len());

                // 样本 >= 2 时自动触发共同元素搜索
                if self.similar_samples.len() >= 2 {
                    self.start_common_search();
                }
            }
        }
    }

    /// 计算共同祖先路径
    fn compute_common_path(&mut self) {
        self.common_path = element_selector::core::extract_common_path(
            &self.similar_samples,
            self.window_info.as_ref(),
            self.config.ignore_numeric_automation_ids,
        );
    }

    /// 启动共同元素搜索
    fn start_common_search(&mut self) {
        self.compute_common_path();

        let Some(ref common) = self.common_path else {
            self.status_msg = "无法提取共同特征".to_string();
            return;
        };

        log::info!("[common_search] 共同路径: target_type='{}', ancestors={}, xpath='{}'",
            common.target_control_type, common.common_ancestors.len(), common.search_xpath);

        // 递增序列号，使旧搜索结果失效
        self.common_search_sequence += 1;
        let seq = self.common_search_sequence;

        let in_progress = self.common_search_in_progress.clone();
        in_progress.store(true, Ordering::SeqCst);

        let window_selector = self.window_selector.clone();
        let xpath = common.search_xpath.clone();

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let results = capture::find_common_elements(&window_selector, &xpath);
            let _ = tx.send((seq, results));
            in_progress.store(false, Ordering::SeqCst);
        });

        self.common_search_rx = Some(rx);
    }

    /// 高亮共同元素
    fn highlight_common_elements(&mut self) {
        if self.found_common_elements.is_empty() {
            return;
        }

        if self.multi_highlight_manager.is_none() {
            self.multi_highlight_manager = Some(MultiHighlightManager::new());
        }

        if let Some(manager) = &mut self.multi_highlight_manager {
            manager.clear();
            for (i, info) in self.found_common_elements.iter().enumerate() {
                let id = format!("common_{}", i);
                let label = format!("{}", i + 1);
                let rect = element_selector::core::model::ElementRect {
                    x: info.rect.x,
                    y: info.rect.y,
                    width: info.rect.width,
                    height: info.rect.height,
                };
                manager.add(&id, &rect, &label);
            }
        }
        self.multi_highlight_create_time = Some(Instant::now());
    }

    fn force_refresh_highlight(&mut self) {
        let (mx, my, _) = input_hook::get_mouse_state();
        self.last_highlight_pos = None;
        self.last_highlighted_element_id = None;
        self.highlight_element_at(mx, my);
    }

    fn reset_and_exit(&mut self) {
        self.similar_samples.clear();
        self.similar_mode_active = false;
        self.found_similar_elements.clear();
        self.cancel_capture();
        self.status_msg = String::from("已退出捕获模式");
    }

    fn poll_background_tasks(&mut self) {
        if let Some(rx) = self.optimization_rx.take() {
            if let Ok(result) = rx.try_recv() {
                if let Some(_start_time) = self.optimization_start_time.take() {
                    match result {
                        Some(opt_result) => {
                            self.hierarchy = opt_result.optimized_hierarchy.clone();
                            self.element_xpath = opt_result.optimized_xpath.clone();
                            self.xpath_text = format!("{}, {}", self.window_selector, self.element_xpath);
                            self.xpath_error = xpath::lint(&self.xpath_text);
                            self.validation = ValidationResult::Idle;
                            self.xpath_source = XPathSource::Optimized(opt_result.summary.clone());
                            let summary = &opt_result.summary;
                            self.status_msg = format!(
                                "✓ 极简优化完成：移除 {} / 简化 {}",
                                summary.removed_dynamic_attrs,
                                summary.simplified_attrs
                            );
                            self.do_validate();
                            self.save_to_file();
                            self.show_log_panel = false;
                        }
                        None => {
                            self.status_msg = String::from("极简优化已取消或失败");
                        }
                    }
                }
            } else {
                self.optimization_rx = Some(rx);
            }
        }

        if let Some(rx) = self.validation_rx.take() {
            if let Ok(result_opt) = rx.try_recv() {
                if let Some(start_time) = self.validation_start_time.take() {
                    let elapsed = start_time.elapsed();
                    match result_opt {
                        Some(detailed_result) => {
                            self.detailed_validation = Some(detailed_result.clone());
                            self.validation = detailed_result.overall.clone();
                            // 统一用 MultiHighlightManager 高亮所有匹配元素（不自动消失）
                            if let ValidationResult::Found { ref rects, .. } = detailed_result.overall {
                                if !rects.is_empty() {
                                    if self.multi_highlight_manager.is_none() {
                                        self.multi_highlight_manager = Some(MultiHighlightManager::new());
                                    }
                                    let manager = self.multi_highlight_manager.as_mut().unwrap();
                                    manager.clear();
                                    for (i, rect) in rects.iter().enumerate() {
                                        let id = format!("validation_{}", i);
                                        let label = format!("{}", i + 1);
                                        manager.add(&id, rect, &label);
                                    }
                                    self.multi_highlight_create_time = Some(Instant::now());
                                }
                            }
                            self.status_msg = match &detailed_result.overall {
                                ValidationResult::Found { count, .. } =>
                                    format!("✓ 校验通过 — 找到 {} 个（{}ms）", count, elapsed.as_millis()),
                                ValidationResult::NotFound => String::from("✗ 未找到匹配元素"),
                                ValidationResult::Error(e) => format!("⚠ {}", e),
                                _ => String::new(),
                            };
                            self.add_history_entry(self.prepare_history_entry());
                        }
                        None => {
                            self.status_msg = String::from("校验失败或超时");
                            self.validation = ValidationResult::Error(String::from("校验超时或失败"));
                        }
                    }
                }
            } else {
                self.validation_rx = Some(rx);
            }
        }

        if let Some(rx) = self.pending_highlight_rx.take() {
            if let Ok((_x, _y, result)) = rx.try_recv() {
                if result.error.is_none() {
                    if let Some(last) = result.hierarchy.last() {
                        let current_element_id = format!(
                            "{}|d{}|{},{}-{}x{}",
                            last.control_type, last.depth_from_window,
                            last.rect.x, last.rect.y, last.rect.width, last.rect.height
                        );
                        let should_show_path = if result.hierarchy.len() >= 2 {
                            let current = &result.hierarchy[result.hierarchy.len() - 1];
                            let parent = &result.hierarchy[result.hierarchy.len() - 2];
                            current.rect.is_visually_overlapping(&parent.rect)
                        } else {
                            false
                        };
                        if should_show_path {
                            use element_selector::core::model::HighlightInfo;
                            let path = build_hierarchy_path(&result.hierarchy);
                            let info = HighlightInfo::new(last.rect.clone(), &path);
                            highlight::update_highlight(&info);
                        } else {
                            use element_selector::core::model::HighlightInfo;
                            let info = HighlightInfo::new(last.rect.clone(), &last.control_type);
                            highlight::update_highlight(&info);
                        }
                        self.last_highlighted_element_id = Some(current_element_id);
                        self.cached_hover_result = Some(result.clone());
                    }
                } else {
                    info!("[悬停高亮] 收到错误结果: {:?}", result.error);
                }
            } else {
                self.pending_highlight_rx = Some(rx);
            }
        }

        if let Some(rx) = self.similar_search_rx.take() {
            if let Ok(results) = rx.try_recv() {
                if let Some(_start_time) = self.similar_search_start_time.take() {
                    self.found_similar_elements = results;
                    let count = self.found_similar_elements.len();
                    self.status_msg = if count >= 2 {
                        format!("✓ 找到 {} 个相似元素", count)
                    } else if count == 1 {
                        String::from("找到 1 个相似元素")
                    } else {
                        String::from("未找到更多相似元素")
                    };
                }
            } else {
                self.similar_search_rx = Some(rx);
            }
        }

        if let Some(rx) = self.common_search_rx.take() {
            if let Ok((seq, results)) = rx.try_recv() {
                // 只处理最新搜索的结果
                if seq == self.common_search_sequence {
                    log::info!("[common_search] 收到结果: {} 个元素 (seq={})", results.len(), seq);
                    self.found_common_elements = results;
                    let count = self.found_common_elements.len();
                    if count > 0 {
                        // 更新 UI XPath 为共同 XPath
                        if let Some(ref common) = self.common_path {
                            self.element_xpath = common.search_xpath.clone();
                            self.xpath_text = format!("{}, {}", self.window_selector, self.element_xpath);
                            self.xpath_source = XPathSource::AutoGenerated;
                        }
                        self.status_msg = format!("✓ 找到 {} 个共同元素", count);
                        log::info!("[common_search] 开始高亮 {} 个元素", count);
                        self.highlight_common_elements();
                    } else {
                        self.status_msg = String::from("未找到共同元素");
                    }
                } else {
                    log::info!("[common_search] 忽略过期结果: seq={} (current={})", seq, self.common_search_sequence);
                }
            } else {
                self.common_search_rx = Some(rx);
            }
        }

        // Raw Input ESC 检测（全局，不受焦点影响）
        if let Some(ref rx) = self.escape_rx {
            if rx.try_recv().is_ok() {
                if self.capture_state != CaptureState::Idle {
                    self.cancel_capture();
                    self.status_msg = String::from("ESC 退出捕获");
                }
            }
        }

        if let CaptureState::WaitingClick { deadline } = self.capture_state {
            if Instant::now() > deadline {
                self.status_msg = String::from("捕获超时，已取消");
                self.cancel_capture();
            }
        }
    }
}

// ─── Update function ─────────────────────────────────────────────────────────

pub fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::ValidatePressed => {
            if let ValidationResult::Found { count, .. } = state.validation {
                if count > 0 {
                    // 清除高亮，恢复按钮
                    if let Some(ref mut manager) = state.multi_highlight_manager {
                        manager.clear();
                    }
                    state.validation = ValidationResult::Idle;
                    state.status_msg = String::from("已清除高亮");
                } else {
                    state.do_validate();
                }
            } else {
                state.do_validate();
            }
        }
        Message::CapturePressed => {
            if state.capture_state == CaptureState::Idle {
                state.start_capture();
            } else {
                state.cancel_capture();
            }
        }
        Message::CancelCapture => {
            if state.capture_state != CaptureState::Idle {
                state.cancel_capture();
            }
        }
        Message::EscapePressed => {
            if state.capture_state != CaptureState::Idle {
                state.cancel_capture();
            }
        }
        Message::LogPanelToggled => {
            state.show_log_panel = !state.show_log_panel;
        }
        Message::TabChanged(tab) => {
            state.active_tab = tab;
        }
        Message::WindowSelectorChanged(val) => {
            state.window_selector = val;
            state.xpath_text = format!("{}, {}", state.window_selector, state.element_xpath);
            state.xpath_error = xpath::lint(&state.xpath_text);
            state.validation = ValidationResult::Idle;
        }
        Message::ElementXPathChanged(val) => {
            state.element_xpath = val;
            state.xpath_text = format!("{}, {}", state.window_selector, state.element_xpath);
            state.xpath_error = xpath::lint(&state.xpath_text);
            state.validation = ValidationResult::Idle;
            state.xpath_source = XPathSource::Manual;
        }
        Message::XPathTextInput(val) => {
            state.element_xpath = val;
            state.xpath_text = format!("{}, {}", state.window_selector, state.element_xpath);
            state.xpath_error = xpath::lint(&state.xpath_text);
            state.validation = ValidationResult::Idle;
            state.xpath_source = XPathSource::Manual;
        }
        Message::XPathSubmitted => {
            let msg = state.sync_xpath_to_tree();
            if msg.starts_with("已同步") || msg.starts_with("部分同步") {
                state.xpath_source = XPathSource::AutoGenerated;
                state.rebuild_xpath();
            }
            state.status_msg = msg;
        }
        Message::XPathRestorePressed => {
            state.xpath_source = XPathSource::AutoGenerated;
            state.rebuild_xpath();
            state.status_msg = "已恢复为自动生成的 XPath".to_string();
        }
        Message::XPathSyncPressed => {
            let msg = state.sync_xpath_to_tree();
            if msg.starts_with("已同步") || msg.starts_with("部分同步") {
                state.xpath_source = XPathSource::AutoGenerated;
                state.rebuild_xpath();
            }
            state.status_msg = msg;
        }
        Message::CopyXPath => {
            state.status_msg = "元素 XPath 已复制到剪贴板".to_string();
            return iced::clipboard::write(state.element_xpath.clone());
        }
        Message::TreeNodeSelected(idx) => {
            state.selected_node = Some(idx);
        }
        Message::TreeNodeIncludedToggled(idx) => {
            if idx < state.hierarchy.len() {
                state.hierarchy[idx].included = !state.hierarchy[idx].included;
                state.rebuild_xpath();
            }
        }
        Message::FilterOperatorChanged(node_idx, filter_idx, op) => {
            if node_idx < state.hierarchy.len() && filter_idx < state.hierarchy[node_idx].filters.len() {
                state.hierarchy[node_idx].filters[filter_idx].operator = op;
                state.rebuild_xpath();
            }
        }
        Message::FilterValueChanged(node_idx, filter_idx, val) => {
            if node_idx < state.hierarchy.len() && filter_idx < state.hierarchy[node_idx].filters.len() {
                state.hierarchy[node_idx].filters[filter_idx].value = val;
                state.rebuild_xpath();
            }
        }
        Message::FilterEnabledToggled(node_idx, filter_idx, enabled) => {
            if node_idx < state.hierarchy.len() && filter_idx < state.hierarchy[node_idx].filters.len() {
                state.hierarchy[node_idx].filters[filter_idx].enabled = enabled;
                state.rebuild_xpath();
            }
        }
        Message::IncludeTogglePressed(idx) => {
            if idx < state.hierarchy.len() {
                state.hierarchy[idx].included = !state.hierarchy[idx].included;
                state.rebuild_xpath();
            }
        }
        Message::WindowFilterEnabled(idx, enabled) => {
            if idx < state.window_filters.len() {
                state.window_filters[idx].enabled = enabled;
                state.rebuild_window_selector();
            }
        }
        Message::WindowFilterOperatorChanged(idx, op) => {
            if idx < state.window_filters.len() {
                state.window_filters[idx].operator = op;
                state.rebuild_window_selector();
            }
        }
        Message::WindowFilterValueChanged(idx, val) => {
            if idx < state.window_filters.len() {
                state.window_filters[idx].value = val;
                state.rebuild_window_selector();
            }
        }
        Message::OptimizePressed => {
            state.do_optimize();
        }
        Message::MinimalOptimizePressed => {
            state.do_minimal_optimize();
        }
        Message::CancelOptimize => {
            state.optimization_in_progress.store(true, Ordering::SeqCst);
            state.status_msg = String::from("正在取消极简优化...");
            info!("[极简优化] 用户请求取消");
        }
        Message::SimilarSearchPressed => {
            if state.similar_samples.len() >= 2 {
                state.start_common_search();
            } else {
                state.status_msg = String::from("请先添加至少 2 个样本元素");
            }
        }
        Message::CodeDialogOpened => {
            state.generated_ts_code = state.generate_typescript_code();
            state.code_content = text_editor::Content::with_text(&state.generated_ts_code);
            state.show_code_dialog = true;
        }
        Message::CodeDialogClosed => {
            state.show_code_dialog = false;
        }
        Message::CodeFormatChanged(fmt) => {
            state.code_format = fmt;
            state.generated_ts_code = state.generate_typescript_code();
            state.code_content = text_editor::Content::with_text(&state.generated_ts_code);
        }
        Message::CodeEdited(action) => {
            state.code_content.perform(action);
        }
        Message::CopyAllCode => {
            state.copy_status_hint = "已复制！".to_string();
            return iced::clipboard::write(state.generated_ts_code.clone());
        }
        Message::ToggleHistoryPanel => {
            state.show_history_panel = !state.show_history_panel;
            if state.show_history_panel {
                state.show_naming_dialog = false;
            }
        }
        Message::HistorySearchChanged(query) => {
            state.history_search_query = query;
        }
        Message::HistoryEntrySelected(filtered_idx) => {
            let filtered = state.filtered_history();
            if filtered_idx < filtered.len() {
                let xpath = filtered[filtered_idx].xpath_text.clone();
                let name = filtered[filtered_idx].name.clone();
                state.xpath_text = xpath;
                if let Some(comma_pos) = state.xpath_text.find(", ") {
                    state.window_selector = state.xpath_text[..comma_pos].to_string();
                    state.element_xpath = state.xpath_text[comma_pos + 2..].to_string();
                }
                state.show_history_panel = false;
                state.status_msg = format!("✓ 已加载历史: {}", name);
            }
        }
        Message::HistoryEntryDeleted(filtered_idx) => {
            let filtered = state.filtered_history();
            if filtered_idx < filtered.len() {
                let xpath_to_remove = filtered[filtered_idx].xpath_text.clone();
                state.history.retain(|h| h.xpath_text != xpath_to_remove);
            }
        }
        Message::ShowNamingDialog => {
            let entry = state.prepare_history_entry();
            state.naming_dialog_input = entry.name.clone();
            state.pending_history_entry = Some(entry);
            state.show_naming_dialog = true;
            state.show_history_panel = false;
        }
        Message::NamingDialogInput(input) => {
            state.naming_dialog_input = input;
        }
        Message::NamingDialogConfirm => {
            if let Some(mut entry) = state.pending_history_entry.take() {
                let name = state.naming_dialog_input.trim().to_string();
                if !name.is_empty() {
                    entry.name = name.chars().take(50).collect();
                    let saved_name = entry.name.clone();
                    state.add_history_entry(entry);
                    state.status_msg = format!("✓ 已保存历史: {}", saved_name);
                }
            }
            state.show_naming_dialog = false;
            state.naming_dialog_input = String::new();
        }
        Message::NamingDialogCancel => {
            state.pending_history_entry = None;
            state.show_naming_dialog = false;
            state.naming_dialog_input = String::new();
        }
        Message::ConfirmAndClose => {
            if let Some(err) = xpath::lint(&state.xpath_text) {
                state.status_msg = format!("⚠ XPath 语法错误，请先修正：{}", err);
                return Task::none();
            }
            if matches!(state.validation, ValidationResult::Idle) {
                state.do_validate();
            }
            match &state.validation {
                ValidationResult::NotFound => {
                    state.status_msg = String::from("校验未通过，元素未找到。如需强制保存请先点击取消");
                    return Task::none();
                }
                ValidationResult::Error(e) => {
                    state.status_msg = format!("校验出错：{}，请检查后再确定", e);
                    return Task::none();
                }
                _ => {}
            }
            state.add_history_entry(state.prepare_history_entry());
            state.save_to_file();
            state.save_config_on_exit();
            return iced::exit();
        }
        Message::CancelAndClose => {
            state.save_config_on_exit();
            state.save_to_file();
            return iced::exit();
        }
        Message::PollBackgroundTasks => {
            state.poll_background_tasks();
        }
        Message::MouseHookEvent(event) => {
            state.handle_mouse_hook_event(event);
        }
        Message::MouseMoved { x: mx, y: my } => {
            if state.capture_state != CaptureState::Idle {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;

                // Reset debounce timer on any position change
                let pos_changed = state.last_known_mouse_pos != Some((mx, my));
                if pos_changed {
                    state.last_position_change_ms = Some(now_ms);
                    state.last_known_mouse_pos = Some((mx, my));
                }

                // Check if mouse is still within current highlight bounds
                // If so, keep the highlight visible (don't hide until new result arrives)
                let mouse_still_in_highlight = if let Some(ref result) = state.cached_hover_result {
                    if let Some(last) = result.hierarchy.last() {
                        let r = &last.rect;
                        mx >= r.x && mx <= r.x + r.width as i32
                            && my >= r.y && my <= r.y + r.height as i32
                    } else {
                        false
                    }
                } else {
                    false
                };

                let stable_for = state.last_position_change_ms
                    .map(|t| now_ms.saturating_sub(t))
                    .unwrap_or(0);

                let has_pending = state.pending_highlight_rx.is_some();

                if stable_for >= 300 {
                    let should_throttle = if let Some(last_t) = state.last_highlight_time {
                        now_ms - last_t < 200
                    } else {
                        false
                    };

                    let pos_changed_for_highlight = if let Some(last) = state.last_highlight_pos {
                        last.0 != mx || last.1 != my
                    } else {
                        true
                    };

                    if !has_pending && !should_throttle && pos_changed_for_highlight {
                        state.highlight_element_at(mx, my);
                        state.last_highlight_pos = Some((mx, my));
                        state.last_highlight_time = Some(now_ms);
                    }
                }

                // If mouse left the highlighted element and no pending query,
                // hide the highlight immediately
                if !mouse_still_in_highlight && !has_pending {
                    highlight::hide();
                    state.cached_hover_result = None;
                    state.last_highlighted_element_id = None;
                }
            }
        }
        Message::SimilarSearchComplete(results) => {
            state.found_similar_elements = results;
        }
        Message::ForceRefreshHighlight => {
            state.force_refresh_highlight();
        }
    }
    Task::none()
}

// ─── View function ───────────────────────────────────────────────────────────

pub fn view(state: &State) -> Element<'_, Message> {
    let colors = &state.colors;

    let top_bar = view_top_bar(state);

    // Tab bar — spans across both panels (like old egui layout)
    let tab_bar = view_tab_bar(state);

    let pane_grid = view_pane_grid(state);

    // XPath preview frame (between pane grid and bottom bar)
    let xpath_frame = view_xpath_frame(state);

    let bottom_bar = view_bottom_bar(state);

    let mut content = column![
        top_bar,
        tab_bar,
        pane_grid,
        xpath_frame,
        bottom_bar,
    ];

    if state.show_code_dialog {
        content = content.push(view_code_dialog(state));
    }

    if state.show_history_panel {
        content = content.push(view_history_panel(state));
    }

    if state.show_naming_dialog {
        content = content.push(view_naming_dialog(state));
    }

    if state.show_log_panel {
        content = content.push(view_log_panel(state));
    }

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(colors.central_bg)),
            ..Default::default()
        })
        .into()
}

// ─── Subscription ────────────────────────────────────────────────────────────

pub fn subscription(state: &State) -> Subscription<Message> {
    let mut subscriptions = vec![
        iced::time::every(Duration::from_millis(100))
            .map(|_| Message::PollBackgroundTasks),

        event::listen_with(|event, _status, _window_id| {
            if let event::Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) = event {
                match key {
                    keyboard::Key::Named(keyboard::key::Named::F4) =>
                        return Some(Message::CapturePressed),
                    keyboard::Key::Named(keyboard::key::Named::F7) =>
                        return Some(Message::ValidatePressed),
                    _ => {}
                }
            }
            None
        }),
    ];

    if state.capture_state != CaptureState::Idle {
        subscriptions.push(mouse_hook_subscription());
        subscriptions.push(mouse_move_subscription());
    }

    Subscription::batch(subscriptions)
}

fn mouse_hook_subscription() -> Subscription<Message> {
    Subscription::run_with_id(
        "mouse_hook",
        stream::channel(100, |mut output| async move {
            loop {
                if let Some(event) = input_hook::poll_mouse_click() {
                    let _ = output.send(Message::MouseHookEvent(event)).await;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }),
    )
}

/// Poll mouse position directly via Win32 GetCursorPos
/// Emits on every poll interval so debounce can work even when mouse is stationary
fn mouse_move_subscription() -> Subscription<Message> {
    Subscription::run_with_id(
        "mouse_move",
        stream::channel(10, |mut output| async move {
            use windows::Win32::Foundation::POINT;
            use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
            loop {
                unsafe {
                    let mut pt = POINT::default();
                    if GetCursorPos(&mut pt).is_ok() {
                        let current = (pt.x, pt.y);
                        let _now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as u64;

                        let _ = output.send(Message::MouseMoved { x: current.0, y: current.1 }).await;
                    }
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }),
    )
}

// ─── View component functions ────────────────────────────────────────────────

fn view_top_bar(state: &State) -> Element<'_, Message> {
    let colors = &state.colors;

    let validate_btn = button(text(validation_button_label(state)).color(if matches!(&state.validation, ValidationResult::Found { .. }) {
            Color::BLACK
        } else {
            colors.text
        }))
        .padding([4, 12])
        .style(move |_, _| validation_button_style(state, colors))
        .on_press(Message::ValidatePressed);

    let capture_btn = button(text(capture_button_label(state)))
        .padding([4, 12])
        .on_press(Message::CapturePressed);

    let log_btn = button(if state.show_log_panel { "隐藏日志" } else { "显示日志" })
        .padding([4, 8])
        .on_press(Message::LogPanelToggled);

    let r = row![Space::with_width(Length::Fill), validate_btn, capture_btn, log_btn]
        .spacing(8)
        .align_y(Alignment::Center)
        .padding([8, 12]);

    container(r)
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(colors.top_bar_bg)),
            border: iced::Border {
                width: 0.0,
                color: colors.border,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn validation_button_label(state: &State) -> String {
    match &state.validation {
        ValidationResult::Found { count, .. } => format!("找到 {} 个", count),
        ValidationResult::Running => String::from("校验中…"),
        ValidationResult::NotFound => String::from("未找到"),
        ValidationResult::Error(_) => String::from("校验出错"),
        ValidationResult::Idle => String::from("校验 F7"),
    }
}

fn validation_button_style(state: &State, colors: &ThemeColors) -> button::Style {
    match &state.validation {
        ValidationResult::Found { .. } => button::Style {
            background: Some(iced::Background::Color(colors.ok)),
            border: iced::Border {
                color: colors.ok,
                width: 1.0,
                radius: 4.0.into(),
            },
            text_color: Color::BLACK,
            shadow: Default::default(),
        },
        ValidationResult::NotFound => button::Style {
            border: iced::Border {
                color: colors.err,
                width: 1.0,
                radius: 4.0.into(),
            },
            text_color: colors.err,
            ..Default::default()
        },
        _ => button::Style {
            border: iced::Border {
                color: colors.border,
                width: 1.0,
                radius: 4.0.into(),
            },
            text_color: colors.text,
            ..Default::default()
        },
    }
}

fn capture_button_label(state: &State) -> String {
    match &state.capture_state {
        CaptureState::Idle => String::from("捕获 F4"),
        CaptureState::WaitingClick { deadline } => {
            let remaining = deadline.saturating_duration_since(Instant::now());
            format!("取消捕获 ({:.0}s)", remaining.as_secs())
        }
        CaptureState::Capturing => String::from("捕获中..."),
    }
}

fn view_tab_bar(state: &State) -> Element<'_, Message> {
    let colors = &state.colors;

    let element_tab = button("元素")
        .padding([4, 16])
        .style(move |_status, _theme| {
            if state.active_tab == ElementTab::Element {
                button::Style {
                    background: Some(iced::Background::Color(colors.sel_bg)),
                    text_color: colors.sel_fg,
                    ..Default::default()
                }
            } else {
                button::Style::default()
            }
        })
        .on_press(Message::TabChanged(ElementTab::Element));

    let window_tab = button("窗口元素")
        .padding([4, 16])
        .style(move |_status, _theme| {
            if state.active_tab == ElementTab::WindowElement {
                button::Style {
                    background: Some(iced::Background::Color(colors.sel_bg)),
                    text_color: colors.sel_fg,
                    ..Default::default()
                }
            } else {
                button::Style::default()
            }
        })
        .on_press(Message::TabChanged(ElementTab::WindowElement));

    row![element_tab, window_tab]
        .spacing(4)
        .into()
}

fn view_pane_grid(state: &State) -> Element<'_, Message> {
    PaneGrid::new(&state.panes, |_pane, content, _is_maximized| {
        let content = match content {
            PaneContent::Left => view_left_panel(state),
            PaneContent::Right => view_right_panel(state),
        };
        pane_grid::Content::new(content)
    })
    .on_resize(8, |_| Message::LogPanelToggled)
    .width(Length::Fill)
    .height(Length::Fill)
    .spacing(8)
    .into()
}

fn view_xpath_frame(state: &State) -> Element<'_, Message> {
    let colors = &state.colors;

    let xpath_editable = matches!(state.xpath_source, XPathSource::Manual);

    let optimize_btn = button("智能优化")
        .padding([2, 8])
        .on_press(Message::OptimizePressed);

    let minimal_btn = if state.optimization_in_progress.load(Ordering::SeqCst) {
        button("❌ 取消")
            .padding([2, 8])
            .on_press(Message::CancelOptimize)
    } else {
        button("极简优化")
            .padding([2, 8])
            .on_press(Message::MinimalOptimizePressed)
    };

    let code_btn = button("生成 TS 代码")
        .padding([2, 8])
        .on_press(Message::CodeDialogOpened);

    let copy_btn = button("复制")
        .padding([2, 8])
        .on_press(Message::CopyXPath);

    // Top row: label + buttons + status indicators
    let mut top_items: Vec<Element<'_, Message>> = vec![
        text("元素 XPath:").size(11).color(colors.muted).into(),
        copy_btn.into(),
        optimize_btn.into(),
        minimal_btn.into(),
        code_btn.into(),
    ];

    // 恢复 / 同步按钮（始终可见）
    let restore_btn = button("恢复").padding([2, 6])
        .on_press(Message::XPathRestorePressed);
    let sync_btn = button("同步").padding([2, 6])
        .on_press(Message::XPathSyncPressed);
    top_items.push(restore_btn.into());
    top_items.push(sync_btn.into());

    if xpath_editable {
        top_items.push(text("[手动编辑]").size(10).color(colors.warn).into());
    }

    if let Some(ref summary) = state.xpath_source.optimization_summary() {
        let opt_text = format!(
            "已优化：移除 {} / 简化 {}{}",
            summary.removed_dynamic_attrs,
            summary.simplified_attrs,
            if summary.used_anchor { " · 锚点" } else { "" }
        );
        top_items.push(text(opt_text).size(10).color(colors.ok).into());
    }

    if let Some(ref err) = state.xpath_error {
        top_items.push(text(format!("⚠ {}", err)).size(10).color(colors.err).into());
    }

    let top_row = row(top_items)
        .spacing(6)
        .align_y(Alignment::Center);

    // XPath text area (editable — on_submit triggers sync to tree checkboxes)
    let xpath_input = text_input("XPath...", &state.element_xpath)
        .size(11)
        .padding([4, 8])
        .width(Length::Fill)
        .on_input(Message::XPathTextInput)
        .on_submit(Message::XPathSubmitted);
    let xpath_area: Element<'_, Message> = container(xpath_input).into();

    container(column![
        top_row,
        xpath_area,
    ].spacing(4))
    .padding([6, 12])
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(colors.preview_bg)),
        border: iced::Border {
            width: 1.0,
            color: colors.border,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn view_left_panel(state: &State) -> Element<'_, Message> {
    let colors = &state.colors;

    let header_text = match state.active_tab {
        ElementTab::Element => "📂 元素层级结构",
        ElementTab::WindowElement => "🪟 窗口信息",
    };
    let header: Element<'_, Message> = container(
        text(header_text)
            .size(13)
            .color(colors.hdr_text)
    ).into();

    let content = match state.active_tab {
        ElementTab::Element => view_element_tree(state),
        ElementTab::WindowElement => view_window_info(state),
    };

    container(column![header, content].spacing(4))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding([4, 4])
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(colors.panel_fill)),
            ..Default::default()
        })
        .into()
}

fn view_window_info(state: &State) -> Element<'_, Message> {
    let colors = &state.colors;

    let header = text("窗口信息")
        .size(13)
        .color(colors.hdr_text);

    if let Some(ref win) = state.window_info {
        let pid_str = win.process_id.to_string();
        let mut props = Vec::new();
        for (l, v) in &[
            ("标题", win.title.as_str()),
            ("类名", win.class_name.as_str()),
            ("进程名", win.process_name.as_str()),
            ("PID", pid_str.as_str()),
        ] {
            let lr = l.to_string();
            let vr = v.to_string();
            props.push(
                row![
                    text(lr).size(12).color(colors.muted).width(Length::Fixed(70.0)),
                    text(vr).size(12).color(colors.text),
                ]
                .spacing(8)
                .into()
            );
        }

        let info_frame = container(column(props).spacing(4))
            .padding([8, 10])
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(colors.info_bg)),
                border: iced::Border {
                    width: 0.0,
                    color: iced::Color::TRANSPARENT,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });

        column![header, info_frame].spacing(8).padding(8).into()
    } else {
        column![header, text("无窗口信息").color(colors.muted)]
            .spacing(8)
            .padding(8)
            .into()
    }
}

fn view_element_tree(state: &State) -> Element<'_, Message> {
    let colors = &state.colors;

    if state.hierarchy.is_empty() {
        return container(text("尚未捕获元素").color(colors.muted))
            .padding(20)
            .center_x(Length::Fill)
            .into();
    }

    let mut rows = Vec::new();
    for (idx, node) in state.hierarchy.iter().enumerate() {
        let label = node.tree_label();
        let is_selected = state.selected_node == Some(idx);
        let is_target = idx == state.hierarchy.len() - 1;

        let cb = checkbox("", node.included)
            .on_toggle(move |_| Message::TreeNodeIncludedToggled(idx));

        // Label color matching old egui theme
        let label_color = if is_target {
            colors.target_fg
        } else if is_selected {
            colors.sel_fg
        } else if !node.included {
            colors.muted
        } else {
            colors.text
        };

        // Transparent button — no bg, no hover highlight, just text
        let btn = button(text(label).color(label_color))
            .padding(2)
            .style(move |_status, _theme| {
                let mut style = button::Style::default();
                style.background = None;
                style.text_color = label_color;
                style
            })
            .on_press(Message::TreeNodeSelected(idx));

        let r = row![cb, btn]
            .spacing(4)
            .align_y(Alignment::Center);

        // Row background highlight (like old egui)
        let row_with_bg = container(r)
            .width(Length::Fill)
            .padding([1, 2])
            .style(move |_| {
                if is_selected {
                    container::Style {
                        background: Some(iced::Background::Color(colors.sel_bg)),
                        ..Default::default()
                    }
                } else {
                    container::Style::default()
                }
            });

        rows.push(row_with_bg.into());
    }

    scrollable(column(rows))
        .height(Length::Fill)
        .into()
}

fn view_right_panel(state: &State) -> Element<'_, Message> {
    let colors = &state.colors;

    let header_text = match state.active_tab {
        ElementTab::Element => "⚙ 元素属性",
        ElementTab::WindowElement => "⚙ 窗口属性过滤器",
    };
    let header: Element<'_, Message> = container(
        text(header_text)
            .size(13)
            .color(colors.hdr_text)
    ).into();

    let props = match state.active_tab {
        ElementTab::Element => view_properties(state),
        ElementTab::WindowElement => view_window_properties(state),
    };

    let mut items: Vec<Element<'_, Message>> = vec![header, props];

    // Validation details (if any)
    if let Some(ref detail) = state.detailed_validation {
        items.push(view_validation_detail(detail, colors).into());
    }

    container(column(items).spacing(8).padding(8))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(colors.panel_fill)),
            ..Default::default()
        })
        .into()
}

fn view_window_properties(state: &State) -> Element<'_, Message> {
    let colors = &state.colors;

    let mut filter_rows = Vec::new();

    // Column header row
    let header_row = row![
        Space::with_width(Length::Fixed(22.0)), // space for checkbox
        text("属性名").size(11).color(colors.muted).width(Length::Fixed(90.0)),
        text("运算符").size(11).color(colors.muted).width(Length::Fixed(80.0)),
        text("值").size(11).color(colors.muted),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let header_container = container(header_row)
        .padding([2, 4])
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(colors.panel_hdr)),
            ..Default::default()
        });

    filter_rows.push(header_container.into());

    for (f_idx, filter) in state.window_filters.iter().enumerate() {
        let cb = checkbox("", filter.enabled)
            .on_toggle(move |_| Message::WindowFilterEnabled(f_idx, !filter.enabled));

        let label = text(&filter.name).width(Length::Fixed(90.0));

        let operators: Vec<_> = Operator::all().iter().cloned().collect();
        let pick = pick_list(operators, Some(filter.operator.clone()), move |op| {
            Message::WindowFilterOperatorChanged(f_idx, op)
        }).width(Length::Fixed(76.0));

        let input = text_input("值", &filter.value)
            .on_input(move |v| Message::WindowFilterValueChanged(f_idx, v))
            .width(Length::Fill);

        let r = row![cb, label, pick, input]
            .spacing(4)
            .align_y(Alignment::Center)
            .padding([1, 4]);

        filter_rows.push(r.into());
    }

    let selector_code = container(
        text(&state.window_selector)
            .size(11)
            .color(colors.mono_fg)
    )
    .padding(6)
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(colors.segment_bg)),
        ..Default::default()
    });

    column![column(filter_rows).spacing(2), selector_code]
        .spacing(8)
        .into()
}

fn view_validation_detail<'a>(detail: &'a DetailedValidationResult, colors: &'a ThemeColors) -> Element<'a, Message> {
    let status_text = match &detail.overall {
        ValidationResult::Found { count, .. } =>
            text(format!("✓ 通过 — 找到 {} 个元素", count)).color(colors.ok),
        ValidationResult::NotFound =>
            text("✗ 未找到匹配元素").color(colors.err),
        ValidationResult::Error(e) =>
            text(format!("⚠ 错误: {}", e)).color(colors.warn_detail_fg),
        _ => text("未校验").color(colors.muted),
    };

    let elapsed_text = text(format!("用时: {}ms", detail.total_duration_ms))
        .size(11)
        .color(colors.muted);

    let frame_bg = if matches!(detail.overall, ValidationResult::NotFound) {
        colors.val_notfound_bg
    } else {
        colors.val_found_bg
    };

    let mut items: Vec<Element<'a, Message>> = vec![
        text("🔍 校验结果").size(13).color(colors.hdr_text).into(),
        status_text.into(),
        elapsed_text.into(),
    ];

    // Show failed steps when not found
    if matches!(detail.overall, ValidationResult::NotFound) {
        items.push(text("失败步骤分析:").size(11).color(colors.muted).into());
        for (_si, seg) in detail.segments.iter().enumerate() {
            if seg.matched || seg.match_count > 0 {
                continue;
            }
            let mut step_items: Vec<Element<'a, Message>> = vec![
                text(format!("第 {} 步失败:", seg.segment_index + 1))
                    .size(11)
                    .color(colors.warn_detail_fg)
                    .into(),
                text(&seg.segment_text)
                    .size(10)
                    .color(colors.mono_fg)
                    .into(),
            ];
            for pf in &seg.predicate_failures {
                let actual_val = if let Some(ref actual) = pf.actual_value {
                    format!("期望 '{}' vs 实际 '{}'", pf.expected_value, actual)
                } else {
                    format!("期望 '{}'", pf.expected_value)
                };
                step_items.push(
                    text(format!("{}: {}", pf.attr_name, actual_val))
                        .size(10)
                        .color(colors.muted)
                        .into(),
                );
                if !pf.reason.is_empty() {
                    step_items.push(
                        text(&pf.reason).size(10).color(colors.muted).into(),
                    );
                }
            }
            let step_frame = container(column(step_items).spacing(2))
                .padding(8)
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(colors.fail_step_bg)),
                    border: iced::Border {
                        width: 0.0,
                        color: iced::Color::TRANSPARENT,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                });
            items.push(step_frame.into());
        }
    }

    container(column(items).spacing(4))
    .padding(8)
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(frame_bg)),
        border: iced::Border {
            width: 0.0,
            color: iced::Color::TRANSPARENT,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn view_properties(state: &State) -> Element<'_, Message> {
    let colors = &state.colors;

    if state.hierarchy.is_empty() {
        return container(text("无属性").color(colors.muted)).into();
    }

    let selected = state.selected_node.unwrap_or(state.hierarchy.len() - 1);
    if selected >= state.hierarchy.len() {
        return container(text("节点无效").color(colors.muted)).into();
    }

    let node = &state.hierarchy[selected];

    // Node summary
    let dims = format!("{}×{}  @({}, {})", node.rect.width, node.rect.height, node.rect.x, node.rect.y);
    let pid_str = if node.process_id > 0 { Some(format!("pid:{}", node.process_id)) } else { None };
    let mut summary_items: Vec<Element<'_, Message>> = vec![
        text("控件类型").size(11).color(colors.muted).into(),
        text(&node.control_type).size(13).color(colors.target_fg).into(),
    ];
    if node.rect.width > 0 {
        summary_items.push(text(dims.clone()).size(10).color(colors.muted).into());
    }
    if let Some(pid) = pid_str {
        summary_items.push(text(pid).size(10).color(colors.muted).into());
    }
    let summary = container(row(summary_items).spacing(6))
        .padding([6, 10])
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(colors.info_bg)),
            border: iced::Border {
                width: 0.0,
                color: iced::Color::TRANSPARENT,
                radius: 4.0.into(),
            },
            ..Default::default()
        });

    // Filter column header
    let header_row = row![
        Space::with_width(Length::Fixed(22.0)),
        text("属性名").size(11).color(colors.muted).width(Length::Fixed(90.0)),
        text("运算符").size(11).color(colors.muted).width(Length::Fixed(80.0)),
        text("值").size(11).color(colors.muted),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let header_container = container(header_row)
        .padding([2, 4])
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(colors.panel_hdr)),
            ..Default::default()
        });

    let mut filter_rows: Vec<Element<'_, Message>> = vec![header_container.into()];

    for (f_idx, filter) in node.filters.iter().enumerate() {
        let cb = checkbox("", filter.enabled)
            .on_toggle(move |_| Message::FilterEnabledToggled(selected, f_idx, !filter.enabled));

        let label = text(&filter.name).width(Length::Fixed(90.0));

        let operators: Vec<_> = Operator::all().iter().cloned().collect();
        let pick = pick_list(operators, Some(filter.operator.clone()), move |op| {
            Message::FilterOperatorChanged(selected, f_idx, op)
        }).width(Length::Fixed(76.0));

        let input = text_input("值", &filter.value)
            .on_input(move |v| Message::FilterValueChanged(selected, f_idx, v))
            .width(Length::Fill);

        let r = row![cb, label, pick, input]
            .spacing(4)
            .align_y(Alignment::Center)
            .padding([1, 4]);

        filter_rows.push(r.into());
    }

    // XPath segment preview (no monospace — use default for CJK support)
    let segment_block = container(
        text(node.xpath_segment())
            .size(11)
            .color(colors.mono_fg)
    )
    .padding(6)
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(colors.segment_bg)),
        border: iced::Border {
            width: 0.5,
            color: colors.border,
            radius: 4.0.into(),
        },
        ..Default::default()
    });

    let segment_label = text("本节点 XPath 片段:").size(11).color(colors.muted);

    // Include/exclude button
    let include_label = if node.included { "排除此节点" } else { "包含此节点" };
    let include_btn = button(include_label)
        .padding([4, 12])
        .on_press(Message::IncludeTogglePressed(selected));

    let props_content = column![
        summary,
        column(filter_rows).spacing(2),
        segment_label,
        segment_block,
        include_btn,
    ].spacing(8);

    scrollable(props_content)
        .height(Length::Fill)
        .into()
}

fn view_bottom_bar(state: &State) -> Element<'_, Message> {
    let colors = &state.colors;

    let status = text(&state.status_msg)
        .size(12)
        .color(colors.text);

    let history_btn = button(text(format!("历史 ({})", state.history.len())).size(12))
        .padding([4, 12])
        .width(Length::Fixed(90.0))
        .height(Length::Fixed(32.0))
        .style(move |_, _| button::Style {
            border: iced::Border {
                color: colors.border,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .on_press(Message::ToggleHistoryPanel);

    let save_btn = button(text("保存并命名").size(12).color(Color::BLACK))
        .padding([4, 12])
        .width(Length::Fixed(90.0))
        .height(Length::Fixed(32.0))
        .style(move |_, _| button::Style {
            background: Some(iced::Background::Color(colors.ok)),
            border: iced::Border {
                color: colors.ok,
                width: 1.0,
                radius: 4.0.into(),
            },
            text_color: Color::BLACK,
            ..Default::default()
        })
        .on_press(Message::ShowNamingDialog);

    let cancel_btn = button(text("取消").size(13).color(colors.muted))
        .padding([4, 16])
        .width(Length::Fixed(90.0))
        .height(Length::Fixed(32.0))
        .style(move |_, _| button::Style {
            border: iced::Border {
                color: colors.border,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .on_press(Message::CancelAndClose);

    let (confirm_color, confirm_bg) = match &state.validation {
        ValidationResult::Found { .. } => (Color::BLACK, Some(iced::Background::Color(colors.ok))),
        _ => (colors.text, None),
    };
    let confirm_btn = button(text("确定").size(13).color(confirm_color))
        .padding([4, 16])
        .width(Length::Fixed(90.0))
        .height(Length::Fixed(32.0))
        .style(move |_, _| button::Style {
            background: confirm_bg,
            border: iced::Border {
                color: if confirm_bg.is_some() { colors.ok } else { colors.border },
                width: 1.0,
                radius: 4.0.into(),
            },
            shadow: Default::default(),
            text_color: confirm_color,
        })
        .on_press(Message::ConfirmAndClose);

    container(row![status, Space::with_width(Length::Fill), history_btn, save_btn, cancel_btn, confirm_btn]
        .spacing(8)
        .align_y(Alignment::Center)
        .padding([6, 12]),
    )
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(colors.bottom_bg)),
        ..Default::default()
    })
    .into()
}

fn view_code_dialog(state: &State) -> Element<'_, Message> {
    let colors = &state.colors;

    let formats = vec![CodeFormat::FullChain, CodeFormat::ParamsObject, CodeFormat::XPathOnly];
    let format_pick = pick_list(formats, Some(state.code_format.clone()), Message::CodeFormatChanged)
        .width(Length::Fixed(150.0));

    let header = row![
        text("TypeScript 代码生成").size(14),
        Space::with_width(Length::Fill),
        text("格式:").size(11).color(colors.muted),
        format_pick,
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let code_editor = text_editor(&state.code_content)
        .height(280)
        .on_action(Message::CodeEdited);

    let hint_text = if state.copy_status_hint.is_empty() {
        "拖拽选中可部分复制 · Ctrl+A 全选 · Ctrl+C 复制"
    } else {
        &state.copy_status_hint
    };

    let copy_btn = button(text("复制全部").color(colors.text))
        .padding([4, 16])
        .height(Length::Fixed(32.0))
        .style(move |_, _| button::Style {
            background: Some(iced::Background::Color(colors.ok)),
            border: iced::Border {
                color: colors.ok,
                width: 1.0,
                radius: 4.0.into(),
            },
            text_color: Color::BLACK,
            shadow: Default::default(),
        })
        .on_press(Message::CopyAllCode);

    let ok_btn = button(text("确定").size(13).color(colors.muted))
        .padding([4, 16])
        .width(Length::Fixed(80.0))
        .height(Length::Fixed(32.0))
        .style(move |_, _| button::Style {
            border: iced::Border {
                color: colors.border,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .on_press(Message::CodeDialogClosed);

    let footer = row![
        text(hint_text).size(11).color(colors.muted),
        Space::with_width(Length::Fill),
        copy_btn,
        ok_btn,
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    container(column![
        header,
        code_editor,
        footer,
    ].spacing(8))
    .padding(12)
    .width(Length::Fixed(620.0))
    .height(Length::Shrink)
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(colors.panel_fill)),
        border: iced::Border {
            color: colors.border,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    })
    .into()
}

// ─── History Panel ───────────────────────────────────────────────────────────

fn view_history_panel(state: &State) -> Element<'_, Message> {
    let colors = &state.colors;

    let header = row![
        text("历史记录").size(14).color(colors.text),
        Space::with_width(Length::Fill),
        button(text("×").size(16).color(colors.muted))
            .padding([2, 8])
            .style(move |_, _| button::Style {
                border: iced::Border {
                    color: colors.border,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .on_press(Message::ToggleHistoryPanel),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let search_input = text_input("搜索名称、窗口、元素类型...", &state.history_search_query)
        .on_input(Message::HistorySearchChanged)
        .padding(6)
        .width(Length::Fill);

    let filtered = state.filtered_history();

    let list_content: Element<'_, Message> = if filtered.is_empty() {
        text(if state.history.is_empty() {
            "暂无历史记录"
        } else {
            "无匹配结果"
        })
        .size(13)
        .color(colors.muted)
        .into()
    } else {
        let mut rows = column![].spacing(4);
        for (filtered_idx, entry) in filtered.iter().enumerate() {
            let name_text = text(&entry.name).size(13).color(colors.text);
            let meta_text = text(format!(
                "{}  {}  {}",
                entry.window_title, entry.control_type, entry.display_time()
            ))
            .size(11)
            .color(colors.muted);

            let delete_btn = button(text("删").size(11).color(colors.err))
                .padding([2, 8])
                .style(move |_, _| button::Style {
                    border: iced::Border {
                        color: colors.err,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
                .on_press(Message::HistoryEntryDeleted(filtered_idx));

            let entry_row = row![
                column![name_text, meta_text].spacing(2),
                Space::with_width(Length::Fill),
                delete_btn,
            ]
            .spacing(8)
            .align_y(Alignment::Center);

            let entry_container = container(entry_row)
                .padding([8, 12])
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(Color::TRANSPARENT)),
                    border: iced::Border {
                        color: colors.border,
                        width: 0.5,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                });

            // Use button to make the whole row clickable
            let entry_btn = button(entry_container)
                .padding(0)
                .style(move |_, _| button::Style {
                    border: iced::Border::default(),
                    background: None,
                    shadow: Default::default(),
                    text_color: colors.text,
                })
                .on_press(Message::HistoryEntrySelected(filtered_idx));

            rows = rows.push(entry_btn);
        }
        scrollable(rows).height(Length::Fill).into()
    };

    container(column![
        header,
        search_input,
        list_content,
    ].spacing(8))
    .padding(12)
    .width(Length::Fixed(480.0))
    .height(Length::Fixed(400.0))
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(colors.panel_fill)),
        border: iced::Border {
            color: colors.border,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    })
    .into()
}

// ─── Naming Dialog ───────────────────────────────────────────────────────────

fn view_naming_dialog(state: &State) -> Element<'_, Message> {
    let colors = &state.colors;

    let header = text("保存到历史记录").size(14).color(colors.text);

    let xpath_preview = container(
        text(state.xpath_text.chars().take(80).collect::<String>())
            .size(11)
            .color(colors.muted),
    )
    .padding(6)
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(colors.segment_bg)),
        border: iced::Border {
            color: colors.border,
            width: 0.5,
            radius: 4.0.into(),
        },
        ..Default::default()
    });

    let name_input = text_input("输入名称（如：Button in 微信）", &state.naming_dialog_input)
        .on_input(Message::NamingDialogInput)
        .on_submit(Message::NamingDialogConfirm)
        .padding(6)
        .width(Length::Fill);

    let cancel_btn = button(text("取消").size(13).color(colors.muted))
        .padding([4, 20])
        .height(Length::Fixed(32.0))
        .style(move |_, _| button::Style {
            border: iced::Border {
                color: colors.border,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .on_press(Message::NamingDialogCancel);

    let save_btn = button(text("保存").size(13).color(Color::BLACK))
        .padding([4, 20])
        .height(Length::Fixed(32.0))
        .style(move |_, _| button::Style {
            background: Some(iced::Background::Color(colors.ok)),
            border: iced::Border {
                color: colors.ok,
                width: 1.0,
                radius: 4.0.into(),
            },
            text_color: Color::BLACK,
            ..Default::default()
        })
        .on_press(Message::NamingDialogConfirm);

    let footer = row![
        Space::with_width(Length::Fill),
        cancel_btn,
        save_btn,
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    container(column![
        header,
        xpath_preview,
        name_input,
        footer,
    ].spacing(12))
    .padding(16)
    .width(Length::Fixed(400.0))
    .height(Length::Shrink)
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(colors.panel_fill)),
        border: iced::Border {
            color: colors.border,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn view_log_panel(state: &State) -> Element<'_, Message> {
    let colors = &state.colors;
    let entries = state.gui_logger.get_logs();

    let mut rows = Vec::new();
    for entry in entries.iter().rev().take(200) {
        let time_str = format_system_time(entry.timestamp);
        let level_str = match entry.level {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        };
        let line = text(format!("[{}] {} {}", time_str, level_str, entry.message))
            .size(10)
            .color(colors.muted);
        rows.push(line.into());
    }

    let clear_btn = button("清空")
        .padding([2, 8])
        .on_press(Message::LogPanelToggled);

    container(column![
        row![text("日志").size(13), clear_btn].spacing(8),
        scrollable(column(rows)).height(Length::Fixed(200.0)),
    ].spacing(4))
    .padding(8)
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(colors.panel_fill)),
        ..Default::default()
    })
    .into()
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn escape_backtick(s: &str) -> String {
    s.replace('`', "\\`")
}

fn build_hierarchy_path(hierarchy: &[HierarchyNode]) -> String {
    if hierarchy.is_empty() {
        return String::new();
    }
    let max_display = 5;
    let total = hierarchy.len();
    if total <= max_display {
        hierarchy.iter()
            .map(|n| n.control_type.as_str())
            .collect::<Vec<_>>()
            .join(" › ")
    } else {
        let mut parts = Vec::new();
        parts.push(hierarchy.last().unwrap().control_type.as_str());
        let mut meaningful_count = 0;
        for node in hierarchy.iter().rev().skip(1).take(total - 2) {
            if !node.name.is_empty() || !node.automation_id.is_empty() {
                parts.insert(0, node.control_type.as_str());
                meaningful_count += 1;
                if meaningful_count >= 3 {
                    break;
                }
            }
        }
        if meaningful_count >= 3 {
            parts.insert(1, "...");
        }
        if parts.last() != Some(&hierarchy[0].control_type.as_str()) {
            parts.push(hierarchy[0].control_type.as_str());
        }
        parts.join(" › ")
    }
}

fn format_system_time(t: std::time::SystemTime) -> String {
    t.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| {
            let secs = d.as_secs();
            let millis = d.subsec_millis();
            format!("{:02}:{:02}:{:02}.{:03}",
                (secs / 3600) % 24,
                (secs / 60) % 60,
                secs % 60,
                millis)
        })
        .unwrap_or_default()
}

