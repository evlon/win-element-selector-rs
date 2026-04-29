// src/core/model.rs
//
// Core data models shared between GUI and HTTP API.

use uiauto_xpath::{is_dynamic_class, extract_stable_prefix};
use serde::{Deserialize, Serialize};

// ─── Operator ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Operator {
    Equals,           // 等于: @Name='value'
    NotEquals,        // 不等于: @Name!='value'
    Contains,         // 包含: contains(@Name, 'value')
    NotContains,      // 不包含: not(contains(@Name, 'value'))
    StartsWith,       // 开头为: starts-with(@Name, 'value')
    NotStartsWith,    // 开头不为: not(starts-with(@Name, 'value'))
    EndsWith,         // 结尾为: substring(@Name, ...)='value'
    NotEndsWith,      // 结尾不为: not(substring(@Name, ...)='value')
    GreaterThan,      // 大于: @Index > value (numeric)
    GreaterThanOrEq,  // 大于等于: @Index >= value
    LessThan,         // 小于: @Index < value
    LessThanOrEq,     // 小于等于: @Index <= value
}

impl Operator {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Equals          => "等于",
            Self::NotEquals       => "不等于",
            Self::Contains        => "包含",
            Self::NotContains     => "不包含",
            Self::StartsWith      => "开头为",
            Self::NotStartsWith   => "开头不为",
            Self::EndsWith        => "结尾为",
            Self::NotEndsWith     => "结尾不为",
            Self::GreaterThan     => "大于",
            Self::GreaterThanOrEq => "大于等于",
            Self::LessThan        => "小于",
            Self::LessThanOrEq    => "小于等于",
        }
    }

    pub fn all() -> &'static [Operator] {
        &[
            Operator::Equals,
            Operator::NotEquals,
            Operator::Contains,
            Operator::NotContains,
            Operator::StartsWith,
            Operator::NotStartsWith,
            Operator::EndsWith,
            Operator::NotEndsWith,
            Operator::GreaterThan,
            Operator::GreaterThanOrEq,
            Operator::LessThan,
            Operator::LessThanOrEq,
        ]
    }

    /// Generate the XPath predicate fragment for this operator.
    pub fn to_predicate(&self, attr: &str, value: &str) -> String {
        match self {
            Self::Equals          => format!("@{}='{}'", attr, value),
            Self::NotEquals       => format!("@{}!='{}'", attr, value),
            Self::Contains        => format!("contains(@{}, '{}')", attr, value),
            Self::NotContains     => format!("not(contains(@{}, '{}'))", attr, value),
            Self::StartsWith      => format!("starts-with(@{}, '{}')", attr, value),
            Self::NotStartsWith   => format!("not(starts-with(@{}, '{}'))", attr, value),
            Self::EndsWith        => {
                let val_len = value.chars().count();
                format!("substring(@{0}, string-length(@{0})-{1}+1)='{2}'", attr, val_len, value)
            }
            Self::NotEndsWith     => {
                let val_len = value.chars().count();
                format!("not(substring(@{0}, string-length(@{0})-{1}+1)='{2}')", attr, val_len, value)
            }
            // Numeric comparisons (for Index, etc.)
            Self::GreaterThan     => format!("@{} > {}", attr, value),
            Self::GreaterThanOrEq => format!("@{} >= {}", attr, value),
            Self::LessThan        => format!("@{} < {}", attr, value),
            Self::LessThanOrEq    => format!("@{} <= {}", attr, value),
        }
    }
}

// ─── PropertyFilter ──────────────────────────────────────────────────────────

/// A single attribute match condition within a hierarchy node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyFilter {
    pub name:     String,
    pub operator: Operator,
    pub value:    String,
    pub enabled:  bool,
}

impl PropertyFilter {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        let value = value.into();
        let enabled = !value.is_empty();
        Self { name: name.into(), operator: Operator::Equals, value, enabled }
    }

    /// Create filter with custom operator (e.g., StartsWith for dynamic class names)
    pub fn new_with_operator(name: impl Into<String>, value: impl Into<String>, operator: Operator) -> Self {
        let value = value.into();
        let enabled = !value.is_empty();
        Self { name: name.into(), operator, value, enabled }
    }

    /// Create disabled filter (will be skipped in XPath generation)
    pub fn new_disabled(name: impl Into<String>) -> Self {
        Self { name: name.into(), operator: Operator::Equals, value: String::new(), enabled: false }
    }

    /// Returns the XPath predicate string, or empty string if disabled/empty.
    pub fn predicate(&self) -> Option<String> {
        if !self.enabled || self.value.is_empty() {
            return None;
        }
        Some(self.operator.to_predicate(&self.name, &self.value))
    }
}

// ─── ElementRect ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ElementRect {
    pub x:      i32,
    pub y:      i32,
    pub width:  i32,
    pub height: i32,
}

// ─── HierarchyNode ───────────────────────────────────────────────────────────

/// One level in the ancestor chain from root window to target element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HierarchyNode {
    pub control_type:          String,
    pub automation_id:         String,
    pub class_name:            String,
    pub name:                  String,
    pub index:                 i32,         // 1-based sibling index, 0 = unknown
    pub framework_id:          String,      // FrameworkId: "Win32", "WPF", "Qt", "WinForms", "UWP"
    pub acc_role:              String,      // Accessibility Role
    pub help_text:             String,      // HelpText property
    pub localized_control_type: String,     // LocalizedControlType
    pub is_enabled:            bool,        // IsEnabled property
    pub is_offscreen:          bool,        // IsOffscreen property
    pub rect:                  ElementRect,
    pub process_id:            u32,
    pub filters:               Vec<PropertyFilter>,
    /// Whether this node should be included in the final XPath.
    pub included:              bool,
    /// Whether this node is the target element (the one user captured).
    /// Used by optimizer to correctly identify the target node.
    pub is_target:             bool,
    /// Position function mode for Index:
    /// - "position": position()=N (default)
    /// - "first": first() (when position=1)
    /// - "last": last() (when position is last sibling)
    /// - "index": @Index='N' (fallback)
    pub position_mode:         String,
    /// Total sibling count (needed for last() detection)
    pub sibling_count:         i32,
}

impl HierarchyNode {
    pub fn new(
        control_type:  impl Into<String>,
        automation_id: impl Into<String>,
        class_name:    impl Into<String>,
        name:          impl Into<String>,
        index:         i32,
        rect:          ElementRect,
        process_id:    u32,
    ) -> Self {
        let ct  = control_type.into();
        let aid = automation_id.into();
        let cn  = class_name.into();
        let nm  = name.into();

        // Build filters with extended properties
        // ClassName: detect dynamic class names and use stable prefix
        let class_filter = if is_dynamic_class(&cn) {
            let prefix = extract_stable_prefix(&cn);
            if prefix.len() >= 4 {
                PropertyFilter::new_with_operator("ClassName", prefix, Operator::StartsWith)
            } else {
                // Prefix too short, disable ClassName filter
                PropertyFilter::new_disabled("ClassName")
            }
        } else {
            PropertyFilter::new("ClassName", &cn)
        };
        
        let mut filters = vec![
            PropertyFilter::new("ControlType",    &ct),
            PropertyFilter::new("AutomationId",   &aid),
            class_filter,
            PropertyFilter::new("Name",           &nm),
        ];
        
        // Add Index filter (will use position() function if use_position_function is true)
        if index > 0 {
            filters.push(PropertyFilter::new("Index", index.to_string()));
        }

        Self {
            control_type: ct,
            automation_id: aid,
            class_name: cn,
            name: nm,
            index,
            framework_id: String::new(),
            acc_role: String::new(),
            help_text: String::new(),
            localized_control_type: String::new(),
            is_enabled: true,
            is_offscreen: false,
            rect,
            process_id,
            filters,
            included: true,
            is_target: false,          // 默认非目标节点，捕获时会设置最后一个节点为目标
            position_mode: "position".to_string(),  // Default to position()=N
            sibling_count: 0,  // Will be computed during capture
        }
    }

    /// XPath tag name derived from ControlType.
    pub fn tag(&self) -> &str {
        if self.control_type.is_empty() { "*" } else { &self.control_type }
    }

    /// Build the XPath segment for this node.
    /// Supports position(), first(), last() functions for better XPath standard compatibility.
    pub fn xpath_segment(&self) -> String {
        let predicates: Vec<String> = self.filters
            .iter()
            .filter_map(|f| {
                // Special handling for Index: convert to position() function
                if f.name == "Index" && f.enabled && !f.value.is_empty() {
                    if let Ok(pos) = f.value.parse::<i32>() {
                        if pos > 0 {
                            return Some(self.format_position(pos));
                        }
                    }
                }
                // Regular property filter
                f.predicate()
            })
            .collect();

        // Note: The prefix (// or /) is determined by the caller based on position.
        // This function only builds the tag and predicates part.
        if predicates.is_empty() {
            format!("{}", self.tag())
        } else {
            format!("{}[{}]", self.tag(), predicates.join(" and "))
        }
    }

    /// Format position using XPath functions: first(), last(), or position()=N
    fn format_position(&self, pos: i32) -> String {
        match self.position_mode.as_str() {
            "first" => {
                // Always use first() (user override)
                "first()".to_string()
            }
            "last" => {
                // Always use last() (user override)
                "last()".to_string()
            }
            "index" => {
                // Fallback to @Index attribute
                format!("@Index='{}'", pos)
            }
            _ => {
                // Default: "position" mode - use smart detection
                if pos == 1 {
                    // First element: use first()
                    "first()".to_string()
                } else if self.sibling_count > 0 && pos == self.sibling_count {
                    // Last element: use last()
                    "last()".to_string()
                } else {
                    // Middle element: use position()=N
                    format!("position()={}", pos)
                }
            }
        }
    }

    /// Build the full XPath segment with proper prefix.
    /// is_first: true for the root node (//), false for children (/)
    #[allow(dead_code)]
    pub fn xpath_segment_with_prefix(&self, is_first: bool) -> String {
        let prefix = if is_first { "//" } else { "/" };
        let segment = self.xpath_segment();
        format!("{}{}", prefix, segment)
    }

    /// Short label shown in the hierarchy tree panel.
    pub fn tree_label(&self) -> String {
        let mut parts = vec![self.control_type.clone()];
        if !self.automation_id.is_empty() {
            parts.push(format!("id=\"{}\"", truncate(&self.automation_id, 22)));
        } else if !self.name.is_empty() {
            parts.push(format!("name=\"{}\"", truncate(&self.name, 22)));
        } else if !self.class_name.is_empty() {
            parts.push(format!("class=\"{}\"", truncate(&self.class_name, 22)));
        }
        if self.index > 0 {
            parts.push(format!("[{}]", self.index));
        }
        parts.join("  ")
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

// ─── WindowInfo ──────────────────────────────────────────────────────────────

/// Information about the target window for fast XPath validation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowInfo {
    pub title: String,           // Window title
    pub class_name: String,      // Window class name
    pub process_id: u32,         // Process ID
    pub process_name: String,    // Process name (e.g., 'Weixin', 'chrome')
}

// ─── XPathResult ─────────────────────────────────────────────────────────────

/// Complete XPath selection result: window selector + element XPath.
pub struct XPathResult {
    /// Window selector in XPath-like format, e.g. "Window[@Name='微信' and @ClassName='mmui::MainWindow']"
    pub window_selector: String,
    /// Element XPath starting from window root, e.g. "/Group/Button[@Name='发送']" or "/Group//Button[...]"
    pub element_xpath: String,
}

// ─── CaptureResult ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CaptureResult {
    pub hierarchy:  Vec<HierarchyNode>,
    pub cursor_x:   i32,
    pub cursor_y:   i32,
    pub error:      Option<String>,
    /// Window information extracted from hierarchy (for fast validation).
    pub window_info: Option<WindowInfo>,
}

// ─── ValidationResult ────────────────────────────────────────────────────────

/// 单个属性校验结果（用于逐层校验显示）
#[derive(Debug, Clone)]
pub struct PropertyValidationResult {
    /// 属性名 (如 ClassName, Name, ControlType)
    pub attr_name: String,
    /// 运算符 (Equals, StartsWith, Contains 等)
    pub operator: Operator,
    /// XPath 中的期望值
    pub expected_value: String,
    /// 实际元素的值（校验后获取）
    pub actual_value: Option<String>,
    /// 匹配状态: true = 匹配成功, false = 不匹配
    pub matched: bool,
    /// 此属性是否启用（参与校验）
    pub enabled: bool,
}

/// 单个层级节点的校验结果
#[derive(Debug, Clone)]
pub struct LayerValidationResult {
    /// 节点在 hierarchy 中的索引
    pub node_index: usize,
    /// ControlType (用于显示)
    pub control_type: String,
    /// 节点简短描述（用于UI显示）
    pub node_label: String,
    /// 是否匹配（此层级是否找到元素）
    pub matched: bool,
    /// 各属性的校验结果
    pub properties: Vec<PropertyValidationResult>,
    /// 匹配的元素数量
    pub match_count: usize,
    /// 此层级执行时间 (ms)
    pub duration_ms: u64,
}

/// 单个属性校验失败详情（用于错误提示）
#[derive(Debug, Clone)]
pub struct PredicateFailure {
    /// 属性名 (如 ClassName, Name)
    pub attr_name: String,
    /// XPath 中的期望值
    pub expected_value: String,
    /// 实际元素的值（如果能获取）
    pub actual_value: Option<String>,
    /// 失败原因提示
    pub reason: String,
}

/// Result of validating a single XPath segment.
#[derive(Debug, Clone)]
pub struct SegmentValidationResult {
    /// Segment index (0-based)
    pub segment_index: usize,
    /// Segment text (e.g., "/Group[@ClassName='QWidget']")
    pub segment_text: String,
    /// Whether this segment matched
    pub matched: bool,
    /// Number of matches found at this level
    pub match_count: usize,
    /// Time taken to validate this segment (in milliseconds)
    pub duration_ms: u64,
    /// 失败的 predicate 详情（如果匹配失败）
    pub predicate_failures: Vec<PredicateFailure>,
}

/// Detailed validation result with per-segment information.
#[derive(Debug, Clone)]
pub struct DetailedValidationResult {
    /// Overall result
    pub overall: ValidationResult,
    /// Per-segment validation results (XPath step results)
    pub segments: Vec<SegmentValidationResult>,
    /// Per-layer validation results (hierarchy node results) - 新增
    pub layers: Vec<LayerValidationResult>,
    /// Total validation time (in milliseconds)
    pub total_duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValidationResult {
    Idle,
    Running,
    Found { count: usize, first_rect: Option<ElementRect> },
    NotFound,
    Error(String),
}

impl ValidationResult {
    pub fn label(&self) -> String {
        match self {
            Self::Idle       => "校验元素  F7".into(),
            Self::Running    => "校验中…".into(),
            Self::Found { count, .. } => format!("✔ 找到 {} 个", count),
            Self::NotFound   => "✘ 未找到".into(),
            Self::Error(_)   => "⚠ 校验错误".into(),
        }
    }
    
    /// 获取详细错误消息（用于状态栏显示）
    pub fn error_message(&self) -> Option<String> {
        match self {
            Self::Error(e) => Some(e.clone()),
            _ => None,
        }
    }
}

// ─── ElementTab (标签页) ────────────────────────────────────────────────────

/// 标签页类型：元素定位或窗口元素定位
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElementTab {
    Element,      // 元素定位（默认）
    WindowElement, // 窗口元素定位
}

impl Default for ElementTab {
    fn default() -> Self {
        ElementTab::Element
    }
}

// ─── HighlightInfo ──────────────────────────────────────────────────────────

/// Information needed for highlight window display.
#[derive(Debug, Clone)]
pub struct HighlightInfo {
    /// Element rectangle for positioning
    pub rect: ElementRect,
    /// Element control type (English, e.g., "Button", "Edit")
    pub control_type: String,
    /// Element control type in Chinese (e.g., "按钮", "可编辑文本")
    pub control_type_cn: String,
}

impl HighlightInfo {
    pub fn new(rect: ElementRect, control_type: &str) -> Self {
        Self {
            rect,
            control_type: control_type.to_string(),
            control_type_cn: control_type_to_chinese(control_type),
        }
    }
}

/// Convert English control type to Chinese description.
pub fn control_type_to_chinese(control_type: &str) -> String {
    match control_type {
        "Button"       => "按钮",
        "Calendar"     => "日历",
        "CheckBox"     => "复选框",
        "ComboBox"     => "下拉框",
        "Custom"       => "自定义控件",
        "DataGrid"     => "数据网格",
        "DataItem"     => "数据项",
        "Document"     => "文档",
        "Edit"         => "可编辑文本",
        "Group"        => "分组",
        "Header"       => "表头",
        "HeaderItem"   => "表头项",
        "Hyperlink"    => "超链接",
        "Image"        => "图片",
        "List"         => "列表",
        "ListItem"     => "列表项",
        "MenuBar"      => "菜单栏",
        "Menu"         => "菜单",
        "MenuItem"     => "菜单项",
        "Pane"         => "面板",
        "ProgressBar"  => "进度条",
        "RadioButton"  => "单选按钮",
        "ScrollBar"    => "滚动条",
        "SemanticZoom" => "语义缩放",
        "Separator"    => "分隔符",
        "Slider"       => "滑块",
        "Spinner"      => "数值调节钮",
        "SplitButton"  => "拆分按钮",
        "StatusBar"    => "状态栏",
        "Tab"          => "选项卡",
        "TabItem"      => "选项卡项",
        "Table"        => "表格",
        "Text"         => "文本",
        "Thumb"        => "缩略图",
        "TitleBar"     => "标题栏",
        "ToolBar"      => "工具栏",
        "ToolTip"      => "提示",
        "Tree"         => "树形控件",
        "TreeItem"     => "树项",
        "Window"       => "窗口",
        "Desktop"      => "桌面",
        "Application"  => "应用程序",
        _               => control_type,  // Return original if no translation
    }.to_string()
}

// ─── AppConfig (persisted) ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub highlight_on_hover: bool,
    pub show_simplified:    bool,
    pub last_xpaths:        Vec<String>,   // history, newest first
    /// Show detailed validation results (per-segment timing)
    pub show_validation_details: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            highlight_on_hover: true,
            show_simplified:    false,
            last_xpaths:        Vec::new(),
            show_validation_details: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_info_extraction() {
        // Test that WindowInfo can be created and compared
        let info1 = WindowInfo {
            title: "微信".to_string(),
            class_name: "mmui::MainWindow".to_string(),
            process_id: 12345,
            process_name: "Weixin".to_string(),
        };
        
        let info2 = WindowInfo {
            title: "微信".to_string(),
            class_name: "mmui::MainWindow".to_string(),
            process_id: 12345,
            process_name: "Weixin".to_string(),
        };
        
        assert_eq!(info1, info2, "WindowInfo should support equality comparison");
    }

    #[test]
    fn test_capture_result_with_window_info() {
        let result = CaptureResult {
            hierarchy: vec![
                HierarchyNode::new("Window", "", "Notepad", "Untitled", 0, ElementRect::default(), 1234),
                HierarchyNode::new("Edit", "", "Edit", "", 0, ElementRect::default(), 1234),
            ],
            cursor_x: 100,
            cursor_y: 200,
            error: None,
            window_info: Some(WindowInfo {
                title: "Untitled".to_string(),
                class_name: "Notepad".to_string(),
                process_id: 1234,
                process_name: "notepad".to_string(),
            }),
        };
        
        assert!(result.window_info.is_some());
        let win = result.window_info.unwrap();
        assert_eq!(win.title, "Untitled");
        assert_eq!(win.class_name, "Notepad");
        assert_eq!(win.process_id, 1234);
    }

    #[test]
    fn test_capture_result_without_window_info() {
        // Desktop capture might not have Window in hierarchy
        let result = CaptureResult {
            hierarchy: vec![
                HierarchyNode::new("Pane", "", "#32769", "桌面", 0, ElementRect::default(), 0),
            ],
            cursor_x: 500,
            cursor_y: 300,
            error: None,
            window_info: None,
        };
        
        assert!(result.window_info.is_none());
    }
}