// src/core/xpath_optimizer.rs
//
// XPath智能优化器：分析属性稳定性，简化XPath以提高可验证性和可读性

use super::model::{HierarchyNode, Operator, PropertyFilter};

/// ClassName 处理策略
#[derive(Debug, Clone, PartialEq)]
pub enum ClassStrategy {
    /// 完整保留：@ClassName='xxx'
    Keep,
    /// 前缀匹配：starts-with(@ClassName, 'xxx')
    Prefix(String),
    /// 后缀匹配：ends-with(@ClassName, 'xxx')
    Suffix(String),
    /// 禁用
    Disable,
}

/// Name 匹配策略
#[derive(Debug, Clone, PartialEq)]
pub enum NameStrategy {
    /// 精确匹配：@Name='xxx'
    Exact,
    /// 前缀匹配：starts-with(@Name, 'xxx')
    Prefix(String),
    /// 后缀匹配：ends-with(@Name, 'xxx')
    Suffix(String),
    /// 包含匹配：contains(@Name, 'xxx')
    Contains(String),
}

/// 锚点强度等级
#[derive(Debug, Clone, PartialEq)]
pub enum AnchorStrength {
    Strong,   // AutomationId 存在
    Medium,   // ClassName 稳定
    Weak,     // Name 稳定
}

/// 单节点优化配置
#[derive(Debug, Clone)]
pub struct NodeOptimization {
    /// 节点索引
    pub index: usize,
    /// 是否使用 AutomationId
    pub use_automation_id: bool,
    /// Name 处理策略
    pub name_strategy: Option<NameStrategy>,
    /// ClassName 处理策略
    pub class_strategy: ClassStrategy,
    /// 是否需要 position 约束
    pub use_position: bool,
}

/// 优化摘要（用于 UI 显示）
#[derive(Debug, Clone, Default)]
pub struct OptimizationSummary {
    /// 移除的动态属性数量
    pub removed_dynamic_attrs: usize,
    /// 简化的属性数量（改为 starts-with 等）
    pub simplified_attrs: usize,
    /// 是否使用了锚点定位
    pub used_anchor: bool,
    /// 锚点描述（如果有）
    pub anchor_description: Option<String>,
}

/// 优化结果
#[derive(Debug, Clone)]
pub struct OptimizationResult {
    /// 优化后的 hierarchy（filters 已修改）
    pub hierarchy: Vec<HierarchyNode>,
    /// 锚点索引（如果有）
    pub anchor_index: Option<usize>,
    /// 优化摘要
    pub summary: OptimizationSummary,
}

/// XPath智能优化器
pub struct XPathOptimizer {
    /// 动态状态词库
    dynamic_keywords: Vec<String>,
    /// 稳定框架类名模式
    stable_class_patterns: Vec<String>,
    /// UI 类型结尾词（用于 Name 可读性优化）
    ui_type_suffixes: Vec<String>,
}

impl Default for XPathOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

impl XPathOptimizer {
    /// 创建默认配置的优化器
    pub fn new() -> Self {
        Self {
            dynamic_keywords: vec![
                // 交互状态
                "active".to_string(), "hover".to_string(), "focus".to_string(), "focused".to_string(),
                "pressed".to_string(), "selected".to_string(), "checked".to_string(),
                "disabled".to_string(), "enabled".to_string(), "readonly".to_string(),
                // 显示状态
                "open".to_string(), "closed".to_string(), "opened".to_string(),
                "expanded".to_string(), "collapsed".to_string(),
                "visible".to_string(), "hidden".to_string(), "showing".to_string(),
                "shown".to_string(), "displayed".to_string(),
                // 动画状态
                "loading".to_string(), "animating".to_string(), "transitioning".to_string(),
                "dragging".to_string(),
                // 位置状态
                "first".to_string(), "last".to_string(), "even".to_string(), "odd".to_string(),
                "top".to_string(), "bottom".to_string(), "left".to_string(), "right".to_string(),
                // 临时状态
                "t-popup".to_string(), "popup".to_string(), "modal".to_string(),
                "dialog".to_string(), "toast".to_string(),
            ],
            stable_class_patterns: vec![
                // Chrome/Electron
                "Chrome_WidgetWin_".to_string(), "BrowserRootView".to_string(),
                "NonClientView".to_string(), "EmbeddedBrowserFrameView".to_string(),
                "BrowserView".to_string(),
                // Qt
                "QWidget".to_string(), "QStackedWidget".to_string(), "QFrame".to_string(),
                "QLabel".to_string(), "QPushButton".to_string(),
                // Win32/WPF
                "mmui::".to_string(), "Qt5QWindow".to_string(), "WindowClass".to_string(),
                "ButtonClass".to_string(),
                // 标准 Windows 类名
                "Edit".to_string(), "Button".to_string(), "ListBox".to_string(),
                "ComboBox".to_string(), "Static".to_string(),
                // Tauri
                "Tauri Window".to_string(),
            ],
            ui_type_suffixes: vec![
                // 中文 UI 类型词
                "按钮".to_string(), "输入框".to_string(), "文本框".to_string(),
                "链接".to_string(), "列表".to_string(), "面板".to_string(), "菜单".to_string(),
                "图标".to_string(), "提示".to_string(), "标签".to_string(), "窗口".to_string(),
                "对话框".to_string(), "工具栏".to_string(),
                // 英文 UI 类型词
                "Button".to_string(), "Btn".to_string(), "Link".to_string(),
                "Input".to_string(), "Edit".to_string(), "List".to_string(), "Panel".to_string(),
                "Menu".to_string(), "Icon".to_string(), "Tip".to_string(), "Label".to_string(),
                "Window".to_string(), "Dialog".to_string(), "Toolbar".to_string(),
                "Tab".to_string(), "Item".to_string(), "Header".to_string(), "Footer".to_string(),
            ],
        }
    }
    
    /// 执行完整优化
    pub fn optimize(&self, hierarchy: &[HierarchyNode]) -> OptimizationResult {
        if hierarchy.is_empty() {
            return OptimizationResult {
                hierarchy: Vec::new(),
                anchor_index: None,
                summary: OptimizationSummary::default(),
            };
        }
        
        // Phase 1: 锚点识别
        let anchor = self.find_anchor(hierarchy);
        
        // Phase 2: 逐节点优化分析
        let optimizations = self.analyze_nodes(hierarchy);
        
        // Phase 3: 应用优化到 hierarchy
        let optimized_hierarchy = self.apply_optimizations(hierarchy, &optimizations);
        
        // Phase 4: 生成摘要
        let summary = self.generate_summary(&optimizations, &anchor, hierarchy);
        
        OptimizationResult {
            hierarchy: optimized_hierarchy,
            anchor_index: anchor.map(|(idx, _)| idx),
            summary,
        }
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // Phase 1: 锚点识别
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// 找到最近的稳定锚点节点
    fn find_anchor(&self, hierarchy: &[HierarchyNode]) -> Option<(usize, AnchorStrength)> {
        // 从叶子向上搜索第一个稳定节点（排除叶子本身）
        for i in (0..hierarchy.len().saturating_sub(1)).rev() {
            let node = &hierarchy[i];
            
            // Strong: 有 AutomationId
            if !node.automation_id.is_empty() {
                return Some((i, AnchorStrength::Strong));
            }
            
            // Medium: 有稳定 ClassName
            if !node.class_name.is_empty() && self.is_stable_class(&node.class_name) {
                return Some((i, AnchorStrength::Medium));
            }
            
            // Weak: 有稳定 Name（非空且无动态特征）
            if !node.name.is_empty() && !self.has_dynamic_features(&node.name) {
                return Some((i, AnchorStrength::Weak));
            }
        }
        None
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // Phase 2: 节点级优化分析
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// 分析所有节点的优化策略
    fn analyze_nodes(&self, hierarchy: &[HierarchyNode]) -> Vec<NodeOptimization> {
        hierarchy
            .iter()
            .enumerate()
            .map(|(i, node)| self.analyze_single_node(i, node, hierarchy.len()))
            .collect()
    }
    
    /// 分析单个节点的优化策略
    fn analyze_single_node(&self, index: usize, node: &HierarchyNode, total: usize) -> NodeOptimization {
        // 叶子节点：保留更多属性以确保唯一性
        let is_leaf = index == total - 1;
        
        // AutomationId：优先使用
        let use_automation_id = !node.automation_id.is_empty();
        
        // Name 策略分析
        let name_strategy = if !use_automation_id && !node.name.is_empty() {
            Some(self.analyze_name(&node.name, is_leaf))
        } else {
            None
        };
        
        // ClassName 策略分析
        let class_strategy = if !use_automation_id {
            self.classify_classname(&node.class_name, is_leaf)
        } else {
            ClassStrategy::Disable // 有 AutomationId 时禁用 ClassName
        };
        
        // Position：叶子节点或无其他标识时使用
        let use_position = is_leaf && !use_automation_id 
            && node.name.is_empty() 
            && matches!(class_strategy, ClassStrategy::Disable);
        
        NodeOptimization {
            index,
            use_automation_id,
            name_strategy,
            class_strategy,
            use_position,
        }
    }
    
    /// 分析 Name 属性，选择最佳匹配策略
    fn analyze_name(&self, name: &str, is_leaf: bool) -> NameStrategy {
        if name.is_empty() {
            return NameStrategy::Exact;
        }
        
        // 叶子节点：优先精确匹配（确保唯一性）
        if is_leaf && name.len() <= 30 {
            // 检测是否含动态特征
            if !self.has_dynamic_features(name) {
                return NameStrategy::Exact;
            }
        }
        
        // 检测 UI 类型结尾 → 使用 ends-with（提升可读性）
        for suffix in &self.ui_type_suffixes {
            if name.ends_with(suffix) {
                // 确保后缀前有有意义内容
                let prefix_len = name.len().saturating_sub(suffix.len());
                if prefix_len > 0 {
                    return NameStrategy::Suffix(suffix.clone());
                }
            }
        }
        
        // 检测语义前缀（前 N 字符有意义）→ 使用 starts-with
        let prefix_len = self.detect_meaningful_prefix(name);
        if prefix_len > 0 && prefix_len < name.chars().count() {
            let prefix: String = name.chars().take(prefix_len).collect();
            return NameStrategy::Prefix(prefix);
        }
        
        // 检测动态特征（数字、特殊字符）→ 使用 contains
        if self.has_dynamic_features(name) {
            // 提取稳定关键词
            let keyword = self.extract_stable_keyword(name);
            if !keyword.is_empty() {
                return NameStrategy::Contains(keyword);
            }
        }
        
        // 默认精确匹配
        NameStrategy::Exact
    }
    
    /// 分析 ClassName 稳定性
    fn classify_classname(&self, class_name: &str, is_leaf: bool) -> ClassStrategy {
        if class_name.is_empty() {
            return ClassStrategy::Disable;
        }
        
        // 叶子节点：更严格，优先保留完整值
        if is_leaf && class_name.len() <= 30 && !self.has_dynamic_state(class_name) {
            return ClassStrategy::Keep;
        }
        
        // 检测稳定框架类名 → 完整保留
        if self.is_stable_class(class_name) {
            return ClassStrategy::Keep;
        }
        
        // 检测动态状态词 → 提取前缀或禁用
        if self.has_dynamic_state(class_name) {
            let prefix = self.extract_stable_prefix(class_name);
            if !prefix.is_empty() && prefix.len() >= 3 {
                return ClassStrategy::Prefix(prefix);
            }
            return ClassStrategy::Disable;
        }
        
        // 检测随机哈希 → 提取前缀
        if self.has_random_hash(class_name) {
            let prefix = self.extract_stable_prefix(class_name);
            if !prefix.is_empty() && prefix.len() >= 3 {
                return ClassStrategy::Prefix(prefix);
            }
            return ClassStrategy::Disable;
        }
        
        // 长度过长 → 提取前缀
        if class_name.len() > 50 {
            let prefix = self.extract_stable_prefix(class_name);
            if !prefix.is_empty() && prefix.len() >= 3 {
                return ClassStrategy::Prefix(prefix);
            }
        }
        
        // 多词组合：提取第一个稳定词
        let parts: Vec<&str> = class_name.split_whitespace().collect();
        if parts.len() > 2 {
            for part in &parts {
                if !self.has_dynamic_state(part) && !self.has_random_hash(part) && part.len() >= 3 {
                    return ClassStrategy::Prefix(part.to_string());
                }
            }
        }
        
        // 默认保留
        ClassStrategy::Keep
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // 辅助检测函数
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// 检测是否为稳定框架类名
    fn is_stable_class(&self, class_name: &str) -> bool {
        for pattern in &self.stable_class_patterns {
            if class_name.starts_with(pattern) || class_name == pattern {
                return true;
            }
        }
        false
    }
    
    /// 检测是否含动态状态词
    fn has_dynamic_state(&self, text: &str) -> bool {
        let text_lower = text.to_lowercase();
        for keyword in &self.dynamic_keywords {
            if text_lower.contains(&keyword.to_lowercase()) {
                return true;
            }
        }
        false
    }
    
    /// 检测是否含随机哈希（CSS Modules / Styled Components）
    fn has_random_hash(&self, text: &str) -> bool {
        // CSS Modules 哈希特征：___XXXX 或 __XXXX
        if text.contains("___") || text.contains("__") {
            // 检查后面是否有疑似哈希（3-6字符的大写字母+数字组合）
            for sep in ["___", "__"] {
                if let Some(pos) = text.find(sep) {
                    let after = &text[pos + sep.len()..];
                    // 哈希特征：3-6字符，含大写字母或数字
                    if after.len() <= 6 && after.chars().any(|c| c.is_uppercase() || c.is_numeric()) {
                        return true;
                    }
                }
            }
        }
        
        // Styled Components 哈希：_XXXX 或 sc-XXXX
        if text.contains("_") && text.len() > 10 {
            let parts: Vec<&str> = text.split('_').collect();
            for part in parts {
                if part.len() <= 6 && part.chars().all(|c| c.is_alphanumeric()) {
                    return true;
                }
            }
        }
        
        false
    }
    
    /// 检测是否含动态特征（数字、状态词等）
    fn has_dynamic_features(&self, text: &str) -> bool {
        // 含动态状态词
        if self.has_dynamic_state(text) {
            return true;
        }
        
        // 含大量数字（可能是计数器、时间戳）
        let digit_count = text.chars().filter(|c| c.is_numeric()).count();
        if digit_count > text.len() / 3 {
            return true;
        }
        
        false
    }
    
    /// 提取 ClassName 的稳定前缀
    fn extract_stable_prefix(&self, class_name: &str) -> String {
        // 方法1：分割空格，取第一个不含动态特征的词
        let parts: Vec<&str> = class_name.split_whitespace().collect();
        for part in &parts {
            if !self.has_dynamic_state(part) && !self.has_random_hash(part) && part.len() >= 3 {
                // 进一步提取哈希前的部分
                let clean = self.extract_before_hash(part);
                if !clean.is_empty() && clean.len() >= 3 {
                    return clean;
                }
            }
        }
        
        // 方法2：取到第一个分隔符（___、__、_）之前
        for sep in ["___", "__", "_"] {
            if let Some(pos) = class_name.find(sep) {
                let prefix = &class_name[..pos];
                // 移除可能的重复词（如 temp-dialogue-btn_temp-dialogue-btn）
                let clean = self.remove_duplicate_parts(prefix);
                if !clean.is_empty() && clean.len() >= 3 {
                    return clean;
                }
            }
        }
        
        // 方法3：取前 20 字符作为前缀（至少保留一些信息）
        let chars: Vec<char> = class_name.chars().collect();
        if chars.len() > 20 {
            chars.iter().take(20).collect()
        } else {
            String::new()
        }
    }
    
    /// 提取哈希分隔符前的部分
    fn extract_before_hash(&self, part: &str) -> String {
        for sep in ["___", "__", "_"] {
            if let Some(pos) = part.find(sep) {
                return part[..pos].to_string();
            }
        }
        part.to_string()
    }
    
    /// 移除重复的命名部分（如 temp-dialogue-btn_temp-dialogue-btn → temp-dialogue-btn）
    fn remove_duplicate_parts(&self, prefix: &str) -> String {
        // 检测并移除重复的 _ 分隔的词
        let parts: Vec<&str> = prefix.split('_').collect();
        if parts.len() >= 2 {
            // 检查是否有重复部分
            let mut unique_parts: Vec<&str> = Vec::new();
            for part in &parts {
                if !unique_parts.contains(part) {
                    unique_parts.push(part);
                }
            }
            // 如果去重后比原来少，说明有重复
            if unique_parts.len() < parts.len() {
                return unique_parts.join("-");
            }
        }
        prefix.to_string()
    }
    
    /// 检测 Name 的有意义前缀长度
    fn detect_meaningful_prefix(&self, name: &str) -> usize {
        let chars: Vec<char> = name.chars().collect();
        
        // 查找语义分隔点
        for (i, &c) in chars.iter().enumerate() {
            // 分隔符：空格、冒号、括号、斜杠
            if c == ' ' || c == ':' || c == '(' || c == '[' || c == '/' {
                // 前面部分有意义且长度足够
                if i >= 2 && i <= 10 {
                    return i;
                }
            }
        }
        
        0
    }
    
    /// 从 Name 中提取稳定关键词
    fn extract_stable_keyword(&self, name: &str) -> String {
        // 移除数字和特殊字符，提取中文/英文词
        let cleaned: String = name
            .chars()
            .filter(|c| c.is_alphabetic())
            .collect();
        
        if cleaned.len() >= 2 {
            cleaned
        } else {
            String::new()
        }
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // Phase 3: 应用优化
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// 将优化策略应用到 hierarchy
    fn apply_optimizations(&self, hierarchy: &[HierarchyNode], optimizations: &[NodeOptimization]) -> Vec<HierarchyNode> {
        hierarchy
            .iter()
            .enumerate()
            .map(|(i, node)| {
                let opt = &optimizations[i];
                self.apply_node_optimization(node, opt)
            })
            .collect()
    }
    
    /// 应用单节点优化
    fn apply_node_optimization(&self, node: &HierarchyNode, opt: &NodeOptimization) -> HierarchyNode {
        let mut new_node = node.clone();
        
        // 遍历并修改 filters
        for filter in &mut new_node.filters {
            match filter.name.as_str() {
                "AutomationId" => {
                    filter.enabled = opt.use_automation_id;
                    if opt.use_automation_id {
                        filter.operator = Operator::Equals;
                    }
                }
                "Name" => {
                    if let Some(ref strategy) = opt.name_strategy {
                        Self::apply_name_strategy(filter, strategy);
                    } else {
                        filter.enabled = false;
                    }
                }
                "ClassName" => {
                    Self::apply_class_strategy(filter, &opt.class_strategy);
                }
                "ControlType" => {
                    // ControlType 始终保留
                    filter.enabled = true;
                    filter.operator = Operator::Equals;
                }
                "Index" => {
                    filter.enabled = opt.use_position;
                }
                _ => {
                    // 其他属性默认禁用
                    filter.enabled = false;
                }
            }
        }
        
        new_node
    }
    
    /// 应用 Name 策略到 PropertyFilter
    fn apply_name_strategy(filter: &mut PropertyFilter, strategy: &NameStrategy) {
        match strategy {
            NameStrategy::Exact => {
                filter.operator = Operator::Equals;
                filter.enabled = true;
            }
            NameStrategy::Prefix(prefix) => {
                filter.operator = Operator::StartsWith;
                filter.value = prefix.clone();
                filter.enabled = true;
            }
            NameStrategy::Suffix(suffix) => {
                filter.operator = Operator::EndsWith;
                filter.value = suffix.clone();
                filter.enabled = true;
            }
            NameStrategy::Contains(keyword) => {
                filter.operator = Operator::Contains;
                filter.value = keyword.clone();
                filter.enabled = true;
            }
        }
    }
    
    /// 应用 ClassName 策略到 PropertyFilter
    fn apply_class_strategy(filter: &mut PropertyFilter, strategy: &ClassStrategy) {
        match strategy {
            ClassStrategy::Keep => {
                filter.operator = Operator::Equals;
                filter.enabled = true;
            }
            ClassStrategy::Prefix(prefix) => {
                filter.operator = Operator::StartsWith;
                filter.value = prefix.clone();
                filter.enabled = true;
            }
            ClassStrategy::Suffix(suffix) => {
                filter.operator = Operator::EndsWith;
                filter.value = suffix.clone();
                filter.enabled = true;
            }
            ClassStrategy::Disable => {
                filter.enabled = false;
            }
        }
    }
    
    // ═══════════════════════════════════════════════════════════════════════════
    // Phase 4: 生成摘要
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// 生成优化摘要
    fn generate_summary(
        &self,
        optimizations: &[NodeOptimization],
        anchor: &Option<(usize, AnchorStrength)>,
        hierarchy: &[HierarchyNode],
    ) -> OptimizationSummary {
        let mut removed_dynamic_attrs = 0;
        let mut simplified_attrs = 0;
        
        for opt in optimizations {
            // 禁用的属性计入移除
            if matches!(opt.class_strategy, ClassStrategy::Disable) {
                removed_dynamic_attrs += 1;
            }
            
            // 改为 starts-with/ends-with 的属性计入简化
            if matches!(opt.class_strategy, ClassStrategy::Prefix(_) | ClassStrategy::Suffix(_)) {
                simplified_attrs += 1;
            }
            if let Some(ref name_strat) = opt.name_strategy {
                if matches!(name_strat, NameStrategy::Prefix(_) | NameStrategy::Suffix(_) | NameStrategy::Contains(_)) {
                    simplified_attrs += 1;
                }
            }
        }
        
        let used_anchor = anchor.is_some();
        let anchor_description = anchor.as_ref().map(|(idx, strength)| {
            let node = &hierarchy[*idx];
            match strength {
                AnchorStrength::Strong => format!("AutomationId='{}'", node.automation_id),
                AnchorStrength::Medium => format!("ClassName='{}'", node.class_name),
                AnchorStrength::Weak => format!("Name='{}'", node.name),
            }
        });
        
        OptimizationSummary {
            removed_dynamic_attrs,
            simplified_attrs,
            used_anchor,
            anchor_description,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 单元测试
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::model::ElementRect;
    
    // ── 动态状态词检测 ──────────────────────────────────────────────────────────
    
    #[test]
    fn test_dynamic_state_detection() {
        let optimizer = XPathOptimizer::new();
        
        // 应检测为动态
        assert!(optimizer.has_dynamic_state("menu-item active"));
        assert!(optimizer.has_dynamic_state("btn hover selected"));
        assert!(optimizer.has_dynamic_state("panel open"));
        assert!(optimizer.has_dynamic_state("t-popup-open"));
        
        // 不应检测为动态（稳定类名）
        assert!(!optimizer.has_dynamic_state("QWidget"));
        assert!(!optimizer.has_dynamic_state("Chrome_WidgetWin_1"));
        assert!(!optimizer.has_dynamic_state("mmui::MainWindow"));
    }
    
    #[test]
    fn test_random_hash_detection() {
        let optimizer = XPathOptimizer::new();
        
        // CSS Modules 哈希
        assert!(optimizer.has_random_hash("temp-dialogue-btn___BOp4"));
        assert!(optimizer.has_random_hash("winFolder_options__ZJ07f"));
        
        // 无哈希
        assert!(!optimizer.has_random_hash("QWidget"));
        assert!(!optimizer.has_random_hash("mmui::MainWindow"));
    }
    
    // ── 前缀提取 ──────────────────────────────────────────────────────────────────
    
    #[test]
    fn test_prefix_extraction_css_modules() {
        let optimizer = XPathOptimizer::new();
        
        // CSS Modules 格式：取到 ___ 之前
        let prefix = optimizer.extract_stable_prefix("temp-dialogue-btn___BOp4");
        assert_eq!(prefix, "temp-dialogue-btn");
        
        // 多词组合：取第一个稳定词
        let prefix = optimizer.extract_stable_prefix("menu-item active selected");
        assert_eq!(prefix, "menu-item");
    }
    
    #[test]
    fn test_prefix_extraction_whitespace() {
        let optimizer = XPathOptimizer::new();
        
        // 空格分隔，取第一个非动态词
        let prefix = optimizer.extract_stable_prefix("winFolder options__ZJ07f t-popup-open");
        assert_eq!(prefix, "winFolder");
    }
    
    // ── ClassName 分类 ──────────────────────────────────────────────────────────────
    
    #[test]
    fn test_classify_stable_class() {
        let optimizer = XPathOptimizer::new();
        
        // 稳定框架类名 → 完整保留
        let strategy = optimizer.classify_classname("QWidget", false);
        assert_eq!(strategy, ClassStrategy::Keep);
        
        let strategy = optimizer.classify_classname("Chrome_WidgetWin_1", false);
        assert_eq!(strategy, ClassStrategy::Keep);
        
        let strategy = optimizer.classify_classname("mmui::MainWindow", false);
        assert_eq!(strategy, ClassStrategy::Keep);
    }
    
    #[test]
    fn test_classify_dynamic_class() {
        let optimizer = XPathOptimizer::new();
        
        // 含动态状态词 → 提取前缀
        let strategy = optimizer.classify_classname("menu-item active selected", false);
        assert_eq!(strategy, ClassStrategy::Prefix("menu-item".to_string()));
        
        // 含随机哈希 → 提取前缀
        let strategy = optimizer.classify_classname("temp-dialogue-btn___BOp4", false);
        assert_eq!(strategy, ClassStrategy::Prefix("temp-dialogue-btn".to_string()));
        
        // 复杂动态类名（实际案例）
        let strategy = optimizer.classify_classname("temp-dialogue-btn_temp-dialogue-btn___BOp4 winFolder_options__ZJ07f t-popup-open", false);
        assert!(matches!(strategy, ClassStrategy::Prefix(_)));
    }
    
    #[test]
    fn test_classify_empty_class() {
        let optimizer = XPathOptimizer::new();
        
        // 空类名 → 禁用
        let strategy = optimizer.classify_classname("", false);
        assert_eq!(strategy, ClassStrategy::Disable);
    }
    
    // ── Name 策略分析 ──────────────────────────────────────────────────────────────
    
    #[test]
    fn test_name_strategy_ui_suffix() {
        let optimizer = XPathOptimizer::new();
        
        // UI 类型结尾 → 使用 ends-with（非叶子节点时生效）
        let strategy = optimizer.analyze_name("发送消息按钮", false);
        assert_eq!(strategy, NameStrategy::Suffix("按钮".to_string()));
        
        let strategy = optimizer.analyze_name("SubmitButton", false);
        assert_eq!(strategy, NameStrategy::Suffix("Button".to_string()));
        
        // 叶子节点 + 短稳定名称 → 优先精确匹配（确保唯一性）
        let strategy = optimizer.analyze_name("发送消息按钮", true);
        assert_eq!(strategy, NameStrategy::Exact); // 叶子节点优先精确匹配
    }
    
    #[test]
    fn test_name_strategy_exact_for_leaf() {
        let optimizer = XPathOptimizer::new();
        
        // 叶子节点 + 短稳定名称 → 精确匹配
        let strategy = optimizer.analyze_name("发送", true);
        assert_eq!(strategy, NameStrategy::Exact);
        
        let strategy = optimizer.analyze_name("搜一搜", true);
        assert_eq!(strategy, NameStrategy::Exact);
    }
    
    #[test]
    fn test_name_strategy_prefix() {
        let optimizer = XPathOptimizer::new();
        
        // 含分隔符 → 使用前缀
        let strategy = optimizer.analyze_name("发送消息 - 群聊", false);
        assert_eq!(strategy, NameStrategy::Prefix("发送消息".to_string()));
    }
    
    // ── 锚点识别 ──────────────────────────────────────────────────────────────────────
    
    #[test]
    fn test_anchor_detection_automation_id() {
        let optimizer = XPathOptimizer::new();
        
        let hierarchy = vec![
            HierarchyNode::new("Group", "", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Pane", "stableId", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Button", "", "", "", 0, ElementRect::default(), 0),
        ];
        
        let anchor = optimizer.find_anchor(&hierarchy);
        let anchor_ref = anchor.as_ref();
        assert_eq!(anchor_ref.map(|(idx, _)| *idx), Some(1)); // Pane with AutomationId
        assert_eq!(anchor_ref.map(|(_, strength)| strength.clone()), Some(AnchorStrength::Strong));
    }
    
    #[test]
    fn test_anchor_detection_stable_class() {
        let optimizer = XPathOptimizer::new();
        
        let hierarchy = vec![
            HierarchyNode::new("Group", "", "QWidget", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Pane", "", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Button", "", "", "", 0, ElementRect::default(), 0),
        ];
        
        let anchor = optimizer.find_anchor(&hierarchy);
        let anchor_ref = anchor.as_ref();
        assert_eq!(anchor_ref.map(|(idx, _)| *idx), Some(0)); // Group with stable ClassName
        assert_eq!(anchor_ref.map(|(_, strength)| strength.clone()), Some(AnchorStrength::Medium));
    }
    
    #[test]
    fn test_anchor_no_stable_node() {
        let optimizer = XPathOptimizer::new();
        
        let hierarchy = vec![
            HierarchyNode::new("Pane", "", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Group", "", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Button", "", "", "", 0, ElementRect::default(), 0),
        ];
        
        let anchor = optimizer.find_anchor(&hierarchy);
        assert_eq!(anchor, None);
    }
    
    // ── 完整优化流程 ──────────────────────────────────────────────────────────────
    
    #[test]
    fn test_full_optimization_dynamic_classname() {
        let optimizer = XPathOptimizer::new();
        
        // 模拟问题节点（元宝聊天窗口捕获）
        let hierarchy = vec![
            HierarchyNode::new("Group", "", "Chrome_WidgetWin_1", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Pane", "", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new(
                "Group", "", 
                "temp-dialogue-btn_temp-dialogue-btn___BOp4 winFolder_options__ZJ07f t-popup-open",
                "新建对话", 
                1, 
                ElementRect::default(), 
                0
            ),
        ];
        
        let result = optimizer.optimize(&hierarchy);
        
        // 检查优化摘要
        assert!(result.summary.simplified_attrs > 0, "Should have simplified attributes");
        
        // 检查叶子节点的 ClassName 被简化为前缀
        let leaf_opt = &result.hierarchy[2];
        let class_filter = leaf_opt.filters.iter()
            .find(|f| f.name == "ClassName")
            .unwrap();
        
        assert!(class_filter.enabled, "ClassName should be enabled");
        assert_eq!(class_filter.operator, Operator::StartsWith, "Should use starts-with");
        assert!(class_filter.value.contains("temp-dialogue"), "Prefix should contain stable part");
    }
    
    #[test]
    fn test_optimization_with_automation_id() {
        let optimizer = XPathOptimizer::new();
        
        // 叶子节点有 AutomationId
        let hierarchy = vec![
            HierarchyNode::new("Pane", "", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Group", "leafId", "dynamic-class active", "发送", 0, ElementRect::default(), 0),
        ];
        
        let result = optimizer.optimize(&hierarchy);
        
        // 叶子节点有 AutomationId → 只用 AutomationId，禁用其他属性
        let leaf_opt = &result.hierarchy[1];
        
        // AutomationId 应启用
        let auto_filter = leaf_opt.filters.iter()
            .find(|f| f.name == "AutomationId")
            .unwrap();
        assert!(auto_filter.enabled);
        
        // ClassName 应禁用（因为有 AutomationId）
        let class_filter = leaf_opt.filters.iter()
            .find(|f| f.name == "ClassName")
            .unwrap();
        assert!(!class_filter.enabled);
        
        // Name 应禁用（因为有 AutomationId）
        let name_filter = leaf_opt.filters.iter()
            .find(|f| f.name == "Name")
            .unwrap();
        assert!(!name_filter.enabled);
    }
    
    #[test]
    fn test_optimization_summary() {
        let optimizer = XPathOptimizer::new();
        
        let hierarchy = vec![
            HierarchyNode::new("Pane", "chatPanel", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Group", "", "menu-item active selected", "", 0, ElementRect::default(), 0),
        ];
        
        let result = optimizer.optimize(&hierarchy);
        
        // 应有锚点
        assert!(result.summary.used_anchor);
        assert!(result.summary.anchor_description.is_some());
        
        // 应有简化的属性
        assert!(result.summary.simplified_attrs > 0 || result.summary.removed_dynamic_attrs > 0);
    }
    
    // ── 元宝聊天窗口实际案例测试 ──────────────────────────────────────────────────────
    
    #[test]
    fn test_yuanbao_real_xpath_optimization() {
        let optimizer = XPathOptimizer::new();
        
        // 模拟元宝聊天窗口捕获的层级结构
        // 关键问题节点：最后一个 Group 的 ClassName 含动态状态词和随机哈希
        let hierarchy = vec![
            // Chrome 稳定层（Chrome_WidgetWin_1 是稳定框架类名）
            HierarchyNode::new("Pane", "", "Chrome_WidgetWin_1", "元宝 - 轻松工作 多点生活 | Chat", 0, ElementRect::default(), 0),
            HierarchyNode::new("Pane", "", "BrowserRootView", "元宝 - 轻松工作 多点生活 | Chat - Web 内容", 0, ElementRect::default(), 0),
            HierarchyNode::new("Pane", "", "NonClientView", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Pane", "", "EmbeddedBrowserFrameView", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Pane", "", "BrowserView", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Pane", "", "SidebarContentsSplitView", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Pane", "", "View", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Pane", "", "MultiContentsView", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Pane", "", "View", "", 0, ElementRect::default(), 0),
            // Document 有 AutomationId
            HierarchyNode::new("Document", "RootWebArea", "", "元宝 - 轻松工作 多点生活 | Chat", 0, ElementRect::default(), 0),
            HierarchyNode::new("Group", "", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Group", "", "chat_mainPage__wilLn mainPageCtrl chat_mainPageWin__yRJfh", "", 0, ElementRect::default(), 0),
            // 问题节点：含随机哈希 ___BOp4 和动态状态 t-popup-open
            HierarchyNode::new(
                "Group", "", 
                "temp-dialogue-btn_temp-dialogue-btn___BOp4 winFolder_options__ZJ07f t-popup-open",
                "新建对话", 
                1, 
                ElementRect::default(), 
                0
            ),
        ];
        
        let result = optimizer.optimize(&hierarchy);
        
        // 打印优化结果
        println!("\n=== 元宝窗口优化测试 ===");
        println!("移除动态属性: {}", result.summary.removed_dynamic_attrs);
        println!("简化属性: {}", result.summary.simplified_attrs);
        println!("使用锚点: {}", result.summary.used_anchor);
        if result.summary.used_anchor {
            println!("锚点描述: {}", result.summary.anchor_description.unwrap_or_default());
        }
        
        // 验证优化效果
        // 1. 应检测到锚点（Document 有 AutomationId 'RootWebArea'）
        assert!(result.summary.used_anchor, "应检测到 Document[@AutomationId='RootWebArea'] 作为锚点");
        
        // 2. 应有动态属性被移除或简化
        assert!(result.summary.removed_dynamic_attrs > 0 || result.summary.simplified_attrs > 0,
            "应有属性被优化");
        
        // 3. 检查叶子节点的 ClassName 处理
        let leaf_node = &result.hierarchy.last().unwrap();
        let class_filter = leaf_node.filters.iter()
            .find(|f| f.name == "ClassName")
            .unwrap();
        
        println!("叶子节点 ClassName 状态: enabled={}, operator={}, value='{}'",
            class_filter.enabled, class_filter.operator.label(), class_filter.value);
        
        // ClassName 应被简化为 starts-with 前缀（因为含动态特征）
        if class_filter.enabled {
            // 若启用，应使用 starts-with（不能用精确匹配）
            assert_ne!(class_filter.operator, Operator::Equals,
                "动态ClassName不应使用精确匹配");
            // 前缀应不含哈希和状态词
            assert!(!class_filter.value.contains("___"), "前缀不应含哈希分隔符");
            assert!(!class_filter.value.contains("t-popup"), "前缀不应含动态状态词");
        }
        
        // 4. 生成优化后的 XPath 片段
        println!("\n优化后 XPath 片段:");
        for (i, node) in result.hierarchy.iter().enumerate() {
            let segment = node.xpath_segment();
            println!("  [{}] {}", i, segment);
        }
    }
}