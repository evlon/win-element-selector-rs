# 捕获与定位系统 — 需求与概要设计

## 一、核心概念定义

### 1.1 四个操作，语义必须分清

| 操作 | 定义 | 输入 | 输出 | 典型场景 |
|------|------|------|------|---------|
| **捕获 (Capture)** | 用户指向屏幕某个位置，系统识别该位置的 UI 元素及其层级路径 | 坐标 (x, y) | 层级链 + XPath + 元素信息 + 搜索上下文 | 用户首次"看到"一个元素 |
| **一次定位 (Locate)** | 根据之前捕获生成的选择器，从窗口根重新找到该元素 | 窗口选择器 + XPath + 搜索上下文 | 匹配的元素列表 + 校验详情 | 脚本执行时"重新找到"元素 |
| **校验 (Validate)** | 检查选择器在当前窗口状态下是否仍能匹配到元素 | 窗口选择器 + XPath + 搜索上下文 | Found/NotFound + 逐层校验详情 + 失败原因 | 确认选择器仍然有效 |
| **二次定位 (LocateFrom)** | 相对已知的父元素，在其子树中搜索子元素 | 父元素 runtime_id + 相对 XPath + 搜索策略 | 匹配的元素列表 | 已有父元素，快速定位子元素 |

> **关键语义区分**：
> - **捕获**是"从屏幕坐标到元素"（鼠标驱动）
> - **一次定位**是"从窗口根到元素"（XPath 驱动，全量搜索）
> - **二次定位**是"从已知元素到子元素"（XPath 驱动，局部搜索）
> - **校验**是"确认路径仍然有效"（只读诊断）

### 1.2 捕获模式 (CaptureMode) — 用户选择，2种

捕获模式是**用户主动选择**的操作模式，描述的是"用什么策略去发现元素"。

| 模式 | 含义 | Walker | 深度 | 子窗口处理 |
|------|------|--------|------|-----------|
| **Normal** (普通) | 快速捕获，适用于原生应用 | ControlView | 浅（ElementFromPoint + 轻量 BFS 5层） | 检测到跨进程时截断层级链 |
| **Enhanced** (增强) | 深度捕获，适用于嵌入式框架 | RawView | 深（BFS 最多 30 层） | 检测到跨进程时切换到子HWND搜索 |

> 用户不会说"我要 fast-child 捕获"——是否跨进程、是否需要切换子窗口，是系统**自动检测**的结果，不是用户选择的。

### 1.3 定位模式 (LocateMode) — 系统决定，4种

定位模式是捕获结果的**附属属性**，描述的是"用这个捕获结果去定位时，应该怎么搜索"。
它由捕获模式 + 运行时条件（是否检测到跨进程）共同决定。

| 定位模式 | 对应捕获模式 | 是否跨进程 | Walker | 搜索起点 | XPath 前缀 |
|---------|------------|-----------|--------|---------|-----------|
| **Fast** | Normal | 否 | ControlView | 窗口根 | `[fast]` |
| **FastChild** | Normal | 是 | ControlView | 子HWND根 | `[fast-child]` |
| **Full** | Enhanced | 否 | RawView | 窗口根 | `[full]` |
| **FullChild** | Enhanced | 是 | RawView | 子HWND根 | `[full-child]` |

**映射关系**：

```
Normal 捕获 + 未跨进程 → LocateMode::Fast
Normal 捕获 + 跨进程   → LocateMode::FastChild
Enhanced 捕获 + 未跨进程 → LocateMode::Full
Enhanced 捕获 + 跨进程   → LocateMode::FullChild
```

> "是否跨进程"由系统在捕获时自动检测，记录在 `SearchContext.child_hwnd_hint` 中：
> - `child_hwnd_hint = None` → 未跨进程
> - `child_hwnd_hint = Some(...)` → 跨进程，且记录了命中的子HWND信息

### 1.4 当前问题与改进方向

> **当前问题**：Fast 模式没有 BFS，ElementFromPoint 可能停在容器层（Group/Pane），找不到叶子元素。
> **改进**：Normal 捕获增加轻量 BFS（5 层 ControlView），找到最深的叶子。

---

## 二、当前实现的问题

### 2.1 捕获问题

| ID | 问题 | 影响 |
|----|------|------|
| C-1 | Normal 捕获无 BFS，ElementFromPoint 可能停在容器层 | XPath 不精确，定位脆弱 |
| C-2 | 捕获结果不区分捕获模式和定位模式，4种模式混为一谈 | 语义模糊，前端难以理解 |
| C-3 | Child 定位模式不记录命中的是哪个子 HWND | 定位时盲枚举所有子 HWND，可能误匹配 |
| C-4 | 不支持"先 Normal 捕获父容器，再 Enhanced 定位子元素" | 无法组合使用两种模式 |

### 2.2 一次定位问题

| ID | 问题 | 影响 |
|----|------|------|
| L1-1 | 定位策略通过 XPath 前缀隐式关联，非显式 | 行为不透明，难以调试 |
| L1-2 | Child 定位模式枚举所有子 HWND 盲试 | 多子窗口场景误匹配 |
| L1-3 | 超时预算不可配置 | Fast/Full 用同一预算，不合理 |

### 2.3 二次定位问题

| ID | 问题 | 影响 |
|----|------|------|
| L2-1 | 不感知搜索策略，走默认回退链 | 无法指定 Fast/Full |
| L2-2 | 无法表达"先 Normal 捕获父容器，再 Full 二次定位" | 组合模式不可用 |

### 2.4 校验问题

| ID | 问题 | 影响 |
|----|------|------|
| V-1 | NotFound 不区分原因 | 无法告诉用户是窗口没了、子窗口变了、还是元素消失了 |

---

## 三、需求规格

### 3.1 捕获需求

| ID | 需求 | 优先级 | 验收标准 |
|----|------|--------|---------|
| REQ-C-1 | Normal 捕获应找到叶子元素 | P0 | Normal 捕获的 `is_target` 元素应为叶子类型（Text/Button/Edit 等），而非 Group/Pane |
| REQ-C-2 | 捕获结果应携带定位模式和搜索上下文 | P0 | `CaptureResult` 包含 `locate_mode` + `SearchContext`，前端可原样传回给定位/校验 API |
| REQ-C-3 | Child 定位模式精确标识目标子窗口 | P1 | 定位时只搜索匹配的子 HWND，不盲试其他子窗口 |
| REQ-C-4 | 支持组合模式（先 Normal 再 Enhanced 定位） | P1 | 二次定位 API 支持指定 Full 策略 |

### 3.2 一次定位需求

| ID | 需求 | 优先级 | 验收标准 |
|----|------|--------|---------|
| REQ-L1-1 | 定位行为由 SearchContext 显式决定 | P0 | `SearchContext.locate_mode` 传入定位 API，搜索策略与捕获时一致 |
| REQ-L1-2 | Child 定位模式精确匹配子 HWND | P1 | 根据 `child_hwnd_hint` 过滤，只搜索匹配的子窗口 |
| REQ-L1-3 | 超时预算可配置 | P2 | Fast 默认 1500ms，Full 默认 3000ms，可自定义 |

### 3.3 二次定位需求

| ID | 需求 | 优先级 | 验收标准 |
|----|------|--------|---------|
| REQ-L2-1 | 二次定位感知搜索策略 | P0 | API 支持 `SearchStrategy` 参数：Fast/Full/Adaptive |
| REQ-L2-2 | 组合模式：Normal 父容器 → Full 二次定位 | P1 | `SearchStrategy::Full` 在子树中用 RawView 深搜 |

### 3.4 校验需求

| ID | 需求 | 优先级 | 验收标准 |
|----|------|--------|---------|
| REQ-V-1 | 校验结果包含失败原因 | P1 | `NotFound` 携带 `NotFoundReason` 枚举 |

---

## 四、概要设计

### 4.1 数据模型增强

#### 4.1.1 捕获模式与定位模式

```rust
/// 捕获模式 — 用户选择，2种
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CaptureMode {
    /// 普通捕获：ControlViewWalker，轻量 BFS，适用于原生应用
    Normal,
    /// 增强捕获：RawViewWalker，深度 BFS + Cross-HWND，适用于嵌入式框架
    Enhanced,
}

/// 定位模式 — 由捕获模式 + 是否跨进程自动决定，4种
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LocateMode {
    /// ControlView，从窗口根搜索
    Fast,
    /// ControlView，从子HWND根搜索
    FastChild,
    /// RawView，从窗口根搜索
    Full,
    /// RawView，从子HWND根搜索
    FullChild,
}
```

> **映射**：`CaptureMode::Normal + 无跨进程` → `LocateMode::Fast`，以此类推。
> XPath 前缀（`[fast]`/`[fast-child]`/`[full]`/`[full-child]`）对应的是 `LocateMode`，不是 `CaptureMode`。

#### 4.1.2 SearchContext — 搜索上下文

捕获时生成，随 `CaptureResult` 返回前端；定位/校验时前端原样传回。

```rust
/// 捕获时的搜索上下文，供后续定位/校验使用
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchContext {
    /// 定位模式（由捕获模式 + 是否跨进程决定）
    pub locate_mode: LocateMode,
    /// 命中的子 HWND 信息（FastChild/FullChild 时有值）
    pub child_hwnd_hint: Option<ChildHwndHint>,
    /// 搜索起点类型
    pub search_root: SearchRoot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildHwndHint {
    pub hwnd_class: String,     // 子 HWND 的窗口类名
    pub hwnd_title: String,     // 子 HWND 的窗口标题
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SearchRoot {
    /// 从窗口根搜索
    Window,
    /// 从特定子 HWND 根搜索
    ChildHwnd { class: String, title: String },
    /// 从缓存元素搜索（二次定位）
    Element { runtime_id: String },
}
```

> **与旧设计的区别**：去掉了 `WalkerType` 枚举，因为 Walker 类型已隐含在 `LocateMode` 中：
> - `LocateMode::Fast` / `FastChild` → ControlViewWalker
> - `LocateMode::Full` / `FullChild` → RawViewWalker

#### 4.1.3 CaptureResult 增强

```rust
pub struct CaptureResult {
    pub hierarchy: Vec<HierarchyNode>,
    pub xpath: String,
    pub cursor_x: i32,
    pub cursor_y: i32,
    pub capture_mode: CaptureMode,     // Normal / Enhanced（2值）
    pub window_info: Option<WindowInfo>,
    pub error: Option<String>,

    // ===== 新增 =====
    pub locate_mode: LocateMode,       // Fast/FastChild/Full/FullChild（4值，自动决定）
    pub search_context: SearchContext,  // 搜索上下文，供定位/校验使用
}
```

> `capture_mode` 记录用户选择（2值），`locate_mode` 记录系统决定的定位策略（4值）。

#### 4.1.4 SearchStrategy — 二次定位搜索策略

```rust
/// 二次定位的搜索策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SearchStrategy {
    /// 快速搜索：ControlViewWalker，限制深度
    Fast { max_depth: u32 },
    /// 深度搜索：RawViewWalker，可深入子树
    Full { max_depth: u32 },
    /// 自适应：先 Fast，失败回退 Full（默认）
    Adaptive,
}
```

#### 4.1.5 NotFoundReason — 校验失败原因

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotFoundReason {
    /// 窗口未找到
    WindowNotFound,
    /// 子窗口未找到（Child 模式）
    ChildHwndNotFound { class: String },
    /// XPath 第 N 步匹配失败
    XPathStepFailed { step: usize, detail: String },
    /// 元素已消失（窗口在但元素不在）
    ElementGone,
    /// 搜索超时
    Timeout { budget_ms: u64, elapsed_ms: u64 },
}
```

### 4.2 API 接口变更

#### 4.2.1 捕获接口（响应增强）

```
POST /api/element/capture          ← 普通捕获 (CaptureMode::Normal)
请求：{ x: i32, y: i32 }
响应：CaptureResponse {
    ...现有字段...,
    captureMode: "Normal",         // 用户选择的捕获模式
    locateMode: "Fast",            // 系统决定的定位模式
    searchContext: SearchContext,   // 搜索上下文
}
```

```
POST /api/element/capture-enhanced ← 增强捕获 (CaptureMode::Enhanced)
请求：{ x: i32, y: i32 }
响应：CaptureResponse {
    ...现有字段...,
    captureMode: "Enhanced",
    locateMode: "Full",            // 或 "FullChild"（如果检测到跨进程）
    searchContext: SearchContext,
}
```

> **关键**：`captureMode` 是用户选的（2值），`locateMode` 是系统自动决定的（4值）。
> 前端无需关心 `locateMode` 的计算逻辑——它已包含在 `searchContext` 中，定位/校验时原样传回即可。

#### 4.2.2 一次定位接口（增加 searchContext 参数）

```
POST /api/element/locate
请求：{
    windowSelector: String,          // 不变
    xpath: String,                   // 不变
    searchContext: Option<SearchContext>,  // 新增：从捕获结果传入
}
响应：不变
```

#### 4.2.3 二次定位接口（增加 searchStrategy 参数）

```
POST /api/element/locate-from
请求：{
    runtimeId: String,               // 不变
    xpath: String,                   // 不变
    searchStrategy: Option<SearchStrategy>,  // 新增，默认 Adaptive
}
响应：不变
```

#### 4.2.4 校验接口（增加 searchContext 参数，响应增强）

```
POST /api/element/validate
请求：{
    windowSelector: String,          // 不变
    xpath: String,                   // 不变
    searchContext: Option<SearchContext>,  // 新增
}
响应：DetailedValidationResponse {
    ...现有字段...,
    notFoundReason: Option<NotFoundReason>,  // 新增
}
```

### 4.3 捕获流程改进

#### 4.3.1 Normal 捕获增加轻量 BFS

```
当前 Normal 捕获流程：
  ElementFromPoint → 直取元素 → 构建 ancestor 链

改进 Normal 捕获流程：
  ElementFromPoint → hit_elem
  → ControlViewWalker BFS（最多5层）→ 找最深的叶子
  → 构建 ancestor 链

BFS 策略：
  - Walker: ControlViewWalker
  - 最大深度: 5
  - 选择策略: 面积最小的包含光标点的子元素
  - 超时: 100ms
  - 不做 Cross-HWND 枚举
  - 不做自身进程过滤
```

#### 4.3.2 跨进程检测与 LocateMode 决定

```
捕获流程中，跨进程检测是自动的：

Normal 捕获：
  1. ElementFromPoint → hit_elem
  2. 检测 hit_elem 的 PID 与窗口 PID 是否一致
  3. 如一致 → LocateMode = Fast，search_root = Window
  4. 如不一致 → 截断层级链到跨边界节点
               → LocateMode = FastChild
               → search_root = ChildHwnd { class, title }
               → child_hwnd_hint = Some(命中的子HWND信息)

Enhanced 捕获：
  1. ElementFromPoint → hit_elem
  2. BFS 深搜 + Cross-HWND 枚举
  3. 如 Cross-HWND 未改变 hit_elem → LocateMode = Full，search_root = Window
  4. 如 Cross-HWND 改变了 hit_elem → LocateMode = FullChild
                                    → search_root = ChildHwnd { class, title }
                                    → child_hwnd_hint = Some(命中的子HWND信息)
```

> **核心**：用户只选 Normal/Enhanced，是否 Child 由系统自动检测，结果记录在 `LocateMode` 和 `SearchContext` 中。

#### 4.3.3 捕获结果自动生成 LocateMode + SearchContext

| 捕获模式 | 是否跨进程 | LocateMode | child_hwnd_hint | search_root |
|---------|-----------|------------|-----------------|-------------|
| Normal | 否 | Fast | None | Window |
| Normal | 是 | FastChild | Some(命中子HWND) | ChildHwnd { ... } |
| Enhanced | 否 | Full | None | Window |
| Enhanced | 是 | FullChild | Some(命中子HWND) | ChildHwnd { ... } |

> `LocateMode` 决定 XPath 前缀：Fast→`[fast]`、FastChild→`[fast-child]`、Full→`[full]`、FullChild→`[full-child]`

### 4.4 定位/校验流程改进

#### 4.4.1 一次定位使用 SearchContext

```
当前：
  解析 xpath 前缀 → 隐式决定搜索策略

改进：
  search_context 传入 → 显式决定搜索策略
  - locate_mode 决定用哪种 Walker（ControlView / RawView）
  - search_root 决定从哪里开始搜索（Window / ChildHwnd）
  - child_hwnd_hint 决定枚举哪些子 HWND

  兼容：searchContext=None 时回退到当前行为（解析 xpath 前缀）
```

#### 4.4.2 Child 模式精确匹配子 HWND

```
当前：enum_child_hwnds(hwnd) → 对每个子 HWND 都搜索 → 取第一个结果

改进：enum_child_hwnds(hwnd)
  → 过滤 class/title 匹配 child_hwnd_hint 的子 HWND
  → 只对匹配的子 HWND 搜索
  → 找不到匹配的子 HWND → NotFoundReason::ChildHwndNotFound
```

#### 4.4.3 二次定位使用 SearchStrategy

```
当前：find_from_element_cached(runtime_id, xpath)
  → 从缓存取 UIElement → find_by_xpath_detailed(默认回退链)

改进：find_from_element_cached(runtime_id, xpath, strategy)
  → 从缓存取 UIElement
  → strategy=Fast: ControlViewWalker 限制深度搜索
  → strategy=Full: RawViewWalker 深度搜索
  → strategy=Adaptive: 先 Fast 失败回退 Full（默认行为）

> SearchStrategy 与 LocateMode 的关系：
> - SearchStrategy 是二次定位时用户/前端指定的策略（3值）
> - LocateMode 是捕获时系统决定的定位方式（4值）
> - 二次定位不直接使用 LocateMode，因为搜索起点是缓存元素而非窗口根
> - 但 SearchStrategy::Fast/Full 对应 LocateMode::Fast/Full 的 Walker 选择
```

---

## 五、BuildCache 使用设计

### 5.1 BuildCache 核心原理

UIA 的 BuildCache 机制：**一次跨进程 COM 调用，批量取回 N 个属性**。

```
不使用 BuildCache（逐一读取）：
  元素A → COM调用 → Name      (≈0.1ms)
  元素A → COM调用 → ClassName  (≈0.1ms)
  元素A → COM调用 → Rect      (≈0.1ms)
  10个属性 = 10次跨进程调用 ≈ 1ms

使用 BuildCache（批量预取）：
  元素A → 1次COM调用 → {Name, ClassName, Rect, ...}  (≈0.2ms)
  10个属性 = 1次跨进程调用 ≈ 0.2ms  → 5x 加速
```

**关键约束**：BuildCache 只在**跨进程**场景有显著收益（UIA 服务是独立进程）。同进程调用缓存反而更慢。

### 5.2 使用决策标准

| 条件 | 决策 | 原因 |
|------|------|------|
| 同一元素读 **5+ 属性** | ✅ 用 | 5+ 次调用 → 1 次，收益显著 |
| 同一父元素下读 **20+ 子元素**属性 | ✅ 用 Children scope | 批量缓存子元素属性 |
| 只读 **1-3 个属性** | ❌ 不用 | BuildCache 创建+调用开销 > 收益 |
| 搜索引擎内部遍历 | ❌ 不用 | 不干预引擎内部缓存策略 |

### 5.3 各操作的 BuildCache 策略

#### ✅ 捕获：层级链构建（最核心场景）

```
场景：遍历 5-15 个祖先元素，每个读 8-10 个属性
不缓存：80-150 次跨进程调用
缓存后：5-15 次调用

CacheRequest 设计：
  add_property: Name, ClassName, AutomationId, ControlType,
                BoundingRectangle, ProcessId, FrameworkId,
                IsOffscreen, NativeWindowHandle
  set_tree_scope(Element)  // 只缓存元素自身

使用方式：
  1. Walker 走完祖先链，收集所有元素到 Vec
  2. 对每个元素 build_updated_cache(&cache_request)
  3. 用 get_cached_xxx() 读取属性
```

#### ✅ 捕获：BFS 深搜

```
场景：每层遍历 20-50 个子元素，每个读 3-4 个属性
不缓存：60-200 次跨进程调用
缓存后：20-50 次调用

CacheRequest 设计：
  add_property: BoundingRectangle, ProcessId, IsOffscreen, RuntimeId
  set_tree_scope(Children)  // 缓存直接子元素

使用方式：
  对当前层的父元素 build_updated_cache(Children)
  → 一次调用获取所有子元素的 4 个属性
```

#### ✅ 校验：属性比对

```
场景：对匹配元素读 5-8 个属性与 hierarchy filters 比较
不缓存：5-8 次调用
缓存后：1 次调用

CacheRequest 设计：
  add_property: Name, ClassName, AutomationId, ControlType,
                ProcessId, FrameworkId, BoundingRectangle
  set_tree_scope(Element)

使用方式：
  对匹配元素 build_updated_cache(Element)
  → 逐属性比对时用 get_cached_xxx()
```

#### ✅ 一次定位：窗口匹配 + 子 HWND 枚举

```
场景 A：对 3-5 个候选窗口读 Name/ClassName/PID
  CacheRequest(Element) → 每窗口 1 次调用替代 3 次

场景 B：对 N 个子 HWND 元素读 class/title/rect
  CacheRequest(Element) → 每元素 1 次调用替代 3 次
```

#### ❌ 二次定位

```
原因：
  - 缓存取 UIElement 是本地操作（0 次跨进程）
  - XPath 搜索由 uiauto-xpath 引擎内部处理
  - 找到后只读少量属性返回
  总跨进程调用很少，BuildCache 收益 < 开销
```

#### ❌ visibility 检测

```
原因：只读 3-4 个属性（Rect, IsOffscreen, Scrollable）
BuildCache 的创建和 build_updated_cache() 本身有开销，3-4 个属性不划算
```

### 5.4 BuildCache 使用总结

| 操作 | 用 BuildCache | 缓存范围 | 属性数 | 加速比 |
|------|-------------|---------|--------|--------|
| 捕获：层级链构建 | ✅ | Element 逐元素 | 8-10 × 5-15元素 | 5-10x |
| 捕获：BFS 深搜 | ✅ | Children 按层 | 4 × 20-50元素 | 3-5x |
| 校验：属性比对 | ✅ | Element 逐元素 | 5-8 × 1元素 | 3-5x |
| 一次定位：窗口匹配 | ✅ | Element 逐窗口 | 3 × 3-5窗口 | 2-3x |
| 一次定位：子 HWND 枚举 | ✅ | Element 逐元素 | 3 × N子窗口 | 2-3x |
| 二次定位 | ❌ | — | 1-3 | ~1x |
| visibility 检测 | ❌ | — | 3-4 | ~1x |
| XPath 搜索引擎内部 | ❌ | — | 引擎自管理 | — |

---

## 六、操作组合矩阵

| 场景 | 第一步 | 第二步 | 是否支持 | 说明 |
|------|--------|--------|---------|------|
| 原生应用标准 | Normal 捕获 (→Fast) | Fast 一次定位 | ✅ | 最常见场景 |
| 原生应用深搜 | Normal 捕获 (→Fast) | Full 一次定位 | ✅ 新增 | 修改 searchContext 中的 locate_mode |
| 嵌入框架 | Enhanced 捕获 (→FullChild) | FullChild 一次定位 | ✅ | 自动识别子 HWND |
| 先快后深 | Normal 捕获父容器 (→Fast) | Full 二次定位 | ✅ 新增 | searchStrategy=Full |
| 子窗口内标准 | Normal 捕获 (→FastChild) | FastChild 一次定位 | ✅ | 精确匹配子 HWND |
| 子窗口内深搜 | Enhanced 捕获 (→FullChild) | FullChild 二次定位 | ✅ | 精确匹配子 HWND |

> 括号内是系统自动决定的 LocateMode，用户只需选择 Normal/Enhanced 捕获。

---

## 七、与架构整改的关系

### 7.1 实施顺序

**架构整改（当前计划）** 和 **功能增强（本文档）** 是正交的：

1. **先完成架构整改**：类型下沉、uiautomation-rs 迁移、消除转发层、LRU 修复
2. **再实施功能增强**：SearchContext、Fast BFS、Child 精确匹配、SearchStrategy、NotFoundReason

### 7.2 架构整改中可预埋的接口

在架构整改过程中，可以预先定义数据结构（空壳/默认值），避免后续二次改 API：

- `SearchContext`：先定义结构体，所有字段默认值，`CaptureResult` 暂时填默认
- `SearchStrategy`：先定义枚举，`find_from_element_cached` 暂时忽略
- `NotFoundReason`：先定义枚举，校验结果暂时不填

### 7.3 BuildCache 与 uiautomation-rs 迁移的关系

uiautomation-rs 迁移是**前提**，BuildCache 是**优化手段**：

- 迁移后 `UIElement` 才有 `build_updated_cache()` 和 `get_cached_xxx()` 方法
- 迁移过程中可以先实现不使用 BuildCache 的版本（`get_xxx()` 逐一读取），功能验证通过后再替换为 BuildCache 版本
- 两种写法对上层调用者透明，仅 `element_to_node()` 等内部函数的实现不同

---

## 八、术语表

| 术语 | 含义 |
|------|------|
| **UIA** | Windows UI Automation，微软的辅助技术框架 |
| **ControlView** | UIA 控件视图，过滤掉装饰性/中间节点，产生更短更快的链 |
| **RawView** | UIA 原始视图，包含所有节点，遍历更慢但更完整 |
| **Walker** | `IUIAutomationTreeWalker` / `UITreeWalker`，用于遍历 UIA 树 |
| **BuildCache** | `UICacheRequest` + `build_updated_cache()`，批量预取属性 |
| **子 HWND** | `EnumChildWindows` 枚举的子窗口句柄，跨进程嵌入窗口 |
| **CaptureMode** | 捕获模式，用户选择：Normal（普通）/ Enhanced（增强） |
| **LocateMode** | 定位模式，系统决定：Fast / FastChild / Full / FullChild |
| **Child 模式** | LocateMode 中带 Child 后缀的模式，表示目标在子HWND内 |
| **SearchContext** | 捕获时生成的搜索上下文，包含 locate_mode + child_hwnd_hint + search_root |
| **SearchStrategy** | 二次定位的搜索策略：Fast/Full/Adaptive |
