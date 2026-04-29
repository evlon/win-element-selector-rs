// src/core/xpath_optimizer.rs
//
// XPath智能优化器：直接调用 uiauto-xpath 库的 optimize 函数
//
// 设计原则：高内聚、模块化
// - 优化逻辑全部在 uiauto-xpath 库中
// - 本模块只做数据格式转换：HierarchyNode ↔ XPath 字符串
// - 统计信息直接由 uiauto-xpath 库提供

use super::model::{HierarchyNode, Operator, PropertyFilter};
use uiauto_xpath::{XPath, is_dynamic_class, extract_stable_prefix};

/// 优化摘要（用于 UI 显示）
#[derive(Debug, Clone, Default)]
pub struct OptimizationSummary {
    pub removed_dynamic_attrs: usize,
    pub simplified_attrs: usize,
    pub used_anchor: bool,
    pub anchor_description: Option<String>,
    pub compression_ratio: f64,
}

/// 优化结果
#[derive(Debug, Clone)]
pub struct OptimizationResult {
    /// 锚点节点索引（在原始 hierarchy 中）
    pub anchor_index: Option<usize>,
    /// 目标节点索引（在原始 hierarchy 中）
    pub target_index: usize,
    pub summary: OptimizationSummary,
    /// 优化后的 XPath 字符串（主推荐）
    pub optimized_xpath: String,
    /// 最小化 XPath（备选）
    pub minimal_xpath: String,
    /// 优化后的 hierarchy（已修改 included 和 filters）
    pub optimized_hierarchy: Vec<HierarchyNode>,
}

/// XPath智能优化器 - 直接调用 uiauto-xpath 库
pub struct XPathOptimizer;

impl Default for XPathOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

impl XPathOptimizer {
    pub fn new() -> Self {
        Self
    }
    
    /// 执行优化：HierarchyNode → XPath → optimize → 返回结果
    /// 同时生成优化后的 hierarchy，同步 UI 状态
    pub fn optimize(&self, hierarchy: &[HierarchyNode]) -> OptimizationResult {
        if hierarchy.is_empty() {
            return OptimizationResult {
                anchor_index: None,
                target_index: 0,
                summary: OptimizationSummary::default(),
                optimized_xpath: String::new(),
                minimal_xpath: String::new(),
                optimized_hierarchy: Vec::new(),
            };
        }
        
        // 生成完整 XPath 字符串
        let full_xpath = self.generate_full_xpath(hierarchy);
        
        // 调用 uiauto-xpath 的 optimize
        let result = XPath::optimize(&full_xpath);
        
        match result {
            Ok(opt_result) => {
                // 生成优化后的 hierarchy
                let optimized_hierarchy = self.apply_optimization_to_hierarchy(
                    hierarchy,
                    opt_result.anchor_index,
                    opt_result.target_index,
                );
                
                // 统计信息
                let summary = OptimizationSummary {
                    removed_dynamic_attrs: self.count_removed_attrs(hierarchy),
                    simplified_attrs: opt_result.simplified_attrs_count,
                    used_anchor: opt_result.anchor_index.is_some(),
                    anchor_description: Some(opt_result.anchor_desc.clone()).filter(|s| s != "none"),
                    compression_ratio: opt_result.compression_ratio,
                };
                
                OptimizationResult {
                    anchor_index: opt_result.anchor_index,
                    target_index: opt_result.target_index,
                    summary,
                    optimized_xpath: opt_result.anchor_relative,
                    minimal_xpath: opt_result.minimal,
                    optimized_hierarchy,
                }
            }
            Err(_) => {
                // optimize 失败时返回原始 hierarchy
                OptimizationResult {
                    anchor_index: None,
                    target_index: hierarchy.len() - 1,
                    summary: OptimizationSummary::default(),
                    optimized_xpath: full_xpath.clone(),
                    minimal_xpath: full_xpath.clone(),
                    optimized_hierarchy: hierarchy.to_vec(),
                }
            }
        }
    }
    
    /// 将优化结果应用到 hierarchy
    /// - 锚点之前的节点：排除 (included = false)
    /// - 锚点到目标之间的节点：包含，但属性简化
    /// - 目标节点：包含，保留必要属性
    fn apply_optimization_to_hierarchy(
        &self,
        hierarchy: &[HierarchyNode],
        anchor_index: Option<usize>,
        target_index: usize,
    ) -> Vec<HierarchyNode> {
        hierarchy.iter().enumerate().map(|(i, node)| {
            let mut optimized_node = node.clone();
            
            // 决定节点是否包含
            match anchor_index {
                Some(ai) => {
                    // 有锚点：只包含锚点到目标的节点
                    optimized_node.included = i >= ai && i <= target_index;
                }
                None => {
                    // 无锚点：只包含目标节点
                    optimized_node.included = i == target_index;
                }
            }
            
            // 如果节点被包含，优化其属性过滤器
            if optimized_node.included {
                optimized_node.filters = self.optimize_node_filters(node, i == target_index);
            }
            
            optimized_node
        }).collect()
    }
    
    /// 优化单个节点的属性过滤器
    /// - AutomationId：保留（锚点节点）
    /// - ClassName：动态类名改用 starts-with
    /// - Name：过长的禁用
    /// - ControlType：保留
    /// - Index：保留
    fn optimize_node_filters(&self, node: &HierarchyNode, is_target: bool) -> Vec<PropertyFilter> {
        node.filters.iter().map(|f| {
            let mut optimized_filter = f.clone();
            
            match f.name.as_str() {
                "AutomationId" => {
                    // AutomationId 对锚点很重要，对目标也可保留
                    optimized_filter.enabled = !f.value.is_empty();
                }
                "ClassName" => {
                    // 动态类名改用 starts-with
                    if is_dynamic_class(&f.value) {
                        let prefix = extract_stable_prefix(&f.value);
                        if prefix.len() >= 4 {
                            optimized_filter.operator = Operator::StartsWith;
                            optimized_filter.value = prefix;
                            optimized_filter.enabled = true;
                        } else {
                            // 前缀太短，禁用
                            optimized_filter.enabled = false;
                        }
                    } else {
                        // 稳定类名保留
                        optimized_filter.enabled = !f.value.is_empty();
                    }
                }
                "Name" => {
                    // 过长的 Name 可能是动态标题，禁用
                    let limit = if is_target { 30 } else { 20 };  // 目标节点限制更宽松
                    optimized_filter.enabled = f.value.len() <= limit && !f.value.is_empty();
                }
                "ControlType" => {
                    // ControlType 对定位很重要，保留
                    optimized_filter.enabled = true;
                }
                "Index" => {
                    // Index 保留
                    optimized_filter.enabled = f.value.parse::<i32>().unwrap_or(0) > 0;
                }
                _ => {
                    // 其他属性默认禁用（FrameworkId 等）
                    optimized_filter.enabled = false;
                }
            }
            
            optimized_filter
        }).collect()
    }
    
    /// 从 HierarchyNode 生成完整 XPath 字符串
    fn generate_full_xpath(&self, hierarchy: &[HierarchyNode]) -> String {
        hierarchy
            .iter()
            .filter(|n| n.included)
            .map(|n| {
                let segment = n.xpath_segment();
                if segment.starts_with('/') {
                    segment
                } else {
                    format!("/{}", segment)
                }
            })
            .collect::<Vec<_>>()
            .join("")
    }
    
    /// 计算移除的属性数量（动态 ClassName 和 AutomationId 的处理）
    fn count_removed_attrs(&self, original: &[HierarchyNode]) -> usize {
        original.iter()
            .filter(|n| n.class_name.len() > 30 || !n.automation_id.is_empty())
            .count()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 单元测试
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::ElementRect;
    use uiauto_xpath::{is_dynamic_class, extract_stable_prefix};
    
    #[test]
    fn test_optimize_with_automation_id() {
        let optimizer = XPathOptimizer::new();
        
        let hierarchy = vec![
            HierarchyNode::new("Pane", "", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Document", "RootWebArea", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Button", "", "dynamic-class", "Click", 0, ElementRect::default(), 0),
        ];
        
        let result = optimizer.optimize(&hierarchy);
        
        println!("optimized_xpath: {}", result.optimized_xpath);
        println!("minimal_xpath: {}", result.minimal_xpath);
        println!("compression_ratio: {:.1}%", result.summary.compression_ratio * 100.0);
        
        // 应检测到 AutomationId 作为锚点
        assert!(result.summary.used_anchor);
        assert!(result.optimized_xpath.contains("AutomationId='RootWebArea'"));
    }
    
    #[test]
    fn test_optimize_dynamic_classname() {
        let optimizer = XPathOptimizer::new();
        
        let hierarchy = vec![
            HierarchyNode::new("Pane", "", "Chrome_WidgetWin_1", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Document", "RootWebArea", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new(
                "Group", "",
                "temp-dialogue-btnBOp4 winFolder_optionsZJ07f t-popup-open",
                "新建对话",
                0, ElementRect::default(), 0
            ),
        ];
        
        let result = optimizer.optimize(&hierarchy);
        
        println!("optimized_xpath: {}", result.optimized_xpath);
        
        // 应使用锚点定位
        assert!(result.summary.used_anchor);
        // 应使用 starts-with 处理动态类名
        assert!(result.optimized_xpath.contains("starts-with(@ClassName"));
    }
    
    #[test]
    fn test_is_dynamic_class() {
        // uiauto-xpath 的动态类名检测
        assert!(is_dynamic_class("chatmainPagewilLn mainPageCtrl chatmainPageWinyRJfh"));
        assert!(!is_dynamic_class("BrowserRootView"));
    }
    
    #[test]
    fn test_extract_stable_prefix() {
        let prefix = extract_stable_prefix("temp-dialogue-btnBOp4");
        println!("prefix: {}", prefix);
        assert!(prefix.contains("temp-dialogue"));
    }
    
    #[test]
    fn test_yuanbao_real_optimization() {
        let optimizer = XPathOptimizer::new();
        
        // 元宝聊天窗口真实层级
        let hierarchy = vec![
            HierarchyNode::new("Pane", "", "Chrome_WidgetWin_1", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Document", "RootWebArea", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new(
                "Group", "",
                "temp-dialogue-btnBOp4 winFolder_optionsZJ07f t-popup-open",
                "新建对话",
                0, ElementRect::default(), 0
            ),
        ];
        
        let result = optimizer.optimize(&hierarchy);
        
        println!("=== 元宝窗口优化 ===");
        println!("optimized_xpath: {}", result.optimized_xpath);
        println!("minimal_xpath: {}", result.minimal_xpath);
        println!("compression: {:.1}%", result.summary.compression_ratio * 100.0);
        
        // 压缩率应大于 70%
        assert!(result.summary.compression_ratio > 0.5);
    }
}