# XPath智能优化器设计文档

## 1. 问题背景

### 1.1 当前问题

1. **验证失败**：生成的 XPath 自己校验不通过
   - 原因：ClassName 包含动态状态值（如 `t-popup-open`），验证时状态已变化

2. **属性冗余**：每个节点默认启用 5 个属性过滤器
   - ControlType、AutomationId、ClassName、Name、Index 全部使用"等于"精确匹配
   - 导致 XPath 过长、可读性差

3. **目标**：只要能工作、性能可接受，越简单越易读越好

### 1.2 示例问题

```
原始 XPath（验证失败）：
/Group[@ControlType='Group' and @ClassName='temp-dialogue-btn___BOp4 winFolder_options__ZJ07f t-popup-open' and @FrameworkId='Chrome']

问题分析：
1. ClassName 含随机哈希（___BOp4、__ZJ07f）→ 不稳定
2. ClassName 含状态词（t-popup-open）→ 验证时可能已变化
3. 所有属性都用"等于"精确匹配 → 过严
```

---

## 2. 设计目标

### 2.1 核心目标

| 目标 | 优先级 | 说明 |
|------|--------|------|
| 可验证性 | P0 | 生成的 XPath 必须能成功验证 |
| 稳定性 | P0 | XPath 不因 UI 状态变化而失效 |
| 简洁性 | P1 | 最少属性，最短路径 |
| 可读性 | P1 | 使用语义清晰的匹配方式 |
| 性能 | P2 | 验证耗时 < 1s |

### 2.2 优化效果预期

```
优化前（14个属性条件，验证失败）：
/Group[@ControlType='Group' and @ClassName='temp-dialogue-btn___BOp4 winFolder_options__ZJ07f t-popup-open' and @FrameworkId='Chrome']

优化后（1-2个属性条件，验证成功）：
/Group[starts-with(@ClassName, 'temp-dialogue-btn')]

或使用锚点：
//Group[@AutomationId='chatPanel']//Button[@Name='发送']
```

---

## 3. 优化策略详解

### 3.1 属性稳定性分级

#### 3.1.1 AutomationId（最稳定）

- **策略**：优先保留，禁用其他属性
- **原因**：唯一标识，不随状态变化

```
节点：Button[@AutomationId='submitBtn' and @Name='提交' and @ClassName='btn-active']
优化后：Button[@AutomationId='submitBtn']
```

#### 3.1.2 Name（条件稳定）

- **策略**：分析语义，选择最佳匹配方式
- **匹配方式优先级**：

| 方式 | XPath | 适用场景 | 可读性 |
|------|-------|----------|--------|
| 精确 | `@Name='发送消息'` | 值稳定、短 | ★★★ |
| 前缀 | `starts-with(@Name, '发送')` | 前缀语义明确 | ★★★★ |
| 后缀 | `ends-with(@Name, '按钮')` | 后缀是类型标识 | ★★★ |
| 包含 | `contains(@Name, '发送')` | 最后选择 | ★★ |

- **UI类型结尾词库**：

```
中文：按钮、输入框、文本框、列表、面板、菜单、链接、图标、提示
英文：Button, Btn, Link, Input, Edit, Panel, List, Menu, Icon, Tip
```

#### 3.1.3 ClassName（需分析）

**动态特征检测**：

| 特征 | 示例 | 策略 |
|------|------|------|
| 状态词 | `active`, `hover`, `open`, `selected` | 禁用或提取前缀 |
| 随机哈希 | `___BOp4`, `_ZJ07f` | 使用 `starts-with` 前缀 |
| 过长(>50字符) | CSS Modules 输出 | 使用 `starts-with` 前缀 |

**稳定特征检测**：

| 框架 | 特征类名 | 策略 |
|------|----------|------|
| Chrome/Electron | `Chrome_WidgetWin_*` | 完整保留 |
| Qt | `QWidget`, `QStackedWidget` | 完整保留 |
| Win32 | 标准 Windows 类名 | 完整保留 |

**处理策略输出**：

```rust
enum ClassStrategy {
    Keep,                      // 完整保留：@ClassName='xxx'
    Prefix(String),            // 前缀匹配：starts-with(@ClassName, 'xxx')
    Suffix(String),            // 后缀匹配：ends-with(@ClassName, 'xxx')
    Disable,                   // 禁用
}
```

#### 3.1.4 ControlType（必需）

- **策略**：始终保留
- **原因**：定位基础，必须指定元素类型

#### 3.1.5 Index/position（最后手段）

- **策略**：当其他属性不足时使用
- **优化**：
  - 位置 1 → `first()`
  - 最后位置 → `last()`
  - 中间位置 → `position()=N`

### 3.2 锚点间接定位

#### 3.2.1 锚点概念

当目标元素本身难以稳定定位时，通过稳定的"锚点"元素间接定位。

#### 3.2.2 锚点类型

| 类型 | XPath 语法 | 适用场景 |
|------|-----------|----------|
| 父容器锚点 | `//Parent//Target` | 目标在稳定容器内 |
| 兄弟锚点 | `//Anchor/following-sibling::Target` | 目标在稳定元素后 |
| 前兄弟锚点 | `//Anchor/preceding-sibling::Target` | 目标在稳定元素前 |

#### 3.2.3 锚点选择优先级

```
1. 有 AutomationId 的元素         → 最佳锚点（Strong）
2. 有稳定 ClassName 的父容器      → 良好锚点（Medium）
3. 有稳定 Name 的兄弟元素         → 可用锚点（Weak）
4. 无锚点                         → 使用传统路径
```

#### 3.2.4 锚点定位示例

**场景：目标无标识，但父容器有 AutomationId**

```
原始层级：
Group[@AutomationId='chatPanel']    ← 稳定锚点
  └─ Pane（空）
      └─ Pane（空）
          └─ Button[@Name='发送']    ← 目标

传统路径：
/Group[@AutomationId='chatPanel']/Pane/Pane/Button[@Name='发送']

锚点简化：
//Group[@AutomationId='chatPanel']//Button[@Name='发送']
```

---

## 4. 实现设计

### 4.1 文件结构

```
src/core/
├── model.rs           # 已有，添加枚举类型
├── xpath.rs           # 已有，添加优化函数
└── xpath_optimizer.rs # 新增，核心优化逻辑
```

### 4.2 核心数据结构

```rust
// src/core/xpath_optimizer.rs

/// ClassName 处理策略
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
pub enum AnchorStrength {
    Strong,   // AutomationId 存在
    Medium,   // ClassName 稳定
    Weak,     // Name 稳定
}

/// 单节点优化配置
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

/// 优化结果
pub struct OptimizationResult {
    /// 优化后的 hierarchy（filters 已修改）
    pub hierarchy: Vec<HierarchyNode>,
    /// 锚点索引（如果有）
    pub anchor_index: Option<usize>,
    /// 优化摘要（用于 UI 显示）
    pub summary: OptimizationSummary,
}

/// 优化摘要
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
```

### 4.3 核心算法

```rust
pub struct XPathOptimizer {
    /// 动态状态词库
    dynamic_keywords: Vec<String>,
    /// 稳定框架类名模式
    stable_class_patterns: Vec<String>,
    /// UI 类型结尾词
    ui_type_suffixes: Vec<String>,
}

impl XPathOptimizer {
    /// 创建默认配置的优化器
    pub fn new() -> Self {
        Self {
            dynamic_keywords: vec![
                "active", "hover", "focus", "pressed", "selected",
                "open", "closed", "expanded", "collapsed",
                "visible", "hidden", "loading", "animating",
            ],
            stable_class_patterns: vec![
                "Chrome_WidgetWin_", "QWidget", "QStackedWidget",
                "mmui::", "Qt5QWindow", "WindowClass",
            ],
            ui_type_suffixes: vec![
                "按钮", "Button", "Btn", "链接", "Link",
                "输入框", "文本框", "Input", "Edit",
                "面板", "列表", "Panel", "List", "Menu",
            ],
        }
    }
    
    /// 执行完整优化
    pub fn optimize(&self, hierarchy: &[HierarchyNode]) -> OptimizationResult {
        // Phase 1: 锚点识别
        let anchor = self.find_anchor(hierarchy);
        
        // Phase 2: 节点级优化
        let optimizations = self.optimize_nodes(hierarchy);
        
        // Phase 3: 应用优化到 hierarchy
        let optimized_hierarchy = self.apply_optimizations(hierarchy, &optimizations, &anchor);
        
        // Phase 4: 生成摘要
        let summary = self.generate_summary(&optimizations, &anchor);
        
        OptimizationResult {
            hierarchy: optimized_hierarchy,
            anchor_index: anchor.map(|(idx, _)| idx),
            summary,
        }
    }
    
    /// Phase 1: 锚点识别
    fn find_anchor(&self, hierarchy: &[HierarchyNode]) -> Option<(usize, AnchorStrength)> {
        // 从叶子向上搜索第一个稳定节点
        for i in (0..hierarchy.len()).rev() {
            let node = &hierarchy[i];
            
            // Strong: 有 AutomationId
            if !node.automation_id.is_empty() {
                return Some((i, AnchorStrength::Strong));
            }
            
            // Medium: 有稳定 ClassName
            if self.is_stable_class(&node.class_name) {
                return Some((i, AnchorStrength::Medium));
            }
            
            // Weak: 有稳定 Name
            if !node.name.is_empty() && !self.has_dynamic_features(&node.name) {
                return Some((i, AnchorStrength::Weak));
            }
        }
        None
    }
    
    /// Phase 2: 分析单个节点的 ClassName
    fn classify_classname(&self, class_name: &str) -> ClassStrategy {
        if class_name.is_empty() {
            return ClassStrategy::Disable;
        }
        
        // 检测稳定框架类名 → 完整保留
        if self.is_stable_class(class_name) {
            return ClassStrategy::Keep;
        }
        
        // 检测动态状态词 → 提取前缀
        if self.has_dynamic_state(class_name) {
            let prefix = self.extract_stable_prefix(class_name);
            if !prefix.is_empty() {
                return ClassStrategy::Prefix(prefix);
            }
            return ClassStrategy::Disable;
        }
        
        // 检测随机哈希 → 提取前缀
        if self.has_random_hash(class_name) {
            let prefix = self.extract_stable_prefix(class_name);
            if !prefix.is_empty() {
                return ClassStrategy::Prefix(prefix);
            }
            return ClassStrategy::Disable;
        }
        
        // 长度过长 → 提取前缀
        if class_name.len() > 50 {
            let prefix = self.extract_stable_prefix(class_name);
            if !prefix.is_empty() {
                return ClassStrategy::Prefix(prefix);
            }
        }
        
        // 默认保留
        ClassStrategy::Keep
    }
    
    /// Phase 2: 分析单个节点的 Name
    fn analyze_name(&self, name: &str) -> NameStrategy {
        if name.is_empty() {
            return NameStrategy::Exact; // 空值不匹配
        }
        
        // 检测 UI 类型结尾 → 使用 ends-with
        for suffix in &self.ui_type_suffixes {
            if name.ends_with(suffix) {
                return NameStrategy::Suffix(suffix.clone());
            }
        }
        
        // 检测语义前缀（前 N 字符有意义）→ 使用 starts-with
        // 如果 Name 以固定词开头（如"发送"、"接收"），取前缀
        let prefix_len = self.detect_meaningful_prefix(name);
        if prefix_len > 0 && prefix_len < name.len() {
            return NameStrategy::Prefix(name.chars().take(prefix_len).collect());
        }
        
        // 默认精确匹配
        NameStrategy::Exact
    }
    
    /// 提取 ClassName 的稳定前缀
    fn extract_stable_prefix(&self, class_name: &str) -> String {
        // 分割空格，取第一个不含动态特征的词
        let parts: Vec<&str> = class_name.split_whitespace().collect();
        for part in parts {
            if !self.has_dynamic_state(part) && !self.has_random_hash(part) {
                return part.to_string();
            }
        }
        
        // 或者取到第一个分隔符（___、__、_）之前
        for sep in ["___", "__", "_"] {
            if let Some(pos) = class_name.find(sep) {
                return class_name[..pos].to_string();
            }
        }
        
        // 取前 20 字符作为前缀
        class_name.chars().take(20).collect()
    }
}
```

### 4.4 与现有代码集成

```rust
// src/core/xpath.rs - 添加优化函数

use super::xpath_optimizer::{XPathOptimizer, OptimizationResult};

/// 优化 hierarchy 并重新生成 XPath
pub fn optimize_and_generate(nodes: &[HierarchyNode]) -> OptimizationResult {
    let optimizer = XPathOptimizer::new();
    optimizer.optimize(nodes)
}

/// 应用优化策略到 PropertyFilter
pub fn apply_name_strategy(filter: &mut PropertyFilter, strategy: &NameStrategy) {
    match strategy {
        NameStrategy::Exact => {
            filter.operator = Operator::Equals;
        }
        NameStrategy::Prefix(prefix) => {
            filter.operator = Operator::StartsWith;
            filter.value = prefix.clone();
        }
        NameStrategy::Suffix(suffix) => {
            filter.operator = Operator::EndsWith;
            filter.value = suffix.clone();
        }
        NameStrategy::Contains(keyword) => {
            filter.operator = Operator::Contains;
            filter.value = keyword.clone();
        }
    }
}

pub fn apply_class_strategy(filter: &mut PropertyFilter, strategy: &ClassStrategy) {
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
```

### 4.5 GUI集成

```rust
// src/gui/app.rs - 添加优化按钮和状态

pub struct SelectorApp {
    // ... 现有字段 ...
    
    /// 优化结果摘要
    optimization_summary: Option<OptimizationSummary>,
}

impl SelectorApp {
    /// 执行智能优化
    fn do_optimize(&mut self) {
        // 1. 执行优化
        let optimizer = XPathOptimizer::new();
        let result = optimizer.optimize(&self.hierarchy);
        
        // 2. 更新 hierarchy
        self.hierarchy = result.hierarchy;
        self.optimization_summary = Some(result.summary);
        
        // 3. 重新生成 XPath
        self.custom_xpath = false;
        self.rebuild_xpath();
        
        // 4. 自动验证
        self.do_validate();
        
        // 5. 更新状态消息
        let summary = &result.summary;
        self.status_msg = format!(
            "智能优化完成：移除 {} 个动态属性，简化 {} 个属性{}",
            summary.removed_dynamic_attrs,
            summary.simplified_attrs,
            if summary.used_anchor {
                format!("，使用锚点 {}", summary.anchor_description.unwrap_or_default())
            } else {
                String::new()
            }
        );
    }
    
    /// 绘制优化按钮
    fn draw_optimize_button(&mut self, ui: &mut Ui) {
        if ui.small_button("智能优化").on_hover_text("自动优化XPath，移除动态属性").clicked() {
            self.do_optimize();
        }
        
        // 显示优化摘要
        if let Some(ref summary) = self.optimization_summary {
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "优化：移除 {} / 简化 {} {}",
                    summary.removed_dynamic_attrs,
                    summary.simplified_attrs,
                    if summary.used_anchor { "锚点" } else { "" }
                ))
                .color(C_OK)
                .size(10.0)
            );
        }
    }
}
```

---

## 5. 实现迭代计划

### Phase 1：核心优化逻辑（MVP）

**目标**：实现基本的 ClassName 动态检测和简化

**文件**：
- `src/core/xpath_optimizer.rs`（新建）

**功能**：
1. `classify_classname()` - ClassName 分析
2. `extract_stable_prefix()` - 前缀提取
3. `optimize()` - 单节点优化

**验证**：
- 单元测试：动态 ClassName 识别
- 单元测试：前缀提取正确性

---

### Phase 2：Name 策略与锚点定位

**目标**：实现 Name 可读性优化和锚点间接定位

**文件**：
- `src/core/xpath_optimizer.rs`（扩展）

**功能**：
1. `analyze_name()` - Name 策略分析
2. `find_anchor()` - 锚点识别
3. `apply_optimizations()` - 应用到 hierarchy

**验证**：
- 单元测试：Name 策略选择
- 单元测试：锚点识别逻辑
- 集成测试：完整优化流程

---

### Phase 3：GUI集成与验证反馈

**目标**：GUI 添加优化按钮，自动验证反馈

**文件**：
- `src/gui/app.rs`（修改）
- `src/core/xpath.rs`（扩展）

**功能**：
1. 添加"智能优化"按钮
2. 优化摘要显示
3. 自动验证 + 回退机制

**验证**：
- 手动测试：捕获元素 → 智能优化 → 验证通过
- 手动测试：优化失败时的回退

---

### Phase 4：边界场景与调优

**目标**：处理边界场景，调优词库

**文件**：
- `src/core/xpath_optimizer.rs`（完善）

**功能**：
1. 扩展动态状态词库
2. 扩展稳定类名库
3. 处理全空节点场景
4. 性能优化（验证耗时 < 1s）

**验证**：
- 真实场景测试：微信、Chrome、Qt应用
- 性能测试：复杂层级优化耗时

---

## 6. 测试计划

### 6.1 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_dynamic_classname_detection() {
        let optimizer = XPathOptimizer::new();
        
        // 动态状态词
        assert!(optimizer.has_dynamic_state("menu-item active"));
        assert!(optimizer.has_dynamic_state("btn hover selected"));
        
        // 稳定类名
        assert!(!optimizer.has_dynamic_state("QWidget"));
        assert!(!optimizer.has_dynamic_state("Chrome_WidgetWin_1"));
    }
    
    #[test]
    fn test_random_hash_detection() {
        let optimizer = XPathOptimizer::new();
        
        assert!(optimizer.has_random_hash("temp-btn___BOp4"));
        assert!(optimizer.has_random_hash("options__ZJ07f"));
        assert!(!optimizer.has_random_hash("QWidget"));
    }
    
    #[test]
    fn test_prefix_extraction() {
        let optimizer = XPathOptimizer::new();
        
        assert_eq!(
            optimizer.extract_stable_prefix("temp-dialogue-btn___BOp4"),
            "temp-dialogue-btn"
        );
        
        assert_eq!(
            optimizer.extract_stable_prefix("menu-item active selected"),
            "menu-item"
        );
    }
    
    #[test]
    fn test_name_strategy() {
        let optimizer = XPathOptimizer::new();
        
        // UI类型结尾
        let strategy = optimizer.analyze_name("发送消息按钮");
        assert!(matches!(strategy, NameStrategy::Suffix(_)));
        
        // 无特殊模式
        let strategy = optimizer.analyze_name("提交");
        assert!(matches!(strategy, NameStrategy::Exact));
    }
    
    #[test]
    fn test_anchor_detection() {
        let optimizer = XPathOptimizer::new();
        let hierarchy = vec![
            HierarchyNode::new("Group", "", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Pane", "stableId", "", "", 0, ElementRect::default(), 0),
            HierarchyNode::new("Button", "", "", "", 0, ElementRect::default(), 0),
        ];
        
        let anchor = optimizer.find_anchor(&hierarchy);
        assert_eq!(anchor.map(|(idx, _)| idx), Some(1)); // Pane with AutomationId
    }
    
    #[test]
    fn test_full_optimization() {
        let optimizer = XPathOptimizer::new();
        
        // 模拟问题节点
        let mut node = HierarchyNode::new(
            "Group", "", 
            "temp-dialogue-btn___BOp4 winFolder_options__ZJ07f t-popup-open",
            "", 0, ElementRect::default(), 0
        );
        
        let result = optimizer.optimize_nodes(&[node.clone()]);
        
        // ClassName 应被简化为前缀
        let class_strategy = &result[0].class_strategy;
        assert!(matches!(class_strategy, ClassStrategy::Prefix(_)));
    }
}
```

### 6.2 集成测试场景

| 场景 | 输入 | 预期输出 |
|------|------|----------|
| 动态ClassName | `temp-btn___BOp4 active` | `starts-with(@ClassName, 'temp-btn')` |
| 有AutomationId | `AutomationId='submitBtn'` | 只用 AutomationId |
| Name含类型结尾 | `发送消息按钮` | `ends-with(@Name, '按钮')` |
| 父容器锚点 | 父有ID，子无 | `//Parent//Target` |
| 全空节点 | 无任何属性 | `ControlType[position()=N]` |

---

## 7. 风险与应对

| 风险 | 影响 | 应对 |
|------|------|------|
| 前缀提取不准确 | 优化后仍验证失败 | 回退机制：逐步增加属性 |
| 锚点选择错误 | 定位到错误元素 | 验证唯一性：count > 1 时增加约束 |
| 词库不全 | 漏判动态特征 | 可配置词库，用户可扩展 |
| 性能问题 | 优化耗时过长 | 限制优化范围，最多分析前 10 层 |

---

## 8. 未来扩展

1. **可配置词库**：用户可添加应用特定的动态词/稳定词
2. **学习模式**：根据验证失败历史自动调整策略
3. **批量优化**：对历史 XPath 批量重新优化
4. **可视化反馈**：在树上标记优化前后对比

---

## 附录：参考案例

### A. 元宝聊天窗口（实际捕获）

```
原始 XPath（验证失败）：
Window[@ClassName='Tauri Window' and @Name='元宝' and @ProcessName='yuanbao'],
//Pane[@ClassName='Chrome_WidgetWin_1' ...]/.../Group[@ClassName='temp-dialogue-btn___BOp4 winFolder_options__ZJ07f t-popup-open']

优化预期：
Window[@ProcessName='yuanbao'],
//Group[starts-with(@ClassName, 'temp-dialogue-btn')]
```

### B. 微信窗口

```
原始 XPath：
Window[@ClassName='mmui::MainWindow' and @Name='微信'],
//Group[@ClassName='mmui::MainView']//Button[@ClassName='mmui::XTabBarItem' and @Name='搜一搜']

优化预期：
Window[@ClassName='mmui::MainWindow'],
//Button[@Name='搜一搜']  // 直接跳到有 Name 的目标
```

---

*文档版本：v1.0*
*创建日期：2026-04-28*