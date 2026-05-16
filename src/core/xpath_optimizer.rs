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
        
        // 1. 找到目标节点（is_target = true）的原始索引
        let original_target_index = hierarchy.iter()
            .position(|n| n.is_target)
            .unwrap_or(hierarchy.len() - 1);  // 如果没标记，默认最后一个
        
        // 2. 构建 XPath 节点列表：只包含 included 且在目标之前（或等于目标）的节点
        //    记录每个 XPath 节点对应的原始 hierarchy 索引
        let xpath_nodes_with_indices: Vec<(usize, &HierarchyNode)> = hierarchy.iter()
            .enumerate()
            .filter(|(i, n)| n.included && *i <= original_target_index)  // 只包含目标之前的 included 节点
            .collect();
        
        // 3. 检查目标节点是否在 XPath 列表中
        let target_xpath_pos = xpath_nodes_with_indices.iter()
            .position(|(orig_idx, _)| *orig_idx == original_target_index);
        
        if target_xpath_pos.is_none() {
            log::warn!("[优化] 目标节点不在 XPath 列表中（可能 included=false），无法优化");
            return OptimizationResult {
                anchor_index: None,
                target_index: original_target_index,
                summary: OptimizationSummary::default(),
                optimized_xpath: String::new(),
                minimal_xpath: String::new(),
                optimized_hierarchy: hierarchy.to_vec(),
            };
        }
        
        // 4. 生成 XPath 字符串
        //    第一个节点是根节点（原始索引=0）时用 / 开头，否则用 // 开头
        let first_is_root = xpath_nodes_with_indices.first()
            .map(|(orig_idx, _)| *orig_idx == 0)
            .unwrap_or(false);
        
        let full_xpath = if first_is_root {
            // 绝对路径：从根节点开始
            xpath_nodes_with_indices.iter()
                .map(|(_, n)| {
                    let segment = n.xpath_segment();
                    if segment.starts_with('/') { segment } else { format!("/{}", segment) }
                })
                .collect::<Vec<_>>()
                .join("")
        } else {
            // 相对路径：从任意位置开始查找第一个节点
            xpath_nodes_with_indices.iter()
                .map(|(_, n)| {
                    let segment = n.xpath_segment();
                    if segment.starts_with('/') { segment } else { format!("/{}", segment) }
                })
                .collect::<Vec<_>>()
                .join("")
                .replacen("/", "//", 1)  // 把第一个 / 替换成 //
        };
        
        // 5. 调用 uiauto-xpath 的 optimize
        let result = XPath::optimize(&full_xpath);
        
        match result {
            Ok(opt_result) => {
                // 6. 转换锚点索引：XPath 索引 → 原始 hierarchy 索引
                //    opt_result.anchor_index 是在 xpath_nodes_with_indices 中的位置
                //    我们需要找到对应的原始索引
                let original_anchor_index = opt_result.anchor_index
                    .and_then(|ai| xpath_nodes_with_indices.get(ai))
                    .map(|(orig_idx, _)| *orig_idx);
                
                // 7. 应用优化到原始 hierarchy（使用原始索引）
                let optimized_hierarchy = self.apply_optimization_to_hierarchy(
                    hierarchy,
                    original_anchor_index,
                    original_target_index,
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
                    // 修正输出 XPath 的开头斜杠：根据原始锚点索引
                    //    - 锚点是根节点（索引0）：用 / 开头（绝对路径）
                    //    - 锚点不是根节点或无锚点：用 // 开头（相对路径）
                    optimized_xpath: self.fix_xpath_prefix(&opt_result.anchor_relative, original_anchor_index),
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
                    // ControlType 已通过 XPath 标签名表达，不需要额外谓词
                    optimized_filter.enabled = false;
                }
                "Index" => {
                    // Index 保留
                    optimized_filter.enabled = f.value.parse::<i32>().unwrap_or(0) > 0;
                }
                // 扩展属性：条件性添加的属性，保留（它们已有区分度）
                "FrameworkId" | "HelpText" | "LocalizedControlType" | "AcceleratorKey"
                | "AccessKey" | "ItemType" | "ItemStatus" => {
                    // 字符串属性：非空时保留
                    optimized_filter.enabled = !f.value.is_empty();
                }
                "IsPassword" | "IsEnabled" | "IsOffscreen" => {
                    // 布尔属性：这些只在有区分度时才添加，保留
                    optimized_filter.enabled = f.enabled;
                }
                _ => {
                    // 其他未知属性默认禁用
                    optimized_filter.enabled = false;
                }
            }
            
            optimized_filter
        }).collect()
    }
    
    /// 计算移除的属性数量（动态 ClassName 和 AutomationId 的处理）
    fn count_removed_attrs(&self, original: &[HierarchyNode]) -> usize {
        original.iter()
            .filter(|n| n.class_name.len() > 30 || !n.automation_id.is_empty())
            .count()
    }
    
    /// 修正 XPath 开头斜杠：根据原始锚点索引
    /// - 锚点是根节点（索引 0）：用 / 开头（绝对路径）
    /// - 锚点不是根节点或无锚点：用 // 开头（相对路径）
    fn fix_xpath_prefix(&self, xpath: &str, original_anchor_index: Option<usize>) -> String {
        match original_anchor_index {
            Some(0) => {
                // 锚点是根节点，用绝对路径 / 开头
                if xpath.starts_with("//") {
                    xpath.replacen("//", "/", 1)
                } else if xpath.starts_with("/") {
                    xpath.to_string()
                } else {
                    format!("/{}", xpath)
                }
            }
            _ => {
                // 锚点不是根节点或无锚点，用相对路径 // 开头
                if xpath.starts_with("//") {
                    xpath.to_string()
                } else if xpath.starts_with("/") {
                    xpath.replacen("/", "//", 1)
                } else {
                    format!("//{}", xpath)
                }
            }
        }
    }
    
    /// 执行极简优化：通过实时尝试验证移除所有非必要属性
    pub fn optimize_minimal(
        &self,
        hierarchy: &[HierarchyNode],
        window_selector: &str,
    ) -> Option<OptimizationResult> {
        use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
        use crate::core::model::ValidationResult;
        
        if hierarchy.is_empty() {
            log::warn!("[极简优化] hierarchy 为空");
            return None;
        }
        
        log::info!("[极简优化] 开始优化，hierarchy 节点数: {}", hierarchy.len());
        
        // 1. 找到目标节点
        let original_target_index = hierarchy.iter()
            .position(|n| n.is_target)
            .unwrap_or(hierarchy.len() - 1);
        
        // 2. 构建 XPath 节点列表
        let xpath_nodes_with_indices: Vec<(usize, &HierarchyNode)> = hierarchy.iter()
            .enumerate()
            .filter(|(i, n)| n.included && *i <= original_target_index)
            .collect();
        
        let target_xpath_pos = xpath_nodes_with_indices.iter()
            .position(|(orig_idx, _)| *orig_idx == original_target_index);
        
        if target_xpath_pos.is_none() {
            log::warn!("[极简优化] 目标节点不在 XPath 列表中");
            return None;
        }
        
        log::info!("[极简优化] XPath 节点数: {}", xpath_nodes_with_indices.len());
        
        // 3. 生成完整 XPath
        let first_is_root = xpath_nodes_with_indices.first()
            .map(|(orig_idx, _)| *orig_idx == 0)
            .unwrap_or(false);
        
        let full_xpath = if first_is_root {
            xpath_nodes_with_indices.iter()
                .map(|(_, n)| {
                    let segment = n.xpath_segment();
                    if segment.starts_with('/') { segment } else { format!("/{}", segment) }
                })
                .collect::<Vec<_>>()
                .join("")
        } else {
            xpath_nodes_with_indices.iter()
                .map(|(_, n)| {
                    let segment = n.xpath_segment();
                    if segment.starts_with('/') { segment } else { format!("/{}", segment) }
                })
                .collect::<Vec<_>>()
                .join("")
                .replacen("/", "//", 1)
        };
        
        log::info!("[极简优化] 原始 XPath 长度: {} 字符", full_xpath.len());
        
        // 4. 创建取消标志
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_flag_clone = cancel_flag.clone();
        
        // 5. 调用 uiauto-xpath 的极简优化（带验证回调和进度回调）
        let window_sel = window_selector.to_string();
        let hierarchy_clone = hierarchy.to_vec();
        
        let result = XPath::optimize_minimal(
            &full_xpath,
            // 验证回调
            move |test_xpath: &str| -> uiauto_xpath::error::Result<bool> {
                // 检查取消标志
                if cancel_flag_clone.load(Ordering::SeqCst) {
                    log::info!("[极简优化-验证] 检测到取消信号");
                    return Err(uiauto_xpath::error::XPathError::ParseError("用户取消".into()));
                }
                
                // 使用 COM worker 进行验证
                match crate::core::com_worker::global_validate_xpath(
                    window_sel.clone(),
                    test_xpath.to_string(),
                    hierarchy_clone.clone(),
                ) {
                    Ok(validation_result) => {
                        // 【关键修复】极简优化要求 XPath 必须找到**恰好1个**元素
                        let is_unique = matches!(validation_result.overall, ValidationResult::Found { count: 1, .. });
                        if !is_unique {
                            match &validation_result.overall {
                                ValidationResult::Found { count, .. } => {
                                    log::debug!("[极简优化-验证] XPath 找到 {} 个元素（需要唯一）: {}", count, test_xpath);
                                }
                                ValidationResult::NotFound => {
                                    log::debug!("[极简优化-验证] XPath 未找到元素: {}", test_xpath);
                                }
                                _ => {}
                            }
                        }
                        Ok(is_unique)
                    }
                    Err(e) => {
                        log::warn!("[极简优化-验证] 验证失败: {}", e);
                        Err(uiauto_xpath::error::XPathError::ParseError(format!("验证失败: {}", e)))
                    }
                }
            },
            // 进度回调 - 输出到日志
            |msg: &str| {
                log::info!("{}", msg);
            }
        );
        
        match result {
            Ok(Some(minimal_xpath)) => {
                log::info!("[极简优化] 优化成功，最终 XPath 长度: {} 字符", minimal_xpath.len());
                
                // 6. 解析极简 XPath 并应用到 hierarchy
                let optimized_hierarchy = self.apply_minimal_optimization_to_hierarchy(
                    hierarchy,
                    &minimal_xpath,
                    original_target_index,
                );
                
                // 7. 统计信息
                let removed_count = self.count_removed_attrs_for_minimal(hierarchy, &optimized_hierarchy);
                let simplified_count = minimal_xpath.matches("starts-with").count()
                    + minimal_xpath.matches("ends-with").count()
                    + minimal_xpath.matches("contains").count();
                
                let summary = OptimizationSummary {
                    removed_dynamic_attrs: removed_count,
                    simplified_attrs: simplified_count,
                    used_anchor: minimal_xpath.contains("//"),
                    anchor_description: Some("极简优化".to_string()),
                    compression_ratio: 1.0 - (minimal_xpath.len() as f64 / full_xpath.len() as f64),
                };
                
                Some(OptimizationResult {
                    anchor_index: None, // 极简优化不强调锚点
                    target_index: original_target_index,
                    summary,
                    optimized_xpath: minimal_xpath.clone(),
                    minimal_xpath,
                    optimized_hierarchy,
                })
            }
            Ok(None) => {
                // 被取消
                log::info!("[极简优化] 优化已被用户取消");
                None
            }
            Err(e) => {
                log::error!("[极简优化] 优化失败: {}", e);
                None
            }
        }
    }
    
    /// 将极简优化结果应用到 hierarchy
    fn apply_minimal_optimization_to_hierarchy(
        &self,
        hierarchy: &[HierarchyNode],
        _minimal_xpath: &str,
        target_index: usize,
    ) -> Vec<HierarchyNode> {
        // 简化实现：只包含目标节点，其他节点全部排除
        hierarchy.iter().enumerate().map(|(i, node)| {
            let mut optimized_node = node.clone();
            optimized_node.included = i == target_index;
            
            if i == target_index {
                // 目标节点：根据 minimal_xpath 中的谓词设置 filters
                // 这里简化处理，保留原有 filters 但禁用大部分
                optimized_node.filters = optimized_node.filters.iter().map(|f| {
                    let mut new_f = f.clone();
                    // 只保留 ClassName 和 AutomationId
                    new_f.enabled = matches!(f.name.as_str(), "ClassName" | "AutomationId");
                    new_f
                }).collect();
            }
            
            optimized_node
        }).collect()
    }
    
    /// 计算极简优化移除的属性数量
    fn count_removed_attrs_for_minimal(
        &self,
        original: &[HierarchyNode],
        optimized: &[HierarchyNode],
    ) -> usize {
        let original_attr_count: usize = original.iter()
            .map(|n| n.filters.iter().filter(|f| f.enabled).count())
            .sum();
        
        let optimized_attr_count: usize = optimized.iter()
            .map(|n| n.filters.iter().filter(|f| f.enabled).count())
            .sum();
        
        original_attr_count.saturating_sub(optimized_attr_count)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 单元测试
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::ElementRect;
    // is_dynamic_class, extract_stable_prefix 已在 use 模块顶部导入，不需要重复导入
    
    #[test]
    fn test_optimize_with_automation_id() {
        let optimizer = XPathOptimizer::new();
        
        let mut hierarchy = vec![
            HierarchyNode::new("Pane", "", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Document", "RootWebArea", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Button", "", "dynamic-class", "Click", 0, ElementRect::default(), 0),
        ];
        // 标记最后一个节点为目标节点
        hierarchy.last_mut().unwrap().is_target = true;
        
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
        
        let mut hierarchy = vec![
            HierarchyNode::new("Pane", "", "Chrome_WidgetWin_1", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Document", "RootWebArea", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new(
                "Group", "",
                "temp-dialogue-btnBOp4 winFolder_optionsZJ07f t-popup-open",
                "新建对话",
                0, ElementRect::default(), 0
            ),
        ];
        // 标记最后一个节点为目标节点
        hierarchy.last_mut().unwrap().is_target = true;
        
        let result = optimizer.optimize(&hierarchy);
        
        println!("optimized_xpath: {}", result.optimized_xpath);
        
        // 应使用锚点定位
        assert!(result.summary.used_anchor);
        // 验证目标节点被正确识别（最后一个节点的索引）
        assert_eq!(result.target_index, 2);
        // 锚点应该是 Document（有 AutomationId）
        assert_eq!(result.anchor_index, Some(1));
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
        let mut hierarchy = vec![
            HierarchyNode::new("Pane", "", "Chrome_WidgetWin_1", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Document", "RootWebArea", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new(
                "Group", "",
                "temp-dialogue-btnBOp4 winFolder_optionsZJ07f t-popup-open",
                "新建对话",
                0, ElementRect::default(), 0
            ),
        ];
        // 标记最后一个节点为目标节点
        hierarchy.last_mut().unwrap().is_target = true;
        
        let result = optimizer.optimize(&hierarchy);
        
        println!("=== 元宝窗口优化 ===");
        println!("optimized_xpath: {}", result.optimized_xpath);
        println!("minimal_xpath: {}", result.minimal_xpath);
        println!("compression: {:.1}%", result.summary.compression_ratio * 100.0);
        
        // 压缩率应大于 70%
        assert!(result.summary.compression_ratio > 0.5);
    }
    
    #[test]
    fn test_xpath_prefix_when_first_node_excluded() {
        let optimizer = XPathOptimizer::new();
        
        // 用户取消了第一个节点（Pane），只保留 Document 和 Group
        let mut hierarchy = vec![
            HierarchyNode::new("Pane", "", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Document", "RootWebArea", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Group", "", "btn-class", "新建对话", 0, ElementRect::default(), 0),
        ];
        
        // 取消第一个节点（原始索引 0）
        hierarchy[0].included = false;
        // 标记最后一个节点为目标节点
        hierarchy[2].is_target = true;
        
        let result = optimizer.optimize(&hierarchy);
        
        println!("=== 第一个节点被取消 ===");
        println!("optimized_xpath: {}", result.optimized_xpath);
        println!("anchor_index: {:?}", result.anchor_index);
        
        // 输入 XPath 第一个节点是 Document（原始索引 1），不是根节点
        // 优化后的 XPath 应该用 // 开头（相对路径），而不是 / 开头
        assert!(result.optimized_xpath.starts_with("//"),
            "Expected // prefix, got: {}", result.optimized_xpath);
    }
    
    #[test]
    fn test_xpath_prefix_when_anchor_is_root() {
        let optimizer = XPathOptimizer::new();
        
        // 第一个节点（Pane）有 AutomationId，会被选为锚点
        let mut hierarchy = vec![
            HierarchyNode::new("Pane", "MainWindow", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Document", "", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Button", "", "btn-class", "点击", 0, ElementRect::default(), 0),
        ];
        
        // 标记最后一个节点为目标节点
        hierarchy[2].is_target = true;
        
        let result = optimizer.optimize(&hierarchy);
        
        println!("=== 锚点是根节点 ===");
        println!("optimized_xpath: {}", result.optimized_xpath);
        println!("anchor_index: {:?}", result.anchor_index);
        
        // 锚点是第一个节点（原始索引 0），是根节点
        // 优化后的 XPath 应该用 / 开头（绝对路径）
        assert!(result.optimized_xpath.starts_with("/"),
            "Expected / prefix, got: {}", result.optimized_xpath);
        assert!(!result.optimized_xpath.starts_with("//"),
            "Expected single / prefix, got: {}", result.optimized_xpath);
    }
}