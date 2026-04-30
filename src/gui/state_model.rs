// src/gui/state_model.rs
//
// 捕获状态数据模型（单一数据源）
// 解决 UI 状态不一致问题：所有用户可修改的状态集中管理，持久化时完整保存

use std::path::Path;
use serde::{Deserialize, Serialize};
use log::info;

use element_selector::core::model::{
    CaptureResult, HierarchyNode, Operator, PropertyFilter, WindowInfo,
};

/// XPath 来源类型
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum XPathSourceKind {
    /// 自动生成
    Auto,
    /// 智能优化（携带摘要）
    Optimized { removed_dynamic_attrs: u32, simplified_attrs: u32, used_anchor: bool },
    /// 用户手动编辑
    Manual,
}

/// 单个节点的过滤器状态（持久化）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeFilterState {
    /// 是否包含在 XPath 中
    pub included: bool,
    /// 每个 PropertyFilter 的 enabled 状态
    pub filters_enabled: Vec<bool>,
    /// 每个 PropertyFilter 的运算符（用户可能修改）
    pub filters_operators: Vec<Operator>,
    /// 每个 PropertyFilter 的值（用户可能修改）
    pub filters_values: Vec<String>,
}

/// 窗口过滤器状态（持久化）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowFilterState {
    pub enabled: bool,
    pub operator: Operator,
    pub value: String,
}

/// 捕获状态数据模型（单一数据源）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureStateModel {
    /// 核心数据：层级结构
    pub hierarchy: Vec<HierarchyNode>,
    /// 窗口信息
    pub window_info: Option<WindowInfo>,
    /// 选中的节点索引
    pub selected_node_idx: Option<usize>,
    
    /// 每个节点的过滤器状态（与 hierarchy 长度一致）
    pub node_filter_states: Vec<NodeFilterState>,
    
    /// 窗口过滤器状态
    pub window_filter_states: Vec<WindowFilterState>,
    
    /// XPath 来源
    pub xpath_source: XPathSourceKind,
    
    /// 手动编辑的 XPath（仅 Manual 状态时有效）
    pub manual_element_xpath: Option<String>,
    pub manual_window_selector: Option<String>,
    
    /// 是否使用简化 XPath
    pub show_simplified: bool,
}

impl CaptureStateModel {
    /// 从 CaptureResult 创建新模型（首次捕获）
    pub fn from_capture(result: CaptureResult, show_simplified: bool) -> Self {
        let n = result.hierarchy.len();
        
        // 初始化节点过滤器状态：全部 included，全部 enabled
        let node_filter_states = result.hierarchy.iter().map(|node| {
            NodeFilterState {
                included: node.included,
                filters_enabled: node.filters.iter().map(|f| f.enabled).collect(),
                filters_operators: node.filters.iter().map(|f| f.operator.clone()).collect(),
                filters_values: node.filters.iter().map(|f| f.value.clone()).collect(),
            }
        }).collect();
        
        // 初始化窗口过滤器状态
        let window_filter_states = result.window_info.as_ref()
            .map(|win| Self::build_window_filter_states(win))
            .unwrap_or_default();
        
        Self {
            hierarchy: result.hierarchy,
            window_info: result.window_info,
            selected_node_idx: n.checked_sub(1),
            node_filter_states,
            window_filter_states,
            xpath_source: XPathSourceKind::Auto,
            manual_element_xpath: None,
            manual_window_selector: None,
            show_simplified,
        }
    }
    
    /// 从 WindowInfo 构建窗口过滤器初始状态
    fn build_window_filter_states(win: &WindowInfo) -> Vec<WindowFilterState> {
        vec![
            WindowFilterState { enabled: !win.title.is_empty(), operator: Operator::Equals, value: win.title.clone() },
            WindowFilterState { enabled: !win.class_name.is_empty(), operator: Operator::Equals, value: win.class_name.clone() },
            WindowFilterState { enabled: !win.process_name.is_empty(), operator: Operator::Equals, value: win.process_name.clone() },
        ]
    }
    
    /// 从持久化文件加载
    pub fn load(path: &Path) -> Option<Self> {
        if !path.exists() {
            return None;
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .map(|model: Self| {
                info!("Loaded CaptureStateModel with {} nodes", model.hierarchy.len());
                model
            })
    }
    
    /// 持久化保存
    pub fn save(&self, path: &Path) {
        if let Ok(json) = serde_json::to_string_pretty(&self) {
            if let Err(e) = std::fs::write(path, json) {
                log::error!("Failed to save CaptureStateModel: {}", e);
            } else {
                info!("Saved CaptureStateModel to {}", path.display());
            }
        }
    }
    
    /// 将模型状态应用到 HierarchyNode（用于渲染/XPath生成）
    pub fn apply_to_hierarchy(&self) -> Vec<HierarchyNode> {
        self.hierarchy.iter()
            .zip(self.node_filter_states.iter())
            .map(|(node, state)| {
                let mut new_node = node.clone();
                new_node.included = state.included;
                // 应用过滤器状态
                for (i, ((enabled, op), val)) in state.filters_enabled.iter()
                    .zip(state.filters_operators.iter())
                    .zip(state.filters_values.iter())
                    .enumerate()
                {
                    if i < new_node.filters.len() {
                        new_node.filters[i].enabled = *enabled;
                        new_node.filters[i].operator = op.clone();
                        new_node.filters[i].value = val.clone();
                    }
                }
                new_node
            })
            .collect()
    }
    
    /// 构建窗口 PropertyFilter 列表（用于渲染）
    pub fn build_window_filters(&self) -> Vec<PropertyFilter> {
        self.window_filter_states.iter()
            .map(|state| PropertyFilter {
                name: String::new(), // 名字由 UI 层补充
                operator: state.operator.clone(),
                value: state.value.clone(),
                enabled: state.enabled,
            })
            .collect()
    }
    
    /// 标记为手动编辑状态
    pub fn set_manual_xpath(&mut self, element_xpath: String, window_selector: String) {
        self.xpath_source = XPathSourceKind::Manual;
        self.manual_element_xpath = Some(element_xpath);
        self.manual_window_selector = Some(window_selector);
    }
    
    /// 标记为优化状态
    pub fn set_optimized(&mut self, summary: element_selector::core::OptimizationSummary) {
        self.xpath_source = XPathSourceKind::Optimized {
            removed_dynamic_attrs: summary.removed_dynamic_attrs as u32,
            simplified_attrs: summary.simplified_attrs as u32,
            used_anchor: summary.used_anchor,
        };
        self.manual_element_xpath = None;
        self.manual_window_selector = None;
    }
    
    /// 重置为自动生成状态
    pub fn reset_to_auto(&mut self) {
        self.xpath_source = XPathSourceKind::Auto;
        self.manual_element_xpath = None;
        self.manual_window_selector = None;
    }
    
    /// 是否为自动生成状态
    pub fn is_auto(&self) -> bool {
        matches!(self.xpath_source, XPathSourceKind::Auto)
    }
    
    /// 获取优化摘要（如果是优化状态）
    pub fn optimization_summary(&self) -> Option<(u32, u32, bool)> {
        match &self.xpath_source {
            XPathSourceKind::Optimized { removed_dynamic_attrs, simplified_attrs, used_anchor } =>
                Some((*removed_dynamic_attrs, *simplified_attrs, *used_anchor)),
            _ => None,
        }
    }
    
    /// 从 UI 层收集用户修改的节点状态
    pub fn sync_node_state(&mut self, idx: usize, included: bool, filters: &[PropertyFilter]) {
        if idx < self.node_filter_states.len() {
            let state = &mut self.node_filter_states[idx];
            state.included = included;
            state.filters_enabled = filters.iter().map(|f| f.enabled).collect();
            state.filters_operators = filters.iter().map(|f| f.operator.clone()).collect();
            state.filters_values = filters.iter().map(|f| f.value.clone()).collect();
        }
    }
    
    /// 从 UI 层收集窗口过滤器修改
    pub fn sync_window_filters(&mut self, filters: &[PropertyFilter], names: &[&str]) {
        self.window_filter_states = names.iter()
            .zip(filters.iter())
            .map(|(_, f)| WindowFilterState {
                enabled: f.enabled,
                operator: f.operator.clone(),
                value: f.value.clone(),
            })
            .collect();
    }
}

impl Default for CaptureStateModel {
    fn default() -> Self {
        Self {
            hierarchy: Vec::new(),
            window_info: None,
            selected_node_idx: None,
            node_filter_states: Vec::new(),
            window_filter_states: Vec::new(),
            xpath_source: XPathSourceKind::Auto,
            manual_element_xpath: None,
            manual_window_selector: None,
            show_simplified: false,
        }
    }
}