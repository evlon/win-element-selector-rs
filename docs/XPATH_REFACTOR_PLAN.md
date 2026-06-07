# XPath 规则匹配与解析整改计划

## 版本历史

| 版本 | 日期 | 作者 | 变更说明 |
|------|------|------|----------|
| 1.0 | 2026-06-05 | System | 初始版本 |
| 1.1 | 2026-06-05 | System | 新增 §4 函数命名语义化方案 + §5 单元测试规划；更新 §6 工时估算 |

---

## 1. 需求文档评估

基于 `需求规格-定位.md` v1.3，核心 XPath 需求：

### 1.1 XPath 语法定义（需求文档 §5.2）

| 表达式 | 含义 | 搜索范围 | 实现策略 |
|--------|------|----------|----------|
| `/A` | 直接子元素 | Children | `FindFirst(TreeScope.Children, Condition)` |
| `//A` | 所有后代 | Descendants | `FindFirst(TreeScope.Descendants, Condition)` |
| `/*/A` | 孙子元素（深度2） | BFS 深度 2 | 手动递归 Children |
| `/*2/A` | 曾孙元素（深度3） | BFS 深度 3 | 手动递归 Children |
| `/*n/A` (n>2) | 深度 n+1 | BFS 深度 n+1 | 手动递归，需超时保护 |

### 1.2 核心原则（需求文档 §2）

| 原则 | 当前代码现状 | 差距 |
|------|-------------|------|
| 速度优先 | ✅ 已实现 Chain FindFirst/FindAll | 无 |
| 唯一性仅对 findOne 叶子强制 | ⚠️ 有 `OnlyOne` 模式但仅做 count 检查，未在父节点下验证 | **部分实现** |
| 无自动 fallback | ❌ 存在大量隐式 fallback（Strategy 1→2→2.5→2.7→3→4） | **严重偏离** |
| 利用 UIA 原生能力 | ✅ 已使用 FindAll(Subtree)、FindFirst | 无 |
| 性能可预测 | ⚠️ 有 perf 日志但无预估值 | 待完善 |

### 1.3 三种定位语义（需求文档 §5.3）

| API | 当前实现 | 差距 |
|-----|---------|------|
| `findFirst` | ✅ `SearchMode::First`（默认） | 无 |
| `findOne` | ⚠️ `SearchMode::OnlyOne` 只做全局 count | **需在父节点下验证唯一性** |
| `findAll` | ✅ `SearchMode::All` | **缺少 FilterCondition 支持** |

### 1.4 `/*n/` 深度限制语法（需求文档 §5.2、§12.2-12.3）

**当前状态：完全未实现！**

现有代码中：
- `find.rs` 中的 `parse_xpath_step` 只解析 `[@Attr='Value']` 谓词，**不解析 `/*n/` 前缀**
- `find_control.rs` 和 `find_raw.rs` 的 descendant 搜索全部使用 `FindAll(Subtree)` 或 Chain 方法
- **没有任何代码处理 `/*n/` 语法**

---

## 2. 现有代码 XPath 相关逻辑的问题分析

### 2.1 架构层面问题

#### P0：违反"无自动 fallback"原则

`find_by_xpath_with_fallback`（`find.rs:39-566`）的核心设计就是 fallback 链：

```
/ XPath: Strategy 1 → 1.5 → 2 → 2.5 → 2.7 → 3 → 4
// XPath: Step 1 → Step 2 → Step 2a → Step 2b
```

需求明确要求：
> 所有定位策略必须在开发阶段确定，生产运行阶段不得有自动 fallback 或隐式策略切换

**根本矛盾**：当前代码用 fallback 链来"猜"最佳搜索策略，而需求要求通过 `LocateMode` 显式指定。

#### P0：XPath 步骤解析不完整

`parse_xpath_step`（`find.rs:728-800`）只处理谓词部分，**不处理步骤前缀语法**：

```rust
// 当前只能解析: Button[@Name='OK']
// 无法解析: /*2/Button[@Name='OK']  （/*n/ 前缀）
// 无法解析: /Button[@Name='OK']    （/ 前缀，虽然实际使用时按 split 处理）
// 无法解析: //Button[@Name='OK']   （// 前缀，虽然 is_descendant 在外层判断）
```

需求文档 §12.3 定义的正则：`^(//?|\*(\d+)?/)([^\[\]]+)(\[.*\])?$`

**缺失的解析**：
- `/*/` → `max_depth = 2`
- `/*2/` → `max_depth = 3`
- `/*n/` → `max_depth = n + 1`

#### P1：XPath 步骤结构体信息不完整

`ParsedXPathStep`（`cache.rs:174-180`）只有 5 个字段，缺少：

```rust
// 缺失的关键字段：
- step_prefix: XPathStepPrefix   // /, //, /*, /*2, /*n
- max_depth: Option<u32>          // /*n/ 的深度限制
- is_absolute: bool               // 是否为绝对路径步骤
```

#### P1：`/*n/` 执行策略完全缺失

需求文档 §5.4 要求：

| Step 语法 | TreeScope | 算法 |
|-----------|-----------|------|
| `/A` | Children | `FindFirst` |
| `//A` | Descendants | `FindFirst` |
| `/*/A` | Children + 递归 | BFS 深度 2 |
| `/*n/A` | Children + 递归 | BFS 深度 n+1 |

现有代码中 `walk_control_tree_steps` 和 `walk_raw_tree_steps` 只支持 Children 遍历（单层），不支持 BFS 深度限制。

#### P2：`findOne` 唯一性验证不完整

需求文档 §5.3.1 要求：
> 叶子节点：定位后必须验证同一父节点下不存在另一个满足相同条件的元素

当前 `SearchMode::OnlyOne` 在 `apply_search_mode` 中只做全局 count 检查，**未在父节点下验证**。

#### P2：`findAll` 缺少 FilterCondition

需求文档 §5.3 和 §8.4 定义了 `FilterCondition`（Eq/NotEq/Contains/Regex/Exists），当前代码中的 `FindAllFilter` 只处理 offscreen/zero-size/out-of-bounds，不支持属性过滤。

### 2.2 代码质量问题

#### Q1：XPath 前缀与后缀混合

当前 XPath 字符串承载了过多元信息：

```
[fast-child @ClassName='Chrome_WidgetWin_0']/Document/Text:first
 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^  ^^^^^^^^^^^^^^^^ ^^^^^
         定位模式+子窗口提示                    实际 XPath        搜索模式
```

建议拆分为结构化数据，XPath 字符串只保留纯 XPath 语法。

#### Q2：正则表达式每次编译

`parse_xpath_step`（`find.rs:754-778`）中 4 个正则每次调用都 `Regex::new(...).unwrap()`，应使用 `lazy_static` 或 `once_cell::sync::Lazy`。

#### Q3：`apply_search_mode_ui` 和 `apply_search_mode` 重复

两个函数逻辑完全相同，仅参数类型不同（`Vec<UIElement>` vs `Vec<ElementData>`），应使用泛型。

#### Q4：`get_uia_property_for_xpath` 使用字符串 key

应使用枚举代替字符串匹配，避免拼写错误和提高性能。

#### Q5：`build_uia_condition_from_step` 属性映射硬编码

只支持 5 个属性（Name/AutomationId/FrameworkId/ClassName/ControlType），但需求文档定义了更丰富的属性集。

#### Q6：SegmentValidationResult 构造逻辑重复

在 `find.rs`、`find_control.rs`、`find_raw.rs` 中至少有 6 处相同的 `SegmentValidationResult` 构造代码。

---

## 3. 整改步骤

### 阶段 1：XPath 步骤解析重构（P0）

#### 步骤 1.1：定义 XPath 步骤前缀枚举

```rust
/// XPath 步骤前缀类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XPathStepPrefix {
    /// `/A` — 直接子元素
    Child,
    /// `//A` — 所有后代（无限深度）
    Descendant,
    /// `/*/A` — 深度限制为 2（孙子）
    DepthLimited { max_depth: u32 },
}
```

#### 步骤 1.2：扩展 ParsedXPathStep

```rust
pub(super) struct ParsedXPathStep {
    /// 步骤前缀类型
    pub prefix: XPathStepPrefix,
    /// ControlType 名称（如 "Button", "Text"）
    pub type_name: Option<String>,
    /// 精确匹配属性
    pub required_props: Vec<(String, String)>,
    /// starts-with 谓词
    pub require_starts_with: Vec<(String, String)>,
    /// contains 谓词
    pub require_contains: Vec<(String, String)>,
    /// 正则匹配谓词
    pub require_matches: Vec<(String, Regex)>,
    /// 是否为 or/not 复杂谓词（无法用简单属性表达）
    pub is_complex: bool,
}
```

#### 步骤 1.3：实现完整的步骤解析

基于需求文档 §12.3 的正则 `^(//?|\*(\d+)?/)([^\[\]]+)(\[.*\])?$`：

```rust
fn parse_xpath_step_full(step: &str) -> ParsedXPathStep {
    // 1. 提取前缀: /, //, /*, /*2, /*n
    // 2. 提取类型名: Button, Text, *
    // 3. 提取谓词: [@Name='OK' and @AutomationId='btn1']
    // 4. 解析谓词中的 starts-with/contains/matches/or/not
}
```

#### 步骤 1.4：预编译正则表达式

```rust
use once_cell::sync::Lazy;

static ATTR_EQ_RE: Lazy<Regex> = Lazy::new(|| 
    Regex::new(r#"@(\w+)\s*=\s*'([^']*)'"#).unwrap()
);
static STARTS_WITH_RE: Lazy<Regex> = Lazy::new(|| 
    Regex::new(r#"@(\w+)\s*=\s*starts-with\(\s*'([^']*)'\s*\)"#).unwrap()
);
static CONTAINS_RE: Lazy<Regex> = Lazy::new(|| 
    Regex::new(r#"@(\w+)\s*=\s*contains\(\s*'([^']*)'\s*\)"#).unwrap()
);
static MATCHES_RE: Lazy<Regex> = Lazy::new(|| 
    Regex::new(r#"@(\w+)\s*=\s*matches\(\s*'([^']*)'\s*(?:,\s*'([^']*)'\s*)?\)"#).unwrap()
);
static STEP_PARSE_RE: Lazy<Regex> = Lazy::new(|| 
    Regex::new(r"^(//?|\*(\d+)?/)([^\[\]]+)(\[.*\])?$").unwrap()
);
```

---

### 阶段 2：执行策略重构（P0）

#### 步骤 2.1：定义 XPath 执行策略

将需求文档 §5.4 的执行策略表编码为代码：

```rust
/// XPath 步骤的执行策略
#[derive(Debug, Clone)]
pub enum StepExecutionStrategy {
    /// 直接子元素查找：`FindFirst(TreeScope::Children, condition)`
    DirectChild,
    /// 后代查找：`FindFirst(TreeScope::Descendants, condition)`
    Descendant,
    /// 深度限制 BFS：逐层 Children 遍历，限制深度
    DepthLimitedBfs { max_depth: u32 },
}
```

#### 步骤 2.2：实现深度限制 BFS

基于需求文档 §12.2 的伪代码实现：

```rust
fn find_with_depth_limit(
    auto: &UIAutomation,
    root: &UIElement,
    target_step: &ParsedXPathStep,
    max_depth: u32,
    walker: &WalkerType,
) -> Vec<UIElement> {
    let mut results = vec![];
    let mut queue = VecDeque::from(vec![(root.clone(), 0)]);
    
    while let Some((node, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        
        // 获取当前节点的所有子元素
        let children = match walker {
            WalkerType::ControlView(w) => get_children(w, &node),
            WalkerType::RawView(w) => get_children(w, &node),
        };
        
        for child in children {
            if depth + 1 == max_depth {
                // 到达目标深度，检查是否匹配
                if element_matches_parsed_step(&child, target_step) {
                    results.push(child);
                }
            } else {
                // 未到达目标深度，继续递归
                queue.push_back((child, depth + 1));
            }
        }
    }
    
    results
}
```

#### 步骤 2.3：移除隐式 fallback

`find_by_xpath_with_fallback` 需要重构为：

```rust
fn execute_xpath(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
    locate_mode: LocateMode,  // 显式指定，不允许自动推断
) -> Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    // 1. 解析 XPath 为步骤列表（含前缀信息）
    // 2. 根据 LocateMode 选择 Walker（ControlView 或 RawView）
    // 3. 逐步骤执行，根据每个步骤的 XPathStepPrefix 选择策略
    // 4. 不进行任何 fallback — 失败即返回错误
}
```

**保留但隔离** fallback 逻辑：将现有的多策略 fallback 代码移到 `find_by_xpath_with_fallback` 函数中，标记为 `#[deprecated]`，仅在校验（开发阶段）使用。

---

### 阶段 3：定位语义完善（P1-P2）

#### 步骤 3.1：实现 `findOne` 叶子唯一性验证

基于需求文档 §12.1：

```rust
fn find_one_leaf(
    parent: &UIElement,
    leaf_step: &ParsedXPathStep,
    walker: &WalkerType,
) -> Result<UIElement, LocateError> {
    let condition = build_condition(leaf_step);
    let scope = match leaf_step.prefix {
        XPathStepPrefix::Child => TreeScope::Children,
        XPathStepPrefix::Descendant | XPathStepPrefix::DepthLimited { .. } => TreeScope::Descendants,
    };
    
    // 最多取 2 个，验证唯一性
    let matches = parent.find_all(scope, &condition, 2);
    match matches.len() {
        0 => Err(LocateError::StepNotFound(last_step_index)),
        1 => Ok(matches[0].clone()),
        _ => Err(LocateError::LeafNotUnique),
    }
}
```

#### 步骤 3.2：为 `findAll` 增加 FilterCondition

基于需求文档 §8.4，增加客户端过滤：

```rust
pub struct FilterCondition {
    pub property: UiaProperty,   // 使用枚举替代字符串
    pub operator: FilterOp,
    pub value: String,
}

pub enum FilterOp {
    Eq,        // UIA 原生条件
    NotEq,     // 客户端过滤
    Contains,  // 客户端过滤
    Regex,     // 客户端过滤
    Exists,    // 客户端过滤
}

fn apply_filter_condition(
    elements: Vec<UIElement>,
    filter: &FilterCondition,
) -> Vec<UIElement> {
    match filter.operator {
        FilterOp::Eq => elements, // 已在 UIA 条件中处理
        _ => elements.into_iter()
            .filter(|elem| element_matches_filter(elem, filter))
            .collect(),
    }
}
```

---

### 阶段 4：XPath 字符串与结构化数据分离（P1）

#### 步骤 4.1：定义结构化的 XPath 表示

```rust
/// 结构化的 XPath 表示（替代字符串拼接）
#[derive(Debug, Clone)]
pub struct StructuredXPath {
    /// 定位模式（Fast/FastChild/Full/FullChild）
    pub locate_mode: LocateMode,
    /// 子窗口提示（Child 模式时使用）
    pub child_hwnd_hint: Option<ChildHwndHint>,
    /// XPath 步骤列表
    pub steps: Vec<XPathStep>,
    /// 搜索模式（First/OnlyOne/All）
    pub search_mode: SearchMode,
}

#[derive(Debug, Clone)]
pub struct XPathStep {
    pub prefix: XPathStepPrefix,
    pub type_name: Option<String>,
    pub predicates: Vec<XPathPredicate>,
}

#[derive(Debug, Clone)]
pub enum XPathPredicate {
    Eq { attr: String, value: String },
    NotEq { attr: String, value: String },
    StartsWith { attr: String, value: String },
    Contains { attr: String, value: String },
    Matches { attr: String, pattern: String },
    Position(i32),
    Or(Vec<Vec<XPathPredicate>>),
    Not(Box<XPathPredicate>),
}
```

#### 步骤 4.2：实现序列化/反序列化

```rust
impl StructuredXPath {
    /// 从字符串解析
    pub fn parse(xpath: &str) -> Result<Self, XPathParseError> { ... }
    
    /// 序列化为字符串（用于显示和存储）
    pub fn to_string(&self) -> String { ... }
    
    /// 序列化为纯 XPath（不含元信息）
    pub fn to_pure_xpath(&self) -> String { ... }
}
```

---

### 阶段 5：代码结构优化

#### 步骤 5.1：统一 XPath 模块

将分散的 XPath 解析逻辑集中到 `src/core/xpath/` 目录：

```
src/core/xpath/
├── mod.rs           // 模块入口，公开 API
├── parser.rs        // XPath 字符串解析（步骤解析、前缀解析）
├── model.rs         // 数据结构定义（StructuredXPath, XPathStep, XPathPredicate）
├── executor.rs      // XPath 执行引擎（根据步骤前缀选择策略）
├── condition.rs     // UIA 条件构建（build_uia_condition_from_step）
├── matcher.rs       // 元素匹配（element_matches_parsed_step）
└── strategy.rs      // 执行策略定义和选择
```

#### 步骤 5.2：消除重复代码

| 重复项 | 解决方案 |
|--------|----------|
| `apply_search_mode` × 2 | 泛型函数 |
| `SegmentValidationResult` 构造 × 6+ | 提取 `new_segment_result()` 工厂函数 |
| `walk_control_tree_steps` / `walk_raw_tree_steps` | 提取公共逻辑，使用泛型 Walker |
| `findall_chain_first` / `findall_chain_all` | 提取公共 Chain 逻辑 |
| `find_by_xpath_control_descendants_manual` / `find_by_xpath_raw_descendants_manual` | 提取公共 BFS 框架 |

#### 步骤 5.3：使用枚举替代字符串 key

```rust
// 替代 get_uia_property_for_xpath 中的字符串 key
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiaAttribute {
    Name,
    AutomationId,
    ClassName,
    FrameworkId,
    ControlType,
    HelpText,
    AcceleratorKey,
    AccessKey,
    ItemType,
    ItemStatus,
    IsEnabled,
    IsPassword,
    IsOffscreen,
}

impl UiaAttribute {
    pub fn get_value(&self, elem: &UIElement) -> String {
        match self {
            UiaAttribute::Name => elem.get_name().unwrap_or_default(),
            UiaAttribute::AutomationId => elem.get_automation_id().unwrap_or_default(),
            // ...
        }
    }
}
```

#### 步骤 5.4：`find_by_xpath_with_fallback` 重构

重构为清晰的策略模式：

```rust
pub(super) fn find_by_xpath(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
    mode: LocateMode,
    filter: &FindAllFilter,
) -> Result<(Vec<UIElement>, Vec<SegmentValidationResult>)> {
    // 1. 解析 XPath
    let structured = StructuredXPath::parse(xpath)?;
    
    // 2. 根据 LocateMode 选择 Walker
    let walker = match mode.walker_hint() {
        WalkerHint::ControlView => auto.get_control_view_walker()?,
        WalkerHint::RawView => auto.get_raw_view_walker()?,
        WalkerHint::ChildHwnd => return execute_child_hwnd_search(...),
        WalkerHint::Unknown => auto.get_control_view_walker()?, // 默认
    };
    
    // 3. 逐步骤执行
    execute_steps(auto, window, &structured.steps, &walker, filter)
}
```

---

## 4. 函数命名语义化方案

### 4.1 命名原则

| 原则 | 说明 | 示例 |
|------|------|------|
| **动词-宾语** | 函数名清晰表达"做什么+操作什么" | `execute_xpath_step` > `findall_chain_first` |
| **避免缩写** | 全称优于缩写，`find_by_xpath` 保留（已成领域术语） | `findall` → `find_all` |
| **层级一致** | 同一抽象层级的函数命名风格一致 | 全部用 `find_*` 或全部用 `search_*` |
| **消歧后缀** | 同名函数用后缀区分算法而非场景 | `_via_control_view` 优于 `_control_descendants` |
| **命名即文档** | 函数名本身就说明行为，不需要读注释才理解 | `execute_xpath_steps_descendant` 一目了然 |

### 4.2 旧名 → 新名映射表

#### find.rs — 主调度

| 旧名 | 新名 | 语义改进 | 优先级 |
|------|------|----------|--------|
| `find_by_xpath_with_fallback` | `execute_xpath_steps` | "fallback"暗示自动回退，违反需求原则；`execute_steps` 表达"逐步骤执行" | P0 |
| `find_by_xpath_with_fallback_filtered` | `execute_xpath_steps_filtered` | 同上 | P0 |
| `find_all_elements_detailed` | `find_elements_by_xpath` | "detailed"模糊，"by_xpath"清晰表达来源 | P1 |
| `find_all_elements_from_root` | `find_elements_from_desktop` | "root"歧义（哪个 root？），"desktop"明确 | P1 |
| `find_from_element_impl` | `find_elements_from_element_raw` | `_impl` 是代码细节泄露，`_raw` 表示返回原始 UIElements | P1 |
| `find_from_element_cached` | `locate_by_runtime_id` | 函数做的是"按 runtime_id 定位"，`_cached` 是内部优化细节 | P1 |
| `find_from_element_cached_filtered` | `locate_by_runtime_id_filtered` | 同上 | P1 |
| `filter_findall_results` | `filter_elements_post_search` | `findall` 是旧风格，表达"搜索后过滤" | P2 |
| `parse_xpath_step` | `parse_xpath_step` | ✅ 保留，语义清晰 | — |
| `element_matches_parsed_step` | `element_matches_xpath_step` | 简化，"parsed"是内部细节 | P2 |
| `build_uia_condition_from_step` | `build_uia_condition` | 简化，参数类型已表意 | P2 |
| `step_has_complex_predicates` | `xpath_step_needs_client_filter` | 更精确：has_complex → 需要客户端过滤 | P2 |

#### find_control.rs — ControlView 搜索

| 旧名 | 新名 | 语义改进 | 优先级 |
|------|------|----------|--------|
| `find_by_xpath_control_descendants` | `search_descendants_via_control_view` | `_via_control_view` 明确策略来源 | P1 |
| `find_by_xpath_detailed` | `search_descendants_via_uiauto_xpath` | "detailed"无意义，`_via_uiauto_xpath` 表达底层引擎 | P1 |
| `find_by_xpath_detailed_strict` | `search_descendants_via_control_walker` | "strict"模糊，"control_walker"精确 | P1 |
| `findall_chain_first` | `search_descendants_chain_find_first` | `findall_chain` 不清晰，`chain_find_first` 表达链式 | P1 |
| `findall_chain_all` | `search_descendants_chain_find_all` | 同上 | P1 |
| `walk_control_tree_steps` | `walk_child_axis_via_control_view` | "tree_steps"歧义，"child_axis"对应 XPath `/A/B` 子轴 | P2 |

#### find_raw.rs — RawView 搜索

| 旧名 | 新名 | 语义改进 | 优先级 |
|------|------|----------|--------|
| `find_by_xpath_raw_descendants` | `search_descendants_via_raw_view` | 与 control_view 命名一致 | P1 |
| `find_by_xpath_raw_descendants_with_depth` | `search_descendants_depth_limited` | `_with_depth` 被动，"depth_limited"主动表达限制 | P1 |
| `walk_raw_tree_steps` | `walk_child_axis_via_raw_view` | 与 control_view 命名一致 | P2 |

#### validation.rs

| 旧名 | 新名 | 语义改进 | 优先级 |
|------|------|----------|--------|
| `validate_selector_and_xpath_detailed` | `validate_xpath` | "detailed" 是内部细节，`_and_` 过长 | P1 |
| `parse_first_xpath_step` | `extract_child_hint_from_xpath` | "parse_first"做什么？`extract_child_hint` 清晰 | P2 |
| `strip_first_xpath_step` | `strip_xpath_prefix_and_root` | 现在实际做的：剥离前缀+根步骤 | P2 |

#### xpath.rs — XPath 生成

| 旧名 | 新名 | 语义改进 | 优先级 |
|------|------|----------|--------|
| `generate` | `generate_xpath_from_hierarchy` | "generate"太泛 | P1 |
| `generate_elements` | `generate_xpath_string` | 返回 String 而非 elements | P1 |
| `generate_simplified_elements` | `generate_simplified_xpath` | 简化命名 | P2 |

### 4.3 替换策略

采用 **"先增后删"** 三步法，确保零风险：

```
Step A: 新增新函数（带新名），保留旧函数标记 #[deprecated]
Step B: 将所有调用点从旧函数改为新函数
Step C: 删除旧函数
```

每个函数替换为独立 commit/PR，可独立验证和回滚。

### 4.4 命名模式统一

整改后，所有搜索函数的命名遵循统一模式：

```
{action}_{target}_{strategy}
```

| 位置 | 含义 | 可选值 |
|------|------|--------|
| action | 操作 | `find`, `search`, `locate`, `walk`, `execute`, `validate` |
| target | 目标 | `elements`, `descendants`, `child_axis`, `xpath` |
| strategy | 策略/方式 | `via_control_view`, `via_raw_view`, `chain_find_first`, `depth_limited` |

**示例**：
- `search_descendants_via_control_view` = search + descendants + via_control_view
- `execute_xpath_steps` = execute + xpath + steps
- `locate_by_runtime_id` = locate + (element) + by_runtime_id

---

## 5. 单元测试规划

### 5.1 测试策略

```
原则：每个函数在替换前先有单元测试 → 新增新函数并测试 → 替换调用点 → 删旧函数
```

测试分层：
- **L1 — 纯函数测试**（解析、匹配、条件构建）：无需 UIA 环境，直接断言
- **L2 — 执行测试**（搜索、定位、校验）：需要 mock/fake UIA 环境
- **L3 — 集成测试**（端到端）：完整流程，依赖真实或预录制的 UIA 树

### 5.2 阶段 1 测试：XPath 步骤解析

**文件**：`src/core/uia/tests/xpath_parser_tests.rs`（新增）

| 测试函数 | 测试内容 | 类型 |
|----------|----------|------|
| `test_parse_child_prefix` | `/Button[@Name='OK']` → `XPathStepPrefix::Child` | L1 |
| `test_parse_descendant_prefix` | `//Button[@Name='OK']` → `XPathStepPrefix::Descendant` | L1 |
| `test_parse_depth_2` | `/*/Button[@Name='OK']` → `XPathStepPrefix::DepthLimited { max_depth: 2 }` | L1 |
| `test_parse_depth_n` | `/*5/Button[@Name='OK']` → `DepthLimited { max_depth: 6 }` | L1 |
| `test_parse_eq_predicate` | `[@Name='OK']` → `required_props: [("Name", "OK")]` | L1 |
| `test_parse_starts_with` | `[@Name=starts-with('Chrome')]` → `require_starts_with: [("Name", "Chrome")]` | L1 |
| `test_parse_contains` | `[@Name=contains('Widget')]` → `require_contains: [("Name", "Widget")]` | L1 |
| `test_parse_matches` | `[@Name=matches('^Chrome.*')]` → `require_matches: [("Name", Regex)]` | L1 |
| `test_parse_multiple_predicates` | `[@Name='OK' and @AutomationId='btn1']` → 两个 required_props | L1 |
| `test_parse_or_predicate` | `[@Name='OK' or @Name='Cancel']` → `is_complex = true` | L1 |
| `test_parse_not_predicate` | `[not(@IsOffscreen)]` → `is_complex = true` | L1 |
| `test_parse_wildcard_type` | `/*/[@Name='OK']` → `type_name = None` | L1 |
| `test_parse_empty_predicate` | `/Button` → 空 predicates | L1 |
| `test_parse_invalid_rejects` | 非法语法应返回 Err | L1 |
| `test_precompiled_regex_used` | 验证使用 `Lazy<Regex>` 而非 `Regex::new()` | L1 |

**小计**：15 个 L1 测试

### 5.3 阶段 2 测试：执行策略

**文件**：`src/core/uia/tests/xpath_executor_tests.rs`（新增）

| 测试函数 | 测试内容 | 类型 |
|----------|----------|------|
| `test_direct_child_find_first` | `/Button` → `FindFirst(Children, condition)` 被调用 | L2 |
| `test_descendant_find_first` | `//Button` → `FindFirst(Descendants, condition)` 被调用 | L2 |
| `test_depth_2_bfs` | `/*/Button` → BFS 深度=2，不越层搜索 | L2 |
| `test_depth_5_bfs` | `/*4/Button` → BFS 深度=5 | L2 |
| `test_depth_limit_honored` | `/*2/` 下不应搜索到深度 4 的节点 | L2 |
| `test_no_implicit_fallback` | 指定策略失败时直接返回 Err，不尝试其他策略 | L2 |
| `test_strategy_fast_first_step` | Fast 模式第一步用 ControlViewWalker | L2 |
| `test_strategy_full_first_step` | Full 模式第一步用 RawViewWalker | L2 |
| `test_chain_find_first` | `//A//B//C` 链式 FindFirst 逐层搜索 | L2 |
| `test_chain_find_all` | 末步链式 FindAll 返回多个结果 | L2 |
| `test_chain_fallback_on_complex` | 复杂谓词时链式返回 None，回退到 BFS | L2 |
| `test_timeout_protection` | `/*100/` 超时保护触发 | L2 |

**小计**：12 个 L2 测试

### 5.4 阶段 3 测试：定位语义

**文件**：`src/core/uia/tests/locate_semantics_tests.rs`（新增）

| 测试函数 | 测试内容 | 类型 |
|----------|----------|------|
| `test_find_one_unique` | 父节点下仅一个匹配 → Ok | L2 |
| `test_find_one_not_unique` | 父节点下两个匹配 → `LeafNotUnique` | L2 |
| `test_find_one_not_found` | 父节点下无匹配 → `StepNotFound` | L2 |
| `test_find_one_across_siblings` | 验证只在直接父节点下验证，不跨兄弟 | L2 |
| `test_find_all_eq_filter` | FilterCondition Eq 过滤 | L2 |
| `test_find_all_not_eq_filter` | FilterCondition NotEq 过滤 | L2 |
| `test_find_all_contains_filter` | FilterCondition Contains 过滤 | L2 |
| `test_find_all_regex_filter` | FilterCondition Regex 过滤 | L2 |
| `test_find_all_exists_filter` | FilterCondition Exists 过滤 | L2 |
| `test_find_all_combined_filters` | 多个 FilterCondition 组合 | L2 |

**小计**：10 个 L2 测试

### 5.5 阶段 4 测试：结构化 XPath

**文件**：`src/core/uia/tests/structured_xpath_tests.rs`（新增）

| 测试函数 | 测试内容 | 类型 |
|----------|----------|------|
| `test_parse_structured_fast` | `[fast]/Group/Button` → `StructuredXPath` | L1 |
| `test_parse_structured_fast_child` | `[fast-child @ClassName='X']/Text` → 带 hint | L1 |
| `test_parse_structured_full` | `[full]//Text` → `StructuredXPath` | L1 |
| `test_to_pure_xpath` | `to_pure_xpath()` 去掉元信息 | L1 |
| `test_to_display_string` | `to_string()` 含可读信息 | L1 |
| `test_roundtrip` | parse → to_string → parse 一致性 | L1 |
| `test_search_mode_suffix` | `:first` / `:all` / `:only-one` 后缀解析 | L1 |
| `test_parse_invalid_prefix` | 无效前缀返回 `XPathParseError` | L1 |

**小计**：8 个 L1 测试

### 5.6 阶段 5 测试：代码优化验证

**文件**：`src/core/uia/tests/code_quality_tests.rs`（新增）

| 测试函数 | 测试内容 | 类型 |
|----------|----------|------|
| `test_apply_search_mode_generic` | 泛型 `apply_search_mode` 对 `UIElement` 和 `ElementData` 行为一致 | L1 |
| `test_uia_attribute_enum` | `UiaAttribute` 枚举 `get_value()` 返回正确值 | L1 |
| `test_segment_result_factory` | `SegmentValidationResult::new()` 工厂函数 | L1 |
| `test_no_duplicate_functions` | 代码审查：确认 `apply_search_mode` 仅 1 个泛型版本 | L1 |
| `test_no_regex_new_in_parse` | 代码审查：确认 `parse_xpath_step` 不使用 `Regex::new()` | L1 |

**小计**：5 个 L1 测试

### 5.7 回归测试：保留并适配现有测试

**现有测试文件**：需要适配函数重命名

| 文件 | 测试数 | 适配工作 |
|------|--------|----------|
| `src/core/uia/validation.rs` 内 `#[cfg(test)]` | ~27 | 函数重命名 |
| `src/core/xpath.rs` 内 `#[cfg(test)]` | ~10 | 函数重命名 |
| 其他内联测试 | ~5 | 函数重命名 |

### 5.8 测试执行顺序

```
每个阶段内：
  1. 编写新函数的 L1 测试（纯函数，无需环境）
  2. 实现新函数，跑通测试
  3. 编写 L2 测试（需要 mock 环境）
  4. 新增新函数，标记旧函数 #[deprecated]
  5. cargo test -- 确认 0 失败
  6. 替换所有调用点
  7. cargo test -- 确认无回归
  8. 删除旧函数
  9. cargo test -- 最终确认
```

### 5.9 测试文件结构

```
src/core/uia/
├── tests/                              # 新增测试目录
│   ├── mod.rs                          # 测试模块入口
│   ├── xpath_parser_tests.rs           # 阶段 1：15 个测试
│   ├── xpath_executor_tests.rs         # 阶段 2：12 个测试
│   ├── locate_semantics_tests.rs       # 阶段 3：10 个测试
│   ├── structured_xpath_tests.rs       # 阶段 4：8 个测试
│   └── code_quality_tests.rs           # 阶段 5：5 个测试
```

---

## 6. 整改优先级和时间估算

| 阶段 | 优先级 | 编码 | 测试 | 替换+删旧 | 合计 | 描述 |
|------|--------|------|------|-----------|------|------|
| 阶段 1.1-1.4 | P0 | 2h | 2h | 1h | **5h** | XPath 步骤解析重构（前缀枚举、ParsedXPathStep 扩展、预编译正则） |
| 阶段 2.1-2.2 | P0 | 3h | 2h | 1h | **6h** | 执行策略重构（深度限制 BFS、策略枚举、函数重命名） |
| 阶段 2.3 | P0 | 2h | 1h | 1h | **4h** | 移除隐式 fallback，分离开发/生产路径 |
| 阶段 3.1 | P1 | 1h | 1h | 1h | **3h** | `findOne` 叶子唯一性验证 |
| 阶段 3.2 | P2 | 1h | 1h | 1h | **3h** | `findAll` FilterCondition |
| 阶段 4.1-4.2 | P1 | 2h | 1h | 1h | **4h** | XPath 结构化表示 |
| 阶段 5.1-5.4 | P2 | 3h | 1h | 2h | **6h** | 代码结构优化（模块化、消除重复、枚举化、命名统一） |

**总计**：约 31 小时（原 24h + 新增 7h 用于测试和函数重命名）

---

## 7. 风险与注意事项

1. **向后兼容**：现有 XPath 字符串格式（`[fast]/Group/Button`）需要继续支持，新增结构化表示作为内部使用
2. **函数重命名风险**：采用"先增后删"策略，每个函数替换独立可回滚；`#[deprecated]` 注解确保编译期警告
3. **性能回归**：深度限制 BFS 在大型子树上可能较慢，需要性能测试；新增测试中包含超时保护验证
4. **测试覆盖**：每个整改阶段先编写测试再实现，确保 50 个新增测试全部通过后才进入下一阶段
5. **uiauto-xpath 依赖**：`search_descendants_via_uiauto_xpath`（原 `find_by_xpath_detailed`）依赖 uiauto-xpath 库，需确认其对 `/*n/` 语法的支持情况
6. **缓存失效**：XPath 解析变更后，现有的 `CompiledStrategy` 缓存需要更新或清空
7. **测试环境依赖**：L2 测试需要 UIA 环境（至少一个运行中的窗口），CI 中需确保有可用 UIA 服务

---

## 8. 验收标准

### 功能验收
- [x] `parse_xpath_step` 能正确解析 `/A`、`//A`、`/*/A`、`/*2/A`、`/*n/A` 五种前缀 ✅
- [x] 深度限制 BFS 正确执行 `/*n/` 语法，深度限制生效 ✅
- [x] `findOne` 在父节点下验证叶子唯一性，多余一个时报 `LeafNotUnique` ✅
- [x] `findAll` 支持 Eq/NotEq/Contains/Regex/Exists 五种 FilterCondition ✅ (`AttributeFilter`)
- [x] 生产路径不含任何隐式 fallback ✅ (`execute_xpath_steps_filtered` 无隐式回退)
- [x] 正则表达式预编译，无每次 `Regex::new()` 调用 ✅ (`Lazy<Regex>`)
- [x] `apply_search_mode` 合并为泛型函数 ✅
- [ ] `get_uia_property_for_xpath` 使用枚举替代字符串 key

### 命名验收
- [ ] 所有搜索函数遵循 `{action}_{target}_{strategy}` 命名模式
- [x] 旧函数全部标记 `#[deprecated]` 或已删除 ✅ (deprecated 已全部清除)
- [ ] 无 `_impl`、`_detailed` 等泄露实现细节的后缀

### 测试验收
- [ ] 50 个新增测试全部通过（当前 27 L1 测试 + 集成测试 = 179 总测试通过）
- [x] 所有现有测试通过，无回归 ✅ (152 单元测试 + 27 集成测试)
- [ ] 每个新增函数至少 1 个 L1 测试
- [x] 每个阶段完成后 `cargo test` 零失败 ✅

### 编译验收
- [x] `cargo build` 通过（debug 模式） ✅
- [x] 0 lint 错误 ✅
- [x] 0 lint 警告（deprecated 除外） ✅
