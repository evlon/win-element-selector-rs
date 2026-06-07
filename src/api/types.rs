// src/api/types.rs
//
// API 请求/响应类型定义

use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// 通用结构
// ═══════════════════════════════════════════════════════════════════════════════

// 从 core::model 下沉的类型 re-export（解决 core→api 依赖）
pub use crate::core::model::{Point, Rect, OverflowInfo, NavigateStep};

// ═══════════════════════════════════════════════════════════════════════════════
// 窗口 API
// ═══════════════════════════════════════════════════════════════════════════════

/// 窗口信息（API 响应格式）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfoResponse {
    pub title: String,
    #[serde(rename = "className")]
    pub class_name: String,
    #[serde(rename = "processId")]
    pub process_id: u32,
    #[serde(rename = "processName")]
    pub process_name: String,
}

/// 窗口列表响应
#[derive(Debug, Clone, Serialize)]
pub struct WindowListResponse {
    pub windows: Vec<WindowInfoResponse>,
}

/// 窗口选择器条件
#[derive(Debug, Clone, Deserialize)]
pub struct WindowSelector {
    pub title: Option<String>,
    #[serde(rename = "className")]
    pub class_name: Option<String>,
    #[serde(rename = "processName")]
    pub process_name: Option<String>,
}

/// 窗口选择器（支持字符串或对象形式）
#[derive(Debug, Clone)]
pub enum WindowSelectorOrString {
    /// 字符串形式："Window[@Name='xxx' and @ClassName='yyy']"
    String(String),
    /// 对象形式：{title: "xxx", className: "yyy"}
    Object(WindowSelector),
}

impl<'de> serde::Deserialize<'de> for WindowSelectorOrString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        
        struct WindowSelectorVisitor;
        
        impl<'de> Visitor<'de> for WindowSelectorVisitor {
            type Value = WindowSelectorOrString;
            
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string or a WindowSelector object")
            }
            
            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(WindowSelectorOrString::String(value.to_string()))
            }
            
            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(WindowSelectorOrString::String(value))
            }
            
            fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                let selector = WindowSelector::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(WindowSelectorOrString::Object(selector))
            }
        }
        
        deserializer.deserialize_any(WindowSelectorVisitor)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 元素 API
// ═══════════════════════════════════════════════════════════════════════════════

/// 元素查询参数（GET 请求）
#[derive(Debug, Clone, Deserialize)]
pub struct ElementQuery {
    /// 窗口选择器，如 "Window[@Name='微信' and @ClassName='mmui::MainWindow']"
    pub window: String,
    /// 元素 XPath，如 "//Button[@AutomationId='btnSend']"
    /// 支持搜索模式后缀：`:first`（第一个）、`:onlyone`（唯一）、`:all`（全部，默认）
    /// 例：`//Button[@AutomationId='btnSend']:first`
    pub element: String,
    /// RuntimeId，用于缓存查找（优先于 XPath 搜索）
    #[serde(default, rename = "runtimeId", skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    /// 搜索模式（可选，覆盖 XPath 后缀中的设置）
    /// - `all`: 返回全部匹配（默认）
    /// - `first`: 只找第一个
    /// - `onlyone`: 必须唯一，否则报错
    #[serde(default, rename = "searchMode")]
    pub search_mode: Option<crate::core::model::SearchMode>,
    /// 随机坐标范围百分比（默认 0.55）
    #[serde(rename = "randomRange", default = "default_random_range")]
    pub random_range: f32,
    /// 搜索上下文（来自捕获结果的 SearchContext，None 时回退到解析 XPath 前缀）
    #[serde(default, rename = "searchContext")]
    pub search_context: Option<crate::core::model::SearchContext>,
    /// 搜索超时时间（毫秒），None 时使用默认值
    /// - Fast/FastChild 模式默认 1500ms
    /// - Full/FullChild 模式默认 3000ms
    /// - 未指定模式默认 5000ms
    #[serde(default, rename = "timeoutMs")]
    pub timeout_ms: Option<u64>,
    /// FindAll 后过滤配置（可选，默认全部开启）
    /// 控制是否排除 offscreen / 零尺寸 / 越界元素
    #[serde(default, rename = "findAllFilter")]
    pub find_all_filter: Option<crate::core::model::FindAllFilter>,
}

fn default_random_range() -> f32 {
    0.55
}

/// 元素信息（API 响应）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
    /// 元素真正可见、可点击的矩形区域（元素矩形 ∩ 窗口视口矩形）
    #[serde(rename = "visibleRect", skip_serializing_if = "Option::is_none")]
    pub visible_rect: Option<Rect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center: Option<Point>,
    #[serde(rename = "centerRandom", skip_serializing_if = "Option::is_none")]
    pub center_random: Option<Point>,
    #[serde(rename = "controlType")]
    pub control_type: String,
    pub name: String,
    #[serde(rename = "automationId")]
    pub automation_id: String,
    #[serde(rename = "className")]
    pub class_name: String,
    #[serde(rename = "frameworkId")]
    pub framework_id: String,
    #[serde(rename = "helpText")]
    pub help_text: String,
    #[serde(rename = "localizedControlType")]
    pub localized_control_type: String,
    #[serde(rename = "isEnabled")]
    pub is_enabled: bool,
    #[serde(rename = "isOffscreen")]
    pub is_offscreen: bool,
    #[serde(rename = "isPassword")]
    pub is_password: bool,
    #[serde(rename = "acceleratorKey")]
    pub accelerator_key: String,
    #[serde(rename = "accessKey")]
    pub access_key: String,
    #[serde(rename = "itemType")]
    pub item_type: String,
    #[serde(rename = "itemStatus")]
    pub item_status: String,
    #[serde(rename = "processId")]
    pub process_id: u32,
    /// RuntimeId of the element (opaque identifier for find-from-element API).
    /// Encoded as comma-separated i32 values. Can be used with /api/element/find-from
    /// to search descendants from this element without re-finding the window.
    #[serde(default, rename = "runtimeId", skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    // ─── UIA Pattern availability ──────────────────────────────────────────
    #[serde(default, rename = "isCheckable", skip_serializing_if = "Option::is_none")]
    pub is_checkable: Option<bool>,
    #[serde(default, rename = "isChecked", skip_serializing_if = "Option::is_none")]
    pub is_checked: Option<bool>,
    #[serde(default, rename = "isClickable", skip_serializing_if = "Option::is_none")]
    pub is_clickable: Option<bool>,
    #[serde(default, rename = "isScrollable", skip_serializing_if = "Option::is_none")]
    pub is_scrollable: Option<bool>,
    #[serde(default, rename = "isSelected", skip_serializing_if = "Option::is_none")]
    pub is_selected: Option<bool>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 元素导航 API (Compass)
// ═══════════════════════════════════════════════════════════════════════════════

/// 元素导航请求
#[derive(Debug, Clone, Deserialize)]
pub struct NavigateRequest {
    /// 窗口选择器
    pub window: String,
    /// 基准元素 XPath（先找到此元素，再从它导航）
    pub element: String,
    /// 基准元素的 RuntimeId（优先于 element XPath 搜索）
    #[serde(default, rename = "runtimeId", skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    /// 导航步骤列表
    pub steps: Vec<NavigateStep>,
}

/// 元素导航响应
#[derive(Debug, Clone, Serialize)]
pub struct NavigateResponse {
    pub found: bool,
    #[serde(rename = "findSelector")]
    pub find_selector: String,
    pub element: Option<ElementInfo>,
    pub error: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 从已知元素查找子元素 API (Find-From-Element)
// ═══════════════════════════════════════════════════════════════════════════════

/// 从已知元素查找子元素请求
/// 通过 runtimeId 定位之前找到的父元素，然后在其子树中执行 XPath 查找。
/// 这避免了每次都从窗口根元素重新搜索整棵树。
#[derive(Debug, Clone, Deserialize)]
pub struct FindFromElementRequest {
    /// 父元素的 RuntimeId（来自之前 API 返回的 ElementInfo.runtimeId）
    #[serde(rename = "runtimeId")]
    pub runtime_id: String,
    /// 相对于父元素的 XPath（如 //Text[@Name='标题']）
    /// 支持搜索模式后缀：`:first`（第一个）、`:onlyone`（唯一）、`:all`（全部，默认）
    pub xpath: String,
    /// 搜索模式（可选，覆盖 XPath 后缀中的设置）
    #[serde(default, rename = "searchMode")]
    pub search_mode: Option<crate::core::model::SearchMode>,
    /// 随机偏移范围 (0.0-1.0)
    #[serde(default, rename = "randomRange")]
    pub random_range: f32,
    /// 二次定位搜索策略（None 时使用 Adaptive 默认策略）
    #[serde(default, rename = "searchStrategy")]
    pub search_strategy: Option<crate::core::model::SearchStrategy>,
}

/// 从已知元素查找子元素响应
#[derive(Debug, Clone, Serialize)]
pub struct FindFromElementResponse {
    pub found: bool,
    pub elements: Vec<ElementInfo>,
    pub total: usize,
    pub error: Option<String>,
}

/// 元素查找响应
#[derive(Debug, Clone, Serialize)]
pub struct ElementResponse {
    pub found: bool,
    #[serde(rename = "findSelector")]
    pub element_selector: String,
    pub element: Option<ElementInfo>,
    /// 匹配到的元素总数（findOne 返回第一个但告知总共有多少个匹配）
    pub total: usize,
    pub error: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 鼠标 API
// ═══════════════════════════════════════════════════════════════════════════════

/// 鼠标移动选项
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MouseMoveOptions {
    /// 是否启用拟人化移动
    #[serde(default = "default_humanize")]
    pub humanize: bool,
    /// 轨迹类型: "line" | "curve"
    #[serde(rename = "movePath", default = "default_trajectory")]
    pub trajectory: String,
    /// 移动持续时间（毫秒）
    #[serde(default = "default_duration")]
    pub duration: u64,
}

fn default_humanize() -> bool { true }
fn default_trajectory() -> String { "bezier".to_string() }
fn default_duration() -> u64 { 600 }

/// 鼠标移动请求
#[derive(Debug, Clone, Deserialize)]
pub struct MouseMoveRequest {
    pub target: Point,
    pub options: Option<MouseMoveOptions>,
}

/// 鼠标移动响应
#[derive(Debug, Clone, Serialize)]
pub struct MouseMoveResponse {
    pub success: bool,
    #[serde(rename = "startPoint")]
    pub start_point: Point,
    #[serde(rename = "endPoint")]
    pub end_point: Point,
    #[serde(rename = "durationMs")]
    pub duration_ms: u64,
    pub error: Option<String>,
}

/// 点击模式：控制如何对目标元素执行点击操作
///
/// - `mouse` (默认): 模拟鼠标点击，移动到元素位置通过 SendInput 点击
/// - `invoke`: 优先使用 UIA InvokePattern.Invoke() 触发点击（不受覆盖层影响）
/// - `setFocus`: 通过 UIA SetFocus() 聚焦元素（适用于输入框等）
/// - `auto`: 自动选择最优策略：先尝试 Invoke，失败则尝试 SetFocus，最后回退到鼠标点击
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ClickMode {
    /// 鼠标点击（默认）
    #[serde(alias = "coordinate")]
    Mouse,
    /// UIA InvokePattern
    Invoke,
    /// UIA SetFocus
    SetFocus,
    /// 自动选择最优策略
    Auto,
}

impl Default for ClickMode {
    fn default() -> Self {
        ClickMode::Mouse
    }
}

/// 鼠标点击选项
#[derive(Debug, Clone, Deserialize)]
pub struct MouseClickOptions {
    /// 是否启用拟人化移动
    #[serde(default = "default_humanize")]
    pub humanize: bool,
    /// 随机坐标范围百分比
    #[serde(rename = "randomRange", default = "default_random_range")]
    pub random_range: f32,
    /// 点击前停顿（毫秒）
    #[serde(rename = "pauseBefore", default)]
    pub pause_before: u64,
    /// 点击后停顿（毫秒）
    #[serde(rename = "pauseAfter", default)]
    pub pause_after: u64,
    /// 点击按钮类型: "left" | "right"
    #[serde(default = "default_button")]
    pub button: String,
    /// 点击区域限制（按比例缩小可点击范围）
    #[serde(rename = "clickArea", default)]
    pub click_area: Option<ClickArea>,
    /// 点击偏移配置（优先级高于 click_area）
    #[serde(default)]
    pub offset: Option<ClickOffset>,
    /// 是否在点击位置留痕（红色圆点标记）
    #[serde(rename = "showDot", default)]
    pub show_dot: bool,
    /// 留痕超时时间（毫秒），默认 3000
    #[serde(rename = "dotDuration", default = "default_dot_duration")]
    pub dot_duration: u64,
    /// 点击模式（默认 mouse，兼容旧值 coordinate）
    #[serde(rename = "clickMode", default)]
    pub click_mode: ClickMode,
    /// 是否启用遮挡检测（仅坐标点击时生效），默认 false 保持向后兼容
    /// 启用后，点击前会通过 ElementFromPoint 检查目标位置是否被其他元素遮挡
    #[serde(rename = "checkBlocked", default)]
    pub check_blocked: bool,
}

impl Default for MouseClickOptions {
    fn default() -> Self {
        Self {
            humanize: default_humanize(),
            random_range: default_random_range(),
            pause_before: 0,
            pause_after: 0,
            button: default_button(),
            click_area: None,
            offset: None,
            show_dot: false,
            dot_duration: default_dot_duration(),
            click_mode: ClickMode::Mouse,
            check_blocked: false,
        }
    }
}

/// 点击区域限制（按比例）
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClickArea {
    /// 左侧排除比例（0-1），如 0.3 表示左侧 30% 区域不点击
    pub left: Option<f32>,
    /// 右侧排除比例（0-1），如 0.3 表示右侧 30% 区域不点击
    pub right: Option<f32>,
    /// 顶部排除比例（0-1）
    pub top: Option<f32>,
    /// 底部排除比例（0-1）
    pub bottom: Option<f32>,
}

/// 点击偏移配置
/// 
/// 支持两种形式：
/// 1. 预设位置：'top' | 'bottom' | 'left' | 'right' | 'center'
/// 2. 自定义表达式字符串：如 'left+20%', 'top-10px', 'right-5%', 'bottom+15px'
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ClickOffset {
    /// 预设位置
    Preset(PresetOffset),
    /// 自定义表达式字符串
    Expression(String),
}

/// 预设偏移位置
#[derive(Debug, Clone, Deserialize)]
pub enum PresetOffset {
    #[serde(rename = "top")]
    Top,
    #[serde(rename = "bottom")]
    Bottom,
    #[serde(rename = "left")]
    Left,
    #[serde(rename = "right")]
    Right,
    #[serde(rename = "center")]
    Center,
}

impl Default for ClickOffset {
    fn default() -> Self {
        ClickOffset::Preset(PresetOffset::Center)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 视口内边距（排除固定遮挡区域）
// ═══════════════════════════════════════════════════════════════════════════════

/// 内边距值：支持像素（i32）或百分比字符串（如 "5%"）
#[derive(Debug, Clone)]
pub enum InsetValue {
    /// 像素值，如 50 表示 50px
    Pixels(i32),
    /// 百分比值，如 0.05 表示 5%
    Percent(f32),
}

impl<'de> serde::Deserialize<'de> for InsetValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};

        struct InsetVisitor;

        impl<'de> Visitor<'de> for InsetVisitor {
            type Value = InsetValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an integer (pixels) or a string percentage (e.g. '5%')")
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(InsetValue::Pixels(v as i32))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(InsetValue::Pixels(v as i32))
            }

            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(InsetValue::Pixels(v as i32))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if v.ends_with('%') {
                    let num = v[..v.len() - 1].trim();
                    num.parse::<f32>()
                        .map(|p| InsetValue::Percent(p / 100.0))
                        .map_err(|_| de::Error::custom(format!("invalid percent value: '{}'", v)))
                } else {
                    Err(de::Error::custom(format!("expected number or percent string like '5%', got: '{}'", v)))
                }
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                // 允许 null → 默认 0px
                Ok(InsetValue::Pixels(0))
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(InsetValue::Pixels(0))
            }
        }

        deserializer.deserialize_any(InsetVisitor)
    }
}

impl InsetValue {
    /// 将百分比值根据容器尺寸转换为像素
    pub fn resolve(&self, container_size: i32) -> i32 {
        match self {
            InsetValue::Pixels(px) => *px,
            InsetValue::Percent(pct) => (*pct * container_size as f32).round() as i32,
        }
    }
}

/// 视口内边距（用于排除固定遮挡区域如悬浮底部栏、顶部导航等）
/// 每个字段支持数字（像素）或字符串（百分比，如 "5%"）
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ViewportInset {
    /// 左侧排除（像素或百分比）
    #[serde(default)]
    pub left: Option<InsetValue>,
    /// 顶部排除（像素或百分比）
    #[serde(default)]
    pub top: Option<InsetValue>,
    /// 右侧排除（像素或百分比）
    #[serde(default)]
    pub right: Option<InsetValue>,
    /// 底部排除（像素或百分比）
    #[serde(default)]
    pub bottom: Option<InsetValue>,
}

fn default_button() -> String { "left".to_string() }
fn default_dot_duration() -> u64 { 3000 }

/// 鼠标点击请求
#[derive(Debug, Clone, Deserialize)]
pub struct MouseClickRequest {
    /// 窗口选择器，支持字符串形式 "Window[@Name='xxx']" 或对象形式
    pub window: WindowSelectorOrString,
    pub element: String,
    /// RuntimeId，用于缓存查找（优先于 XPath 搜索）
    #[serde(default, rename = "runtimeId", skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    pub options: Option<MouseClickOptions>,
}

/// 点击的元素信息摘要
#[derive(Debug, Clone, Serialize)]
pub struct ClickedElement {
    #[serde(rename = "controlType")]
    pub control_type: String,
    pub name: String,
}

/// 鼠标点击响应
#[derive(Debug, Clone, Serialize)]
pub struct MouseClickResponse {
    pub success: bool,
    #[serde(rename = "clickPoint")]
    pub click_point: Point,
    pub element: Option<ClickedElement>,
    /// 实际使用的点击方式: "invoke" | "setFocus" | "mouse" | "auto->invoke" 等
    #[serde(rename = "clickMethod", skip_serializing_if = "Option::is_none")]
    pub click_method: Option<String>,
    /// 遮挡检测结果（仅 occlusionCheck=true 时有值）
    #[serde(rename = "occlusionDetected", skip_serializing_if = "Option::is_none")]
    pub occlusion_detected: Option<bool>,
    /// 遮挡元素信息（检测到遮挡时有值）
    #[serde(rename = "occlusionInfo", skip_serializing_if = "Option::is_none")]
    pub occlusion_info: Option<String>,
    pub error: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 滚动 API
// ═══════════════════════════════════════════════════════════════════════════════

/// 滚动选项
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MouseScrollOptions {
    /// WHEEL_DELTA 单位，默认 120
    pub delta: Option<i32>,
    /// 滚动次数，默认 30
    #[serde(default = "default_scroll_times")]
    pub times: Option<u32>,
    /// 等待出现的 xpath（支持多个，任意一个满足即停止滚动）
    pub wait: Option<serde_json::Value>,
    /// 等待超时 ms，默认 5000
    pub timeout: Option<u64>,
    /// 是否自动计算 delta（基于容器高度），默认 false
    #[serde(rename = "autoDelta", default)]
    pub auto_delta: Option<bool>,
    /// 容器高度倍率（0-1），默认 0.8
    #[serde(rename = "deltaFactor", default = "default_delta_factor")]
    pub delta_factor: Option<f32>,
    /// 等待模式：exist=元素存在即可，visible=元素存在且不在屏幕外
    #[serde(rename = "waitMode", default)]
    pub wait_mode: Option<String>,
    /// 是否滚动到视口中心（仅 visible 模式生效），默认 true
    #[serde(rename = "scrollToCenter", default = "default_scroll_to_center")]
    pub scroll_to_center: Option<bool>,
    /// scrollToCenter 模式下，元素可见后继续调整到视口中心的最大滚动次数，避免死循环，默认 5
    #[serde(rename = "scrollToCenterAdjustTimes", default = "default_scroll_to_center_adjust_times")]
    pub scroll_to_center_adjust_times: Option<u32>,
    /// 滚动间隔（每次滚动后等待 UI 响应时间，毫秒），默认 1000
    #[serde(rename = "scrollIntervalMs", default = "default_scroll_interval_ms")]
    pub scroll_interval_ms: Option<u64>,
    /// autoDelta 首次滚动后延迟（等待 UI 重新计算布局，毫秒），默认 1000
    #[serde(rename = "autoDeltaInitialDelayMs", default = "default_auto_delta_initial_delay_ms")]
    pub auto_delta_initial_delay_ms: Option<u64>,
    /// 最小 delta 比例（调整滚动时的最小 delta 占原始 delta 的比例），默认 0.1
    #[serde(rename = "minDeltaRatio", default = "default_min_delta_ratio")]
    pub min_delta_ratio: Option<f32>,
    /// 滚动居中阈值（元素中心与目标中心距离小于此阈值时认为已居中，单位：视口高度比例），默认 0.10
    #[serde(rename = "scrollToCenterThreshold", default = "default_scroll_to_center_threshold")]
    pub scroll_to_center_threshold: Option<f32>,
    /// 视口内边距（从容器视口向内扣除固定遮挡区域，支持像素和百分比）
    #[serde(rename = "viewportInset", default)]
    pub viewport_inset: Option<ViewportInset>,
    /// 平滑滚动步长（每次小步滚动的 delta），默认 40。设为 0 则使用原有 delta 逻辑
    #[serde(rename = "smoothStepDelta", default = "default_smooth_step_delta")]
    pub smooth_step_delta: Option<i32>,
    /// 平滑滚动每步等待时间（毫秒），默认 200
    #[serde(rename = "smoothStepDelayMs", default = "default_smooth_step_delay_ms")]
    pub smooth_step_delay_ms: Option<u64>,
    /// 黄金比例目标位置（元素中心在视口中的目标比例），默认根据方向自动选择：
    /// 下滚→0.618，上滚→0.382。手动指定则使用该值
    #[serde(rename = "goldenRatio", default)]
    pub golden_ratio: Option<f32>,
    /// 黄金微调最大步数（防止死循环），默认 10
    #[serde(rename = "goldenAdjustMaxSteps", default = "default_golden_adjust_max_steps")]
    pub golden_adjust_max_steps: Option<u32>,
}

fn default_scroll_to_center() -> Option<bool> { Some(true) }
fn default_scroll_to_center_adjust_times() -> Option<u32> { Some(5) }
fn default_scroll_times() -> Option<u32> { Some(100) }
fn default_delta_factor() -> Option<f32> { Some(0.8) }
fn default_scroll_interval_ms() -> Option<u64> { Some(1000) }
fn default_auto_delta_initial_delay_ms() -> Option<u64> { Some(1000) }
fn default_min_delta_ratio() -> Option<f32> { Some(0.1) }
fn default_scroll_to_center_threshold() -> Option<f32> { Some(0.10) }
fn default_smooth_step_delta() -> Option<i32> { Some(40) }
fn default_smooth_step_delay_ms() -> Option<u64> { Some(200) }
fn default_golden_adjust_max_steps() -> Option<u32> { Some(10) }

/// 滚动请求
#[derive(Debug, Clone, Deserialize)]
pub struct MouseScrollRequest {
    /// 窗口选择器（用于限定搜索范围，避免遍历所有窗口）
    pub window: Option<String>,
    pub element: String,
    pub options: Option<MouseScrollOptions>,
}

/// 滚动响应
#[derive(Debug, Clone, Serialize)]
pub struct MouseScrollResponse {
    pub success: bool,
    /// 实际滚动次数
    pub scrolled: u32,
    /// 是否找到目标
    #[serde(rename = "targetFound")]
    pub target_found: bool,
    /// 目标元素的矩形区域（仅当 target_found=true 时有值）
    #[serde(rename = "targetRect", skip_serializing_if = "Option::is_none")]
    pub target_rect: Option<Rect>,
    /// 目标元素在容器视口内可见的矩形区域（target_rect ∩ container_rect）
    #[serde(rename = "visibleRect", skip_serializing_if = "Option::is_none")]
    pub visible_rect: Option<Rect>,
    /// 是否滚动到了边界（内容不再移动）
    #[serde(rename = "scrolledToEnd")]
    pub scrolled_to_end: bool,
    pub error: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 滚动边界检测 API
// ═══════════════════════════════════════════════════════════════════════════════

/// 滚动边界检测请求
#[derive(Debug, Clone, Deserialize)]
pub struct MouseScrollDetectRequest {
    /// 窗口选择器（用于限定搜索范围）
    pub window: Option<String>,
    /// 滚动容器的元素 XPath（鼠标移动到此元素上执行滚动）
    pub container: String,
    /// 要监控的元素 ControlType 名称列表，默认 ["Text"]
    /// 支持: "Text", "Image", "ListItem", "Button", "DataItem" 等
    /// 传空则查询所有可见元素（不按 ControlType 过滤）
    #[serde(default = "default_control_types")]
    pub control_types: Vec<String>,
    /// 滚动方向："down"=向下滚（检测到底），"up"=向上滚（检测到顶），默认 "down"
    #[serde(default = "default_detect_direction")]
    pub direction: String,
    /// 排除的元素标识列表（XPath），这些元素的位置变化不计入判定
    /// 适用于悬浮工具栏等随滚动自动移位的元素
    #[serde(default)]
    pub exclude: Vec<String>,
    /// 滚动后等待 UI 响应的时间（毫秒），默认 500
    /// 某些应用（WPF/UWP/Electron）有滚动动画，需要等待 BoundingRectangle 更新到最终位置
    #[serde(default = "default_scroll_delay_ms")]
    pub scroll_delay_ms: u64,
    /// 检测后是否反向滚动抵消（默认 false）
    /// true: 检测完后往反方向滚一次，恢复原始位置
    #[serde(default)]
    pub rollback: bool,
    /// 每次小步滚动的 delta（默认 40，比之前 120 更平滑）
    #[serde(rename = "stepDelta", default = "default_step_delta")]
    pub step_delta: i32,
    /// 每步滚动后等待 UI 响应时间（毫秒），默认 200
    #[serde(rename = "stepDelayMs", default = "default_step_delay_ms")]
    pub step_delay_ms: u64,
    /// 连续 stuck 次数阈值，达到后判定为到底（默认 3）
    #[serde(rename = "stuckThreshold", default = "default_stuck_threshold")]
    pub stuck_threshold: u32,
    /// 最大滚动步数（防止死循环），默认 30
    #[serde(rename = "maxSteps", default = "default_max_steps")]
    pub max_steps: u32,
}

fn default_control_types() -> Vec<String> { vec!["Text".to_string()] }
fn default_detect_direction() -> String { "down".to_string() }
fn default_scroll_delay_ms() -> u64 { 500 }
fn default_step_delta() -> i32 { 40 }
fn default_step_delay_ms() -> u64 { 200 }
fn default_stuck_threshold() -> u32 { 3 }
fn default_max_steps() -> u32 { 30 }

/// 滚动边界检测响应
#[derive(Debug, Clone, Serialize)]
pub struct MouseScrollDetectResponse {
    pub success: bool,
    /// 是否到达边界（排除exclude后，所有监控元素位置均未变化）
    #[serde(rename = "atEnd")]
    pub at_end: bool,
    /// 监控的元素总数（排除后）
    #[serde(rename = "watchedCount")]
    pub watched_count: usize,
    /// 发生位置变化的元素数
    #[serde(rename = "changedCount")]
    pub changed_count: usize,
    /// 变化元素的详情列表
    #[serde(rename = "details", skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<ElementChangeDetail>,
    /// 是否执行了反向回滚
    #[serde(rename = "rolledBack")]
    pub rolled_back: bool,
    /// 实际执行的滚动步数
    #[serde(rename = "stepsScrolled")]
    pub steps_scrolled: u32,
    pub error: Option<String>,
}

/// 元素变化详情（滚动前后对比）
#[derive(Debug, Clone, Serialize)]
pub struct ElementChangeDetail {
    /// 元素标识（automationId / name / className 组合）
    pub identifier: String,
    /// 滚动前 bound.top
    #[serde(rename = "beforeTop", skip_serializing_if = "Option::is_none")]
    pub before_top: Option<i32>,
    /// 滚动后 bound.top
    #[serde(rename = "afterTop", skip_serializing_if = "Option::is_none")]
    pub after_top: Option<i32>,
    /// bound.top 变化量
    #[serde(rename = "deltaTop", skip_serializing_if = "Option::is_none")]
    pub delta_top: Option<i32>,
    /// isOffscreen 是否变化
    #[serde(rename = "offscreenChanged")]
    pub offscreen_changed: bool,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 悬停 & 拖拽 API
// ═══════════════════════════════════════════════════════════════════════════════

/// 悬停选项
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MouseHoverOptions {
    /// 是否启用拟人化移动
    #[serde(default = "default_humanize")]
    pub humanize: bool,
    /// 悬停停留时间（毫秒），默认 500
    #[serde(default = "default_hover_duration")]
    pub duration: u64,
}

fn default_hover_duration() -> u64 { 500 }

/// 悬停请求
#[derive(Debug, Clone, Deserialize)]
pub struct MouseHoverRequest {
    /// 窗口选择器
    pub window: WindowSelectorOrString,
    /// 元素 XPath
    pub element: String,
    /// RuntimeId，用于缓存查找（优先于 XPath 搜索）
    #[serde(default, rename = "runtimeId", skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    pub options: Option<MouseHoverOptions>,
}

/// 悬停响应
#[derive(Debug, Clone, Serialize)]
pub struct MouseHoverResponse {
    pub success: bool,
    pub hover_point: Point,
    pub error: Option<String>,
}

/// 拖拽选项
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MouseDragOptions {
    /// 是否启用拟人化移动
    #[serde(default = "default_humanize")]
    pub humanize: bool,
    /// 拖拽持续时间（毫秒）
    #[serde(default = "default_duration")]
    pub duration: u64,
}

/// 拖拽请求
#[derive(Debug, Clone, Deserialize)]
pub struct MouseDragRequest {
    /// 源元素窗口选择器
    pub window: WindowSelectorOrString,
    /// 源元素 XPath
    #[serde(rename = "sourceElement")]
    pub source_element: String,
    /// 源元素 RuntimeId
    #[serde(default, rename = "sourceRuntimeId", skip_serializing_if = "Option::is_none")]
    pub source_runtime_id: Option<String>,
    /// 目标元素 XPath
    #[serde(rename = "targetElement")]
    pub target_element: String,
    /// 目标元素 RuntimeId
    #[serde(default, rename = "targetRuntimeId", skip_serializing_if = "Option::is_none")]
    pub target_runtime_id: Option<String>,
    pub options: Option<MouseDragOptions>,
}

/// 拖拽响应
#[derive(Debug, Clone, Serialize)]
pub struct MouseDragResponse {
    pub success: bool,
    pub source_point: Point,
    pub target_point: Point,
    pub duration_ms: u64,
    pub error: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 元素可视区域位置 API
// ═══════════════════════════════════════════════════════════════════════════════

/// 元素可视区域位置查询请求
#[derive(Debug, Clone, Deserialize)]
pub struct ElementVisibilityRequest {
    /// 窗口选择器
    pub window: String,
    /// 元素 XPath
    pub element: String,
    /// RuntimeId，用于缓存查找（优先于 XPath 搜索）
    #[serde(default, rename = "runtimeId", skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    /// 可选的滚动容器 XPath，用于计算元素在容器内的可见矩形
    /// 若提供，可见区域 = 元素矩形 ∩ 容器可见矩形 ∩ 窗口视口
    #[serde(default)]
    pub container: Option<String>,
}

/// 元素可视区域位置响应
#[derive(Debug, Clone, Serialize)]
pub struct ElementVisibilityResponse {
    /// 是否找到元素
    pub found: bool,
    /// UIA 的 IsOffscreen 属性
    #[serde(rename = "isOffscreen")]
    pub is_offscreen: Option<bool>,
    /// 可视性：fully_visible / partially_visible / offscreen
    pub visibility: String,
    /// 相对位置：above / below / left / right / inside / partial
    pub position: String,
    /// 元素的边界矩形
    #[serde(rename = "elementRect", skip_serializing_if = "Option::is_none")]
    pub element_rect: Option<Rect>,
    /// 元素真正可见、可点击的矩形区域（元素矩形 ∩ 视口矩形 ∩ 容器矩形）
    #[serde(rename = "visibleRect", skip_serializing_if = "Option::is_none")]
    pub visible_rect: Option<Rect>,
    /// 窗口（视口）的边界矩形
    #[serde(rename = "viewportRect", skip_serializing_if = "Option::is_none")]
    pub viewport_rect: Option<Rect>,
    /// 元素在各方向超出视口的像素数（正值=超出，0=在视口内）
    #[serde(rename = "overflow", skip_serializing_if = "Option::is_none")]
    pub overflow: Option<OverflowInfo>,
    /// 建议滚动方向：up / down / left / right / null
    #[serde(rename = "scrollDirection", skip_serializing_if = "Option::is_none")]
    pub scroll_direction: Option<String>,
    pub error: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 元素高亮闪烁 API
// ═══════════════════════════════════════════════════════════════════════════════

/// 元素高亮闪烁请求
#[derive(Debug, Clone, Deserialize)]
pub struct ElementFlashRequest {
    /// 窗口选择器
    pub window: String,
    /// 元素 XPath
    pub element: String,
    /// RuntimeId，用于缓存查找（优先于 XPath 搜索）
    #[serde(default, rename = "runtimeId", skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    /// 闪烁持续时间（毫秒），默认 1000
    #[serde(default = "default_flash_timeout")]
    pub timeout: u64,
}

fn default_flash_timeout() -> u64 { 1000 }

/// 元素高亮闪烁响应
#[derive(Debug, Clone, Serialize)]
pub struct ElementFlashResponse {
    pub success: bool,
    #[serde(rename = "elementRect", skip_serializing_if = "Option::is_none")]
    pub element_rect: Option<Rect>,
    pub error: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 元素 Inspect API
// ═══════════════════════════════════════════════════════════════════════════════

/// Inspect 请求参数
#[derive(Debug, Clone, Deserialize)]
pub struct InspectRequest {
    /// 窗口选择器 XPath
    pub window: String,
    /// 目标元素 XPath（inspect 此元素下的所有子元素）
    pub element: String,
    /// RuntimeId，用于缓存查找（优先于 XPath 搜索）
    #[serde(default, rename = "runtimeId", skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    /// 最大遍历深度，默认 10
    #[serde(rename = "maxDepth", default = "default_inspect_max_depth")]
    pub max_depth: usize,
    /// 最大节点数，默认 500
    #[serde(rename = "maxNodes", default = "default_inspect_max_nodes")]
    pub max_nodes: usize,
    /// 返回格式：'json'（默认）、'txt' 或 'text'
    #[serde(default = "default_inspect_format")]
    pub format: String,
}

fn default_inspect_max_depth() -> usize { 10 }
fn default_inspect_max_nodes() -> usize { 500 }
fn default_inspect_format() -> String { "json".to_string() }

/// Inspect 单个节点信息（API 响应格式）
#[derive(Debug, Clone, Serialize)]
pub struct InspectNodeInfo {
    /// 元素层级深度（根元素为 0）
    pub depth: usize,
    /// 控件类型，如 "Button"、"Text"、"Edit" 等
    #[serde(rename = "controlType")]
    pub control_type: String,
    /// 控件的 Name 属性
    pub name: String,
    /// 控件的 ClassName 属性
    #[serde(rename = "className")]
    pub class_name: String,
    /// 控件的 AutomationId 属性
    #[serde(rename = "automationId")]
    pub automation_id: String,
    /// 控件的 FrameworkId 属性
    #[serde(rename = "frameworkId")]
    pub framework_id: String,
    /// 控件的文本内容（通过 ValuePattern 获取）
    #[serde(rename = "textValue", skip_serializing_if = "Option::is_none")]
    pub text_value: Option<String>,
    /// 控件的 HelpText 属性（辅助说明文字）
    #[serde(rename = "helpText", skip_serializing_if = "String::is_empty")]
    pub help_text: String,
    /// 控件的 ItemType 属性
    #[serde(rename = "itemType", skip_serializing_if = "String::is_empty")]
    pub item_type: String,
    /// 控件的 ItemStatus 属性
    #[serde(rename = "itemStatus", skip_serializing_if = "String::is_empty")]
    pub item_status: String,
    /// 控件的区域位置
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
    /// 是否在屏幕外
    #[serde(rename = "isOffscreen")]
    pub is_offscreen: bool,
    /// 选中该控件相对于根元素的 XPath 表达式
    pub xpath: String,
    /// 子节点列表
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<InspectNodeInfo>,
}

/// Inspect 单个节点信息（扁平列表格式，无 children）
#[derive(Debug, Clone, Serialize)]
pub struct FlatInspectNodeInfo {
    /// 元素层级深度（根元素为 0）
    pub depth: usize,
    /// 控件类型，如 "Button"、"Text"、"Edit" 等
    #[serde(rename = "controlType")]
    pub control_type: String,
    /// 控件的 Name 属性
    pub name: String,
    /// 控件的 ClassName 属性
    #[serde(rename = "className")]
    pub class_name: String,
    /// 控件的 AutomationId 属性
    #[serde(rename = "automationId")]
    pub automation_id: String,
    /// 控件的 FrameworkId 属性
    #[serde(rename = "frameworkId")]
    pub framework_id: String,
    /// 控件的文本内容（通过 ValuePattern 获取）
    #[serde(rename = "textValue", skip_serializing_if = "Option::is_none")]
    pub text_value: Option<String>,
    /// 控件的 HelpText 属性（辅助说明文字）
    #[serde(rename = "helpText", skip_serializing_if = "String::is_empty")]
    pub help_text: String,
    /// 控件的 ItemType 属性
    #[serde(rename = "itemType", skip_serializing_if = "String::is_empty")]
    pub item_type: String,
    /// 控件的 ItemStatus 属性
    #[serde(rename = "itemStatus", skip_serializing_if = "String::is_empty")]
    pub item_status: String,
    /// 控件的区域位置
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
    /// 是否在屏幕外
    #[serde(rename = "isOffscreen")]
    pub is_offscreen: bool,
    /// 选中该控件相对于根元素的 XPath 表达式
    pub xpath: String,
}

/// Inspect 响应
#[derive(Debug, Clone, Serialize)]
pub struct InspectResponse {
    /// 是否成功
    pub success: bool,
    /// 根元素 XPath
    #[serde(rename = "rootXpath")]
    pub root_xpath: String,
    /// 结构化节点树（format='json' 时有值）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nodes: Option<InspectNodeInfo>,
    /// 扁平化节点列表（DFS 顺序，方便遍历和过滤）
    #[serde(rename = "flatNodes")]
    pub flat_nodes: Vec<FlatInspectNodeInfo>,
    /// 过滤后的节点列表（当请求中包含 filter 时有值）
    #[serde(rename = "filteredNodes", skip_serializing_if = "Vec::is_empty")]
    pub filtered_nodes: Vec<FlatInspectNodeInfo>,
    /// 格式化文本（format='txt'/'text' 时有值）
    #[serde(rename = "text", skip_serializing_if = "Option::is_none")]
    pub text_output: Option<String>,
    /// 子元素总数
    #[serde(rename = "totalChildren")]
    pub total_children: usize,
    /// 错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 将 core::uia::InspectNode 转换为 API 层的 InspectNodeInfo
impl From<crate::core::uia::InspectNode> for InspectNodeInfo {
    fn from(node: crate::core::uia::InspectNode) -> Self {
        InspectNodeInfo {
            depth: node.depth,
            control_type: node.control_type,
            name: node.name,
            class_name: node.class_name,
            automation_id: node.automation_id,
            framework_id: node.framework_id,
            text_value: node.text_value,
            help_text: node.help_text,
            item_type: node.item_type,
            item_status: node.item_status,
            rect: node.rect,
            is_offscreen: node.is_offscreen,
            xpath: node.relative_xpath,
            children: node.children.into_iter().map(Into::into).collect(),
        }
    }
}

/// 将 core::uia::InspectNode 转换为 API 层的 FlatInspectNodeInfo（扁平格式，无 children）
impl From<crate::core::uia::InspectNode> for FlatInspectNodeInfo {
    fn from(node: crate::core::uia::InspectNode) -> Self {
        FlatInspectNodeInfo {
            depth: node.depth,
            control_type: node.control_type,
            name: node.name,
            class_name: node.class_name,
            automation_id: node.automation_id,
            framework_id: node.framework_id,
            text_value: node.text_value,
            help_text: node.help_text,
            item_type: node.item_type,
            item_status: node.item_status,
            rect: node.rect,
            is_offscreen: node.is_offscreen,
            xpath: node.relative_xpath,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 辅助函数
// ═══════════════════════════════════════════════════════════════════════════════

impl From<super::super::model::ElementRect> for Rect {
    fn from(r: super::super::model::ElementRect) -> Self {
        Self {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        }
    }
}

/// Convert core::model::ElementData → api::types::ElementInfo
impl From<super::super::model::ElementData> for ElementInfo {
    fn from(d: super::super::model::ElementData) -> Self {
        Self {
            rect: d.rect,
            visible_rect: d.visible_rect,
            center: d.center,
            center_random: d.center_random,
            control_type: d.control_type,
            name: d.name,
            automation_id: d.automation_id,
            class_name: d.class_name,
            framework_id: d.framework_id,
            help_text: d.help_text,
            localized_control_type: d.localized_control_type,
            is_enabled: d.is_enabled,
            is_offscreen: d.is_offscreen,
            is_password: d.is_password,
            accelerator_key: d.accelerator_key,
            access_key: d.access_key,
            item_type: d.item_type,
            item_status: d.item_status,
            process_id: d.process_id,
            runtime_id: d.runtime_id,
            is_checkable: d.is_checkable,
            is_checked: d.is_checked,
            is_clickable: d.is_clickable,
            is_scrollable: d.is_scrollable,
            is_selected: d.is_selected,
        }
    }
}

impl From<super::super::model::WindowInfo> for WindowInfoResponse {
    fn from(w: super::super::model::WindowInfo) -> Self {
        Self {
            title: w.title,
            class_name: w.class_name,
            process_id: w.process_id,
            process_name: w.process_name,
        }
    }
}