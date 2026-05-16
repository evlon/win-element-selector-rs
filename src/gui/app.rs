// src/gui/app.rs
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Frame, Key, Layout, RichText, Sense, Stroke, Ui,
};
use log::info;

use element_selector::core::model::{
    AppConfig, DetailedValidationResult, ElementTab, HighlightInfo,
    HierarchyNode, PropertyFilter,
    ValidationResult, WindowInfo,
};
use element_selector::core::xpath;
use element_selector::core::{XPathOptimizer, OptimizationResult};
use element_selector::capture;

use super::capture_overlay::CaptureOverlay;
use super::highlight;
use super::logger::{GuiLogger, init_gui_logger};
use super::mouse_hook::{self, CaptureMode};

// 引用重构后的独立模块
use super::theme::Theme;
use super::types::*;
use super::helpers::*;
use super::layout::LayoutComponents;



// ─── App ──────────────────────────────────────────────────────────────────────
pub struct SelectorApp {
    pub element_name:      String,
    pub active_tab:        ElementTab,

    pub hierarchy:         Vec<HierarchyNode>,
    pub selected_node:     Option<usize>,
    pub window_info:       Option<WindowInfo>,
    pub window_filters:    Vec<PropertyFilter>,
    /// 窗口过滤器是否已为当前 window_info 初始化过
    pub window_filters_initialized: bool,

    #[allow(dead_code)]
    pub available_windows: Vec<WindowInfo>,
    #[allow(dead_code)]
    pub show_window_panel: bool,

    pub xpath_text:          String,
    pub window_selector:     String,
    pub element_xpath:       String,
    pub xpath_error:         Option<String>,
    /// 窗口选择器是否由用户手动编辑
    pub custom_window_xpath: bool,
    /// XPath 来源（替代 custom_xpath bool + optimization_summary Option）
    pub xpath_source:        XPathSource,
    pub show_simplified:     bool,

    pub validation:          ValidationResult,
    pub detailed_validation: Option<DetailedValidationResult>,
    pub capture_state:       CaptureState,
    pub overlay:             CaptureOverlay,

    pub status_msg:   String,
    pub history:      Vec<String>,
    pub pending_save: bool,

    pub config:        AppConfig,
    #[allow(dead_code)]
    pub countdown_str: String,

    pub last_mouse_move:    Option<Instant>,
    pub last_highlight_pos: Option<(i32, i32)>,
    pub last_highlight_time: Option<u64>,  // Timestamp of last highlight operation (in ms)

    pub node_expanded:    Vec<bool>,
    pub left_panel_width: f32,
    pub divider_dragging: bool,

    pub theme: Theme,
    
    // TypeScript 代码生成相关
    pub show_code_dialog: bool,
    pub generated_ts_code: String,
    pub code_format: CodeFormat,
    
    // 极简优化相关
    pub gui_logger: GuiLogger,
    pub show_log_panel: bool,
    pub optimization_in_progress: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub optimization_rx: Option<std::sync::mpsc::Receiver<Option<OptimizationResult>>>,
    pub optimization_start_time: Option<std::time::Instant>,
    pub log_panel_auto_close_time: Option<std::time::Instant>,  // 【新增】日志窗口自动关闭时间
}

impl SelectorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let dark = cc.egui_ctx.global_style().visuals.dark_mode;
        let theme = Theme::new(dark);

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

        let (hierarchy, selected_node, xpath_text, window_selector, element_xpath, window_info, persisted_state) =
            if save_path.exists() {
                match std::fs::read_to_string(&save_path)
                    .ok()
                    .and_then(|s| serde_json::from_str::<PersistedCapture>(&s).ok())
                {
                    Some(c) => {
                        info!("Restored {} nodes from last_capture.json", c.hierarchy.len());
                        let window_info = c.window_info.or_else(|| {
                            c.hierarchy.first().map(|node| WindowInfo {
                                title:        node.name.clone(),
                                class_name:   node.class_name.clone(),
                                process_id:   node.process_id,
                                process_name: String::new(),
                            })
                        });
                        // 恢复每个节点的状态
                        let mut hierarchy = c.hierarchy;
                        for (i, node) in hierarchy.iter_mut().enumerate() {
                            // 恢复 included 状态
                            if i < c.node_included.len() {
                                node.included = c.node_included[i];
                            }
                            // 恢复 filters enabled 状态
                            if i < c.node_filters_enabled.len() {
                                for (j, enabled) in c.node_filters_enabled[i].iter().enumerate() {
                                    if j < node.filters.len() {
                                        node.filters[j].enabled = *enabled;
                                    }
                                }
                            }
                        }
                        // 恢复 XPath 来源状态
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
                        // 直接使用保存的 xpath_text，而不是重新生成
                        // 因为用户可能已经做了智能优化（修改了 included 状态等）
                        let xpath_text = c.xpath_text.clone();
                        // 从保存的 xpath_text 中解析 window_selector 和 element_xpath
                        let (window_selector, element_xpath) = if let Some(comma_pos) = xpath_text.find(", ") {
                            (xpath_text[..comma_pos].to_string(), xpath_text[comma_pos + 2..].to_string())
                        } else {
                            // Fallback: 重新生成
                            let xpath_result = xpath::generate(&hierarchy, window_info.as_ref());
                            (xpath_result.window_selector, xpath_result.element_xpath)
                        };
                        (hierarchy, c.selected_node, xpath_text, window_selector, element_xpath, window_info, (c.window_filters_enabled, xpath_source))
                    }
                    None => {
                        info!("Failed to parse last_capture.json, using mock");
                        let (h, s, x, ws, ex, wi) = Self::mock_capture();
                        (h, s, x, ws, ex, wi, (Vec::new(), XPathSource::AutoGenerated))
                    }
                }
            } else {
                info!("No last_capture.json, using mock");
                let (h, s, x, ws, ex, wi) = Self::mock_capture();
                (h, s, x, ws, ex, wi, (Vec::new(), XPathSource::AutoGenerated))
            };

        // 从 window_info 构建初始过滤器，并恢复 enabled 状态
        let (window_filters, window_filters_initialized) = if let Some(ref win) = window_info {
            let mut filters = Self::build_filters_from_info(win);
            // 恢复 enabled 状态
            for (i, enabled) in persisted_state.0.iter().enumerate() {
                if i < filters.len() {
                    filters[i].enabled = *enabled;
                }
            }
            (filters, true)
        } else {
            (Vec::new(), false)
        };

        let n = hierarchy.len();
        Self {
            element_name: String::new(),
            active_tab: ElementTab::Element,
            hierarchy,
            selected_node,
            window_info,
            window_filters,
            window_filters_initialized,
            available_windows: Vec::new(),
            show_window_panel: true,
            xpath_text,
            window_selector,
            element_xpath,
            xpath_error: None,
            custom_window_xpath: false,
            xpath_source: persisted_state.1,
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
            last_highlight_time: None,
            node_expanded: vec![true; n],
            left_panel_width: 300.0,
            divider_dragging: false,
            theme,
            
            // TypeScript 代码生成相关
            show_code_dialog: false,
            generated_ts_code: String::new(),
            code_format: CodeFormat::FullChain,
            
            // 极简优化相关
            gui_logger: init_gui_logger(1000),
            show_log_panel: false,
            optimization_in_progress: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            optimization_rx: None,
            optimization_start_time: None,
            log_panel_auto_close_time: None,  // 【新增】初始化
        }
    }

    fn mock_capture() -> (Vec<HierarchyNode>, Option<usize>, String, String, String, Option<WindowInfo>) {
        let result = capture::mock();
        let window_info = result.window_info.clone();
        let xpath_result = xpath::generate(&result.hierarchy, window_info.as_ref());
        let xpath = format!("{}, {}", xpath_result.window_selector, xpath_result.element_xpath);
        (result.hierarchy, Some(3), xpath, xpath_result.window_selector, xpath_result.element_xpath, window_info)
    }

    pub fn save_to_file(&self) {
        if self.hierarchy.is_empty() { return; }
        let path = std::env::current_dir().unwrap_or_default().join("last_capture.json");
        let data = self.build_persisted_capture();
        if let Ok(json) = serde_json::to_string_pretty(&data) {
            let _ = std::fs::write(&path, json);
            info!("Saved capture to {}", path.display());
        }
    }

    /// 构建 PersistedCapture（包含完整的用户状态）
    fn build_persisted_capture(&self) -> PersistedCapture {
        // 提取每个节点的过滤器 enabled 状态
        let node_filters_enabled: Vec<Vec<bool>> = self.hierarchy.iter()
            .map(|node| node.filters.iter().map(|f| f.enabled).collect())
            .collect();
        // 提取每个节点的 included 状态
        let node_included: Vec<bool> = self.hierarchy.iter().map(|node| node.included).collect();
        // 提取窗口过滤器 enabled 状态
        let window_filters_enabled: Vec<bool> = self.window_filters.iter().map(|f| f.enabled).collect();
        // XPath 来源类型
        let xpath_source_kind = match &self.xpath_source {
            XPathSource::AutoGenerated => "Auto",
            XPathSource::Optimized(_) => "Optimized",
            XPathSource::Manual => "Manual",
        }.to_string();
        // 优化摘要
        let optimization_summary = self.xpath_source.optimization_summary().map(|s| OptimizationSummaryPersisted {
            removed_dynamic_attrs: s.removed_dynamic_attrs,
            simplified_attrs: s.simplified_attrs,
            used_anchor: s.used_anchor,
        });

        PersistedCapture {
            hierarchy: self.hierarchy.clone(),
            selected_node: self.selected_node,
            xpath_text: self.xpath_text.clone(),
            window_info: self.window_info.clone(),
            node_filters_enabled,
            node_included,
            window_filters_enabled,
            xpath_source_kind,
            optimization_summary,
        }
    }

    // ── XPath helpers ─────────────────────────────────────────────────────────

    pub fn rebuild_xpath(&mut self) {
        // 只有 AutoGenerated 状态才自动重建，Optimized / Manual 保留当前内容
        if !self.xpath_source.is_auto() { return; }

        let window_selector = self.build_window_selector_from_info();
        let element_xpath = if self.show_simplified {
            xpath::generate_simplified_elements(&self.hierarchy)
        } else {
            xpath::generate_elements(&self.hierarchy)
        };

        self.window_selector = window_selector;
        self.element_xpath   = element_xpath;
        self.xpath_text      = format!("{}, {}", self.window_selector, self.element_xpath);
        self.xpath_error     = xpath::lint(&self.xpath_text);
        self.validation      = ValidationResult::Idle;
    }

    // ── TypeScript 代码生成 ────────────────────────────────────────────────────

    /// 生成 TypeScript 代码
    pub fn generate_typescript_code(&self) -> String {
        match self.code_format {
            CodeFormat::FullChain => self.generate_full_chain_format(),
            CodeFormat::ParamsObject => self.generate_params_object_format(),
            CodeFormat::XPathOnly => self.generate_xpath_only_format(),
        }
    }

    /// 完整链式调用格式
    /// 适用场景：从头开始编写新脚本
    fn generate_full_chain_format(&self) -> String {
        let window_obj = self.build_window_selector_object();
        let element_xpath = &self.element_xpath;
        
        format!(
            "const sdk = new SDK();\nconst flow = sdk.flow();\n\n// 激活窗口\nawait flow.window({});\n\n// 查找元素\nconst element = await flow.find(`{}`);",
            window_obj,
            escape_backtick(element_xpath)
        )
    }

    /// 参数对象格式
    /// 适用场景：在已有代码中插入新步骤
    fn generate_params_object_format(&self) -> String {
        let window_obj = self.build_window_selector_object();
        let element_xpath = &self.element_xpath;
        
        format!(
            "// 窗口选择器\nconst windowSelector = {};\n\n// XPath\nconst xpath = `{}`;",
            window_obj,
            escape_backtick(element_xpath)
        )
    }

    /// 仅 XPath 格式
    /// 适用场景：快速复制 XPath 用于其他用途
    fn generate_xpath_only_format(&self) -> String {
        self.element_xpath.clone()
    }

    /// 构建窗口选择器对象（公共方法）
    fn build_window_selector_object(&self) -> String {
        if let Some(ref _win) = self.window_info {
            let mut fields = Vec::new();
            
            // 根据 window_filters 的 enabled 状态决定包含哪些字段
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

    /// 打开代码生成对话框
    pub fn open_code_dialog(&mut self) {
        self.generated_ts_code = self.generate_typescript_code();
        self.show_code_dialog = true;
    }

    /// 关闭代码生成对话框
    pub fn close_code_dialog(&mut self) {
        self.show_code_dialog = false;
    }

    /// 复制代码到剪贴板
    pub fn copy_code_to_clipboard(&self, ctx: &egui::Context) {
        ctx.copy_text(self.generated_ts_code.clone());
    }

    /// 从 window_info 构建窗口选择器字符串
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

    /// 从 WindowInfo 构建 PropertyFilter 列表（纯函数，不依赖 &mut self）
    fn build_filters_from_info(win: &WindowInfo) -> Vec<PropertyFilter> {
        vec![
            PropertyFilter::new("Name",        &win.title),
            PropertyFilter::new("ClassName",   &win.class_name),
            PropertyFilter::new("ProcessName", &win.process_name),
        ]
    }

    /// 初始化窗口过滤器（仅在 window_info 变更时调用，而非每帧 draw）
    pub fn init_window_filters(&mut self) {
        if let Some(ref win) = self.window_info {
            self.window_filters             = Self::build_filters_from_info(win);
            self.window_filters_initialized = true;
            self.custom_window_xpath        = false;
        }
    }

    /// 从窗口过滤器重新生成窗口选择器
    pub fn rebuild_window_selector(&mut self) {
        let predicates: Vec<String> = self.window_filters
            .iter()
            .filter_map(|f| f.predicate())
            .collect();
        self.window_selector = if predicates.is_empty() {
            "Window".to_string()
        } else {
            format!("Window[{}]", predicates.join(" and "))
        };
        self.xpath_text  = format!("{}, {}", self.window_selector, self.element_xpath);
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

    pub fn start_capture(&mut self) {
        self.capture_state = CaptureState::WaitingClick {
            deadline: Instant::now() + Duration::from_secs(30),
        };
        self.status_msg = "请点击目标控件（Esc 取消）…".to_string();
        mouse_hook::activate_capture(true);
        self.overlay.show();
    }

    pub fn cancel_capture(&mut self, ctx: &egui::Context) {
        highlight::hide();
        mouse_hook::deactivate_capture();
        self.overlay.hide();
        self.capture_state = CaptureState::Idle;
        self.status_msg    = "捕获已取消".to_string();
        ctx.set_cursor_icon(egui::CursorIcon::Default);
    }

    fn finish_capture_at(&mut self, x: i32, y: i32, mode: CaptureMode, ctx: &egui::Context) {
        highlight::hide();
        mouse_hook::deactivate_capture();
        self.overlay.hide();
        self.capture_state = CaptureState::Capturing;

        if let CaptureMode::Batch = mode {
            self.status_msg = "批量捕获模式：正在分析相似元素…".to_string();
        }

        // 【关键修复】频繁移动窗口后，COM 对象可能进入半失效状态
        // 策略：每次捕获前强制重置 IUIAutomation 实例，避免使用 stale 对象
        use element_selector::core::uia::windows_impl::{ComManager, UiaExecutor};
        
        // 1. 确保 COM STA 初始化
        match ComManager::ensure_sta() {
            Ok(true) => {
                log::debug!("COM STA verified");
            }
            Ok(false) => {
                log::warn!("Thread in MTA mode, attempting recovery...");
                if let Err(e) = ComManager::safe_reinitialize() {
                    info!("COM recovery failed: {}", e);
                    self.status_msg = format!("COM 恢复失败: {}", e);
                    self.capture_state = CaptureState::Idle;
                    ctx.set_cursor_icon(egui::CursorIcon::Default);
                    return;
                }
            }
            Err(e) => {
                info!("COM STA check failed: {}", e);
                self.status_msg = format!("COM 检查失败: {}", e);
                self.capture_state = CaptureState::Idle;
                ctx.set_cursor_icon(egui::CursorIcon::Default);
                return;
            }
        }
        
        // 2. 强制重置 IUIAutomation 实例（避免使用 stale 对象）
        UiaExecutor::force_reset_automation();
        log::debug!("IUIAutomation instance reset before capture");

        // 3. 执行捕获操作（带重试机制）
        let capture_result = UiaExecutor::execute_with_retry(
            || Ok(capture::capture_at(x, y)),
            2  // 最多重试 2 次
        );

        match capture_result {
            Ok(result) => {
                if let Some(err) = &result.error {
                    self.status_msg = format!("捕获失败: {}", err);
                } else {
                    let window_idx = Self::find_window_idx(&result);

                    info!("捕获 hierarchy 共 {} 个节点:", result.hierarchy.len());
                    for (i, n) in result.hierarchy.iter().enumerate() {
                        info!("  [{}] {} class='{}' name='{}' pid={}", i, n.control_type, n.class_name, n.name, n.process_id);
                    }
                    if let Some(ref win) = result.window_info {
                        info!("Window info: class='{}', name='{}', process='{}', pid={}",
                              win.class_name, win.title, win.process_name, win.process_id);
                    }
                    info!("窗口节点定位: window_idx = {}", window_idx);

                    let element_hierarchy: Vec<HierarchyNode> = if window_idx < result.hierarchy.len() - 1 {
                        result.hierarchy[window_idx + 1..].to_vec()
                    } else {
                        Vec::new()
                    };

                    let n = element_hierarchy.len();
                    self.selected_node = n.checked_sub(1);
                    self.window_info   = result.window_info.clone();

                    // window_info 变更时同步初始化过滤器
                    self.init_window_filters();

                    let window_hint = result.window_info
                        .as_ref()
                        .map(|w| format!(" [窗口: {}]", w.title))
                        .unwrap_or_default();
                    self.status_msg = format!(
                        "已捕获 {} 层层级 — 坐标 ({}, {}){}",
                        n, result.cursor_x, result.cursor_y, window_hint
                    );

                    self.node_expanded = vec![true; n];
                    self.hierarchy     = element_hierarchy;
                }
            }
            Err(e) => {
                info!("Capture failed after retries: {}", e);
                self.status_msg = format!("捕获失败: {}", e);
                self.capture_state = CaptureState::Idle;
            }
        }

        self.capture_state = CaptureState::Idle;
        self.xpath_source  = XPathSource::AutoGenerated;
        self.validation    = ValidationResult::Idle;
        self.rebuild_xpath();
        self.pending_save = true;
        self.save_to_file();
        info!("capture done: {}", self.xpath_text);

        ctx.set_cursor_icon(egui::CursorIcon::Default);
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    /// 从 capture 结果中定位窗口节点索引
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

    pub fn do_validate(&mut self) {
        self.xpath_error = xpath::lint(&self.xpath_text);
        if let Some(err) = &self.xpath_error {
            self.status_msg = format!("XPath 语法错误: {}", err);
            return;
        }
        self.validation = ValidationResult::Running;

        let detailed_result = capture::validate_selector_and_xpath_detailed(
            &self.window_selector,
            &self.element_xpath,
            &self.hierarchy,
        );
        self.detailed_validation = Some(detailed_result.clone());
        self.validation = detailed_result.overall.clone();

        if let ValidationResult::Found { ref first_rect, .. } = detailed_result.overall {
            if let Some(r) = first_rect { highlight::flash(r, 1200); }
        }
        self.status_msg = match &detailed_result.overall {
            ValidationResult::Found { count, .. } =>
                format!("校验通过 ✔ — 找到 {} 个匹配元素 (总用时: {}ms)", count, detailed_result.total_duration_ms),
            ValidationResult::NotFound =>
                format!("校验失败 — 未找到匹配元素 (总用时: {}ms)", detailed_result.total_duration_ms),
            ValidationResult::Error(e) => format!("校验错误: {}", e),
            _ => String::new(),
        };
        self.push_history();
    }

    pub fn do_optimize(&mut self) {
        if self.hierarchy.is_empty() {
            self.status_msg = "没有元素可优化".to_string();
            return;
        }

        info!("[智能优化] 开始优化，节点数: {}", self.hierarchy.len());
        info!("[智能优化] 优化前 XPath: {}", self.element_xpath);

        let optimizer = XPathOptimizer::new();
        let result    = optimizer.optimize(&self.hierarchy);

        info!("[智能优化] 完成：移除 {} 个动态属性，简化 {} 个属性",
            result.summary.removed_dynamic_attrs,
            result.summary.simplified_attrs);

        // 更新 hierarchy
        self.hierarchy = result.optimized_hierarchy.clone();

        let window_selector = self.build_window_selector_from_info();
        let element_xpath = if self.show_simplified {
            xpath::generate_simplified_elements(&self.hierarchy)
        } else {
            xpath::generate_elements(&self.hierarchy)
        };
        self.window_selector = window_selector;
        self.element_xpath   = element_xpath;
        self.xpath_text      = format!("{}, {}", self.window_selector, self.element_xpath);
        self.xpath_error     = xpath::lint(&self.xpath_text);
        self.validation      = ValidationResult::Idle;

        // 标记为已优化状态
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

        info!("[智能优化] 优化后 XPath: {}", self.element_xpath);
        self.do_validate();
        self.save_to_file();
    }
    
    /// 执行极简优化
    pub fn do_minimal_optimize(&mut self) {
        if self.hierarchy.is_empty() {
            self.status_msg = "没有可优化的元素层级".to_string();
            return;
        }
        
        // 设置优化进行中标志
        use std::sync::atomic::Ordering;
        self.optimization_in_progress.store(true, Ordering::SeqCst);
        self.status_msg = "正在执行极简优化...".to_string();
        
        // 自动打开日志面板
        self.show_log_panel = true;
        
        // 清空旧日志，只显示本次优化的日志
        self.gui_logger.clear();
        
        // 记录开始时间
        let start_time = std::time::Instant::now();
        
        log::info!("========================================");
        log::info!("[极简优化] 用户触发极简优化");
        log::info!("========================================");
        
        // 【关键修复】使用 channel 在后台线程执行优化，实时传递日志和结果
        let (tx, rx) = std::sync::mpsc::channel();
        
        let hierarchy = self.hierarchy.clone();
        let window_selector = self.window_selector.clone();
        let optimization_in_progress = self.optimization_in_progress.clone();
        
        std::thread::spawn(move || {
            // 【关键修复】捕获后台线程的 panic，避免程序崩溃
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // 执行极简优化
                let optimizer = XPathOptimizer::new();
                optimizer.optimize_minimal(
                    &hierarchy,
                    &window_selector,
                )
            }));
            
            // 清除优化进行中标志
            optimization_in_progress.store(false, Ordering::SeqCst);
            
            match result {
                Ok(opt_result) => {
                    let elapsed = start_time.elapsed();
                    
                    log::info!("========================================");
                    log::info!("[极简优化] 优化流程结束，总耗时: {:.1}s", elapsed.as_secs_f64());
                    log::info!("========================================");
                    
                    // 发送结果到主线程
                    let _ = tx.send(opt_result);
                }
                Err(panic_info) => {
                    log::error!("[极简优化] 后台线程 panic: {:?}", panic_info);
                    log::error!("[极简优化] 优化失败，请查看控制台输出");
                    
                    // 发送 None 表示失败
                    let _ = tx.send(None);
                }
            }
        });
        
        // 将 receiver 存储到 App 状态，在 update 中检查
        self.optimization_rx = Some(rx);
        self.optimization_start_time = Some(start_time);
    }
    
    /// 取消极简优化
    pub fn cancel_minimal_optimize(&mut self) {
        use std::sync::atomic::Ordering;
        self.optimization_in_progress.store(true, Ordering::SeqCst);
        self.status_msg = "正在取消极简优化...".to_string();
        log::info!("[极简优化] 用户请求取消");
    }

    pub fn do_confirm_and_close(&mut self, ctx: &egui::Context) {
        // 1. 语法检查
        if let Some(err) = xpath::lint(&self.xpath_text) {
            self.status_msg = format!("⚠ XPath 语法错误，请先修正：{}", err);
            return;
        }
        // 2. 若未校验过，先自动校验
        if matches!(self.validation, ValidationResult::Idle) {
            self.do_validate();
        }
        // 3. 校验失败时阻止关闭
        match &self.validation {
            ValidationResult::NotFound => {
                self.status_msg = "校验未通过，元素未找到。如需强制保存请先点击取消".to_string();
                return;
            }
            ValidationResult::Error(e) => {
                self.status_msg = format!("校验出错：{}，请检查后再确定", e);
                return;
            }
            _ => {}
        }
        self.push_history();
        self.save_to_file();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn highlight_element_at(&mut self, x: i32, y: i32) {
        let result = capture::capture_at(x, y);
        if result.error.is_none() {
            if let Some(last) = result.hierarchy.last() {
                let highlight_info = HighlightInfo::new(last.rect.clone(), &last.control_type);
                highlight::update_highlight(&highlight_info);
            }
        }
    }

    // ── Panels ────────────────────────────────────────────────────────────────

    // UI 布局组件方法已移至 layout.rs 模块
}

// ─── eframe::App impl ────────────────────────────────────────────────────────

impl eframe::App for SelectorApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.config.last_xpaths = self.history.clone();
        if let Ok(json) = serde_json::to_string(&self.config) {
            storage.set_string("app_config", json);
        }
        if !self.hierarchy.is_empty() {
            let data = self.build_persisted_capture();
            if let Ok(json) = serde_json::to_string(&data) {
                storage.set_string("last_capture", json);
            }
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        // ── 更新主题 ────────────────────────────────────────────────────────
        let dark = ui.ctx().global_style().visuals.dark_mode;
        self.theme = Theme::new(dark);

        // 【新增】检查是否需要自动关闭日志窗口
        if let Some(close_time) = self.log_panel_auto_close_time {
            if std::time::Instant::now() >= close_time {
                self.show_log_panel = false;
                self.log_panel_auto_close_time = None;
            } else {
                // 【关键修复】在等待关闭期间，持续请求重绘
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            }
        }

        // 【关键修复】检查后台优化是否完成
        if let Some(rx) = self.optimization_rx.take() {
            if let Ok(result) = rx.try_recv() {
                // 优化完成，处理结果
                if let Some(start_time) = self.optimization_start_time.take() {
                    let elapsed = start_time.elapsed();
                    
                    match result {
                        Some(opt_result) => {
                            // 更新 hierarchy
                            self.hierarchy = opt_result.optimized_hierarchy.clone();
                            
                            // 直接使用优化器返回的 XPath
                            self.element_xpath = opt_result.optimized_xpath.clone();
                            self.xpath_text = format!("{}, {}", self.window_selector, self.element_xpath);
                            self.xpath_error = xpath::lint(&self.xpath_text);
                            self.validation = ValidationResult::Idle;
                            
                            // 标记为已优化状态
                            self.xpath_source = XPathSource::Optimized(opt_result.summary.clone());
                            
                            let summary = &opt_result.summary;
                            self.status_msg = format!(
                                "极简优化完成（耗时 {:.1}s）：移除 {} 个属性，简化 {} 个属性",
                                elapsed.as_secs_f64(),
                                summary.removed_dynamic_attrs,
                                summary.simplified_attrs
                            );
                            
                            info!("[极简优化] 优化后 XPath: {}", self.element_xpath);
                            self.do_validate();
                            self.save_to_file();
                            
                            // 【新增】优化成功后自动关闭日志窗口（延迟 2 秒，让用户有时间查看）
                            self.log_panel_auto_close_time = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                            
                            // 【关键修复】强制请求 UI 重绘，确保自动关闭能立即生效
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                        }
                        None => {
                            self.status_msg = "极简优化已取消或失败".to_string();
                            log::info!("[极简优化] 用户取消了优化或优化失败");
                        }
                    }
                }
            } else {
                // 还没有收到结果，放回去继续等待
                self.optimization_rx = Some(rx);
                // 强制刷新 UI，让用户看到日志实时更新
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
        }

        // 捕获状态下持续刷新
        if self.capture_state != CaptureState::Idle {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }

        // ── 全局键盘 ─────────────────────────────────────────────────────────
        let (f4, f7, escape) = ui.ctx().input(|i| {
            (i.key_pressed(Key::F4), i.key_pressed(Key::F7), i.key_pressed(Key::Escape))
        });
        if f4 && self.capture_state == CaptureState::Idle { self.start_capture(); }
        if f7 { self.do_validate(); }

        // ── 捕获等待逻辑 ──────────────────────────────────────────────────────
        if let CaptureState::WaitingClick { deadline } = &self.capture_state {
            if Instant::now() > *deadline {
                // 超时
                highlight::hide();
                mouse_hook::deactivate_capture();
                self.overlay.hide();
                self.capture_state = CaptureState::Idle;
                self.status_msg    = "捕获超时，已取消".to_string();
                ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
            } else if escape {
                self.cancel_capture(ui.ctx());
            } else if let Some(event) = mouse_hook::poll_click() {
                if event.is_down {
                    let mode = event.capture_mode();
                    if mode != CaptureMode::None {
                        self.finish_capture_at(event.x, event.y, mode, ui.ctx());
                    }
                }
            }

            // 悬停高亮（防抖 500ms）
            // 使用节流机制避免频繁调用 capture
            let (mx, my, mt) = mouse_hook::get_mouse_state();
            if mt > 0 {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                
                // Only attempt highlight if mouse has been stationary for at least 500ms
                // and position has changed since last highlight
                let time_since_move = now_ms.saturating_sub(mt);
                if time_since_move >= 500 && self.last_highlight_pos != Some((mx, my)) {
                    // Check if we should throttle to avoid too frequent captures
                    let should_highlight = if let Some(last_highlight_time) = self.last_highlight_time {
                        now_ms - last_highlight_time >= 100  // At least 100ms between highlights
                    } else {
                        true
                    };
                    
                    if should_highlight {
                        self.highlight_element_at(mx, my);
                        self.last_highlight_pos = Some((mx, my));
                        self.last_highlight_time = Some(now_ms);
                    }
                } else if time_since_move < 500 {
                    // Mouse is still moving, clear the highlight position
                    self.last_highlight_pos = None;
                }
            }
        } else {
            highlight::hide();
            self.last_mouse_move    = None;
            self.last_highlight_pos = None;
        }

        self.overlay.draw(ui.ctx());

        // ── 全局样式 ──────────────────────────────────────────────────────────
        let t = self.theme;
        ui.ctx().set_global_style({
            let mut s = (*ui.ctx().global_style()).clone();
            s.visuals.panel_fill   = t.panel_fill;
            s.visuals.window_fill  = t.panel_fill;
            s.spacing.item_spacing = egui::vec2(4.0, 2.0);
            s.spacing.window_margin     = egui::Margin::same(0);
            s.spacing.button_padding    = egui::vec2(6.0, 3.0);
            s.spacing.indent            = 14.0;
            s.spacing.interact_size     = egui::vec2(18.0, 18.0);
            s.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.divider);
            s
        });

        // ── Panel 布局 ────────────────────────────────────────────────────────
        self.draw_titlebar(ui);
        self.draw_top_bar(ui);
        self.draw_capture_banner(ui);   // 仅捕获状态下可见
        self.draw_bottom_panel(ui);     // XPath 预览 + 状态 + 确定/取消（单一 Panel）

        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(t.central_bg))
            .show_inside(ui, |ui| {
                ui.add_space(6.0);

                // ── 标签页 + 辅助开关 ─────────────────────────────────────────
                ui.horizontal(|ui| {
                    let element_active = self.active_tab == ElementTab::Element;
                    let window_active  = self.active_tab == ElementTab::WindowElement;

                    if ui.selectable_label(element_active, RichText::new("元素").size(12.0)).clicked() {
                        self.active_tab = ElementTab::Element;
                    }
                    ui.add_space(4.0);
                    if ui.selectable_label(window_active, RichText::new("窗口元素").size(12.0)).clicked() {
                        self.active_tab = ElementTab::WindowElement;
                    }

                    ui.add_space(16.0);

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // 简化/完整 XPath 切换
                        let simplified_changed = ui.checkbox(&mut self.show_simplified, "简化 XPath")
                            .on_hover_text("切换简化模式与完整属性模式")
                            .changed();
                        if simplified_changed && self.xpath_source.is_auto() {
                            self.rebuild_xpath();
                        }
                    });
                });

                ui.separator();

                // ── 两列分割布局（可拖拽分隔线）────────────────────────────────
                const DIVIDER_W:   f32 = 1.0;
                const DRAG_ZONE_W: f32 = 5.0;
                const GAP:         f32 = 8.0;
                const LEFT_MIN:    f32 = 220.0;

                let full_rect = ui.available_rect_before_wrap();
                let left_max  = 0.9 * full_rect.width();
                let left_w    = self.left_panel_width.clamp(LEFT_MIN, left_max);
                let div_x     = full_rect.min.x + left_w;

                let left_rect = egui::Rect::from_min_max(
                    full_rect.min,
                    egui::pos2(div_x - GAP, full_rect.max.y),
                );
                let right_rect = egui::Rect::from_min_max(
                    egui::pos2(div_x + GAP, full_rect.min.y),
                    full_rect.max,
                );

                // 拖拽区域
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
                            .clamp(LEFT_MIN, left_max);
                }
                if drag_resp.drag_stopped() {
                    self.divider_dragging = false;
                }
                if drag_resp.hovered() || self.divider_dragging {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }

                // 分隔线
                let line_color = if self.divider_dragging { t.sel_fg } else { t.border };
                ui.painter().line_segment(
                    [
                        egui::pos2(div_x, full_rect.min.y),
                        egui::pos2(div_x, full_rect.max.y),
                    ],
                    Stroke::new(DIVIDER_W, line_color),
                );

                // 左栏
                ui.painter().rect_filled(left_rect, 0.0, t.panel_fill);
                let left_content = left_rect.shrink2(egui::vec2(6.0, 4.0));
                let mut left_ui  = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(left_content)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                left_ui.set_clip_rect(left_rect);
                self.draw_left_panel(&mut left_ui);

                // 右栏
                ui.painter().rect_filled(right_rect, 0.0, t.panel_fill);
                let right_content = right_rect.shrink2(egui::vec2(8.0, 4.0));
                let mut right_ui  = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(right_content)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                right_ui.set_clip_rect(right_rect);
                self.draw_right_panel(&mut right_ui);
            });
        
        // 绘制代码生成对话框
        self.draw_code_dialog(ui.ctx());
        
        // 绘制极简优化日志面板
        {
            use std::sync::atomic::Ordering;
            use log::Level;
            
            let is_optimizing = self.optimization_in_progress.load(Ordering::SeqCst);
            
            if is_optimizing || self.show_log_panel {
                if self.show_log_panel {
                    egui::Window::new("📋 极简优化日志")
                        .open(&mut self.show_log_panel)
                        .resizable(true)
                        .default_size([700.0, 400.0])
                        .min_size([400.0, 200.0])
                        .show(ui.ctx(), |window_ui| {
                            let log_count = self.gui_logger.len();
                            
                            window_ui.horizontal(|window_ui| {
                                window_ui.label(format!("共 {} 条日志", log_count));
                                
                                window_ui.add_space(10.0);
                                
                                if window_ui.small_button("🗑️ 清空").clicked() {
                                    self.gui_logger.clear();
                                }
                                
                                if window_ui.small_button("📋 复制全部").clicked() {
                                    let logs: Vec<String> = self.gui_logger.get_logs()
                                        .iter()
                                        .map(|entry| {
                                            format!("[{}] {}", 
                                                match entry.level {
                                                    Level::Error => "ERROR",
                                                    Level::Warn => "WARN",
                                                    Level::Info => "INFO",
                                                    Level::Debug => "DEBUG",
                                                    Level::Trace => "TRACE",
                                                },
                                                entry.message
                                            )
                                        })
                                        .collect();
                                    let text = logs.join("\n");
                                    window_ui.ctx().copy_text(text);
                                }
                            });
                            
                            window_ui.separator();
                            
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .stick_to_bottom(true)
                                .show(window_ui, |scroll_ui| {
                                    let entries = self.gui_logger.get_logs();
                                    
                                    for entry in entries {
                                        // 【关键修复】使用更柔和、更易读的颜色方案
                                        let (color, prefix) = match entry.level {
                                            Level::Error => (egui::Color32::from_rgb(255, 100, 100), "❌ ERROR"),  // 柔和红色
                                            Level::Warn => (egui::Color32::from_rgb(255, 200, 100), "⚠️ WARN"),   // 柔和橙色
                                            Level::Info => (egui::Color32::from_rgb(200, 200, 200), "ℹ️ INFO"),   // 浅灰色（主要日志）
                                            Level::Debug => (egui::Color32::from_rgb(150, 150, 200), "🔧 DEBUG"),  // 柔和蓝色
                                            Level::Trace => (egui::Color32::from_rgb(180, 180, 180), "📝 TRACE"),  // 中灰色
                                        };
                                        
                                        let time_str = entry.timestamp
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .ok()
                                            .map(|d| {
                                                let secs = d.as_secs();
                                                let millis = d.subsec_millis();
                                                format!("{:02}:{:02}:{:02}.{:03}",
                                                    (secs / 3600) % 24,
                                                    (secs / 60) % 60,
                                                    secs % 60,
                                                    millis
                                                )
                                            })
                                            .unwrap_or_default();
                                        
                                        scroll_ui.horizontal(|row_ui| {
                                            row_ui.label(
                                                egui::RichText::new(format!("[{}]", time_str))
                                                    .color(egui::Color32::GRAY)
                                                    .monospace()
                                                    .size(9.0)
                                            );
                                            
                                            row_ui.label(
                                                egui::RichText::new(prefix)
                                                    .color(color)
                                                    .monospace()
                                                    .size(10.0)
                                            );
                                            
                                            row_ui.label(
                                                egui::RichText::new(entry.message)
                                                    .color(color)
                                                    .monospace()
                                                    .size(10.0)
                                            );
                                        });
                                    }
                                });
                        });
                }
            }
        }
    }
}


