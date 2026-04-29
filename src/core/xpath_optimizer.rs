// src/core/xpath_optimizer.rs
//
// XPath智能优化器：直接调用 uiauto-xpath 库的 optimize 函数
//
// 设计原则：高内聚、模块化
// - 优化逻辑全部在 uiauto-xpath 库中
// - 本模块只做数据格式转换：HierarchyNode ↔ XPath 字符串
// - 统计信息直接由 uiauto-xpath 库提供

use super::model::HierarchyNode;
use uiauto_xpath::XPath;

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
    /// 注意：此方法不修改 hierarchy，只返回优化后的 XPath 字符串和索引信息
    /// 所有统计信息由 uiauto-xpath 库提供，保持高内聚
    pub fn optimize(&self, hierarchy: &[HierarchyNode]) -> OptimizationResult {
        if hierarchy.is_empty() {
            return OptimizationResult {
                anchor_index: None,
                target_index: 0,
                summary: OptimizationSummary::default(),
                optimized_xpath: String::new(),
                minimal_xpath: String::new(),
            };
        }
        
        // 生成完整 XPath 字符串
        let full_xpath = self.generate_full_xpath(hierarchy);
        
        // 调用 uiauto-xpath 的 optimize
        let result = XPath::optimize(&full_xpath);
        
        match result {
            Ok(opt_result) => {
                // 直接使用 uiauto-xpath 提供的索引和统计信息
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
                }
            }
            Err(_) => {
                // optimize 失败时返回原始 XPath
                let xpath = full_xpath.clone();
                OptimizationResult {
                    anchor_index: None,
                    target_index: hierarchy.len() - 1,
                    summary: OptimizationSummary::default(),
                    optimized_xpath: xpath.clone(),
                    minimal_xpath: xpath.clone(),
                }
            }
        }
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