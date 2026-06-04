# 捕获与定位系统 — 代码整改计划

基于 `docs/CAPTURE_LOCATE_DESIGN.md` 中的需求与概要设计，制定本整改计划。

## 前置条件

- [x] 架构整改已完成（uiautomation-rs 迁移、类型下沉、转发层消除）
- [x] 设计文档已写入 `docs/CAPTURE_LOCATE_DESIGN.md`

## 当前代码现状摘要

| 文件 | 现状 |
|------|------|
| `src/core/model.rs` | `CaptureMode` 是 4 值(Fast/Full/FastChild/FullChild)，无 `LocateMode`，无 `SearchContext` |
| `src/core/uia/capture.rs` | `capture_at_point` / `capture_enhanced_at_point` 直接产出 4 值 CaptureMode |
| `src/core/uia/find.rs` | `find_from_element_cached(runtime_id, xpath)` 无策略参数 |
| `src/core/uia/validation.rs` | `NotFound` 不区分原因 |
| `src/api/types.rs` | 无 CaptureResponse/LocateRequest，捕获走 GUI 直调 core |
| `src/api/element.rs` | `/api/element/find-from` 用 `FindFromElementRequest`，无 searchStrategy |
| `src/gui/iced_app.rs` | 直接调用 `core::uia::capture_at_point` 等 |

---

## 整改阶段划分

### 阶段一：数据模型重构（P0）

**目标**：CaptureMode 改 2 值 + 新增 LocateMode + SearchContext + NotFoundReason

#### 步骤 1.1：新增类型定义

**文件**：`src/core/model.rs`

```rust
// 新增：捕获模式（2值）
pub enum CaptureMode {
    Normal,     // 原 Fast + FastChild
    Enhanced,   // 原 Full + FullChild
}

// 新增：定位模式（4值）
pub enum LocateMode {
    Fast,
    FastChild,
    Full,
    FullChild,
}

// 新增：搜索上下文
pub struct SearchContext {
    pub locate_mode: LocateMode,
    pub child_hwnd_hint: Option<ChildHwndHint>,
    pub search_root: SearchRoot,
}

pub struct ChildHwndHint {
    pub hwnd_class: String,
    pub hwnd_title: String,
}

pub enum SearchRoot {
    Window,
    ChildHwnd { class: String, title: String },
    Element { runtime_id: String },
}

// 新增：二次定位搜索策略
pub enum SearchStrategy {
    Fast { max_depth: u32 },
    Full { max_depth: u32 },
    Adaptive,
}

// 新增：校验失败原因
pub enum NotFoundReason {
    WindowNotFound,
    ChildHwndNotFound { class: String },
    XPathStepFailed { step: usize, detail: String },
    ElementGone,
    Timeout { budget_ms: u64, elapsed_ms: u64 },
}
```

#### 步骤 1.2：修改 CaptureResult

**文件**：`src/core/model.rs`

```rust
pub struct CaptureResult {
    // ...现有字段...
    pub capture_mode: CaptureMode,     // 改为 2 值
    pub locate_mode: LocateMode,       // 新增
    pub search_context: SearchContext,  // 新增
}
```

#### 步骤 1.3：修改 CaptureMode 的辅助方法

**文件**：`src/core/model.rs`

- `CaptureMode::xpath_prefix()` → 删除（由 LocateMode 提供）
- `CaptureMode::is_child_mode()` → 删除（由 LocateMode 判断）
- `CaptureMode::from_xpath_prefix()` → 改为 `LocateMode::from_xpath_prefix()`
- `CaptureMode::strip_xpath_prefix()` → 改为 `LocateMode::strip_xpath_prefix()`
- 新增 `LocateMode::xpath_prefix()` 返回 `[fast]`/`[fast-child]`/`[full]`/`[full-child]`
- 新增 `LocateMode::is_child_mode()`
- 新增 `LocateMode::walker_type()` → 返回 WalkerHint::ControlView / RawView
- 新增 `LocateMode::from_capture_mode(capture_mode, is_cross_process)` 构造函数

#### 步骤 1.4：修改 ValidationResult

**文件**：`src/core/model.rs`

```rust
pub enum ValidationResult {
    Idle,
    Running,
    Found { count: usize, first_rect: Option<ElementRect>, rects: Vec<ElementRect> },
    NotFound { reason: Option<NotFoundReason> },  // 增加 reason
    Error(String),
}
```

#### 步骤 1.5：修改 DetailedValidationResult

**文件**：`src/core/model.rs`

```rust
pub struct DetailedValidationResult {
    // ...现有字段...
    pub not_found_reason: Option<NotFoundReason>,  // 新增
}
```

---

### 阶段二：捕获流程改造（P0 + P1）

**目标**：Normal 捕获增加轻量 BFS + 捕获时自动生成 LocateMode + SearchContext

#### 步骤 2.1：Normal 捕获增加轻量 BFS（REQ-C-1）

**文件**：`src/core/uia/capture.rs`

改造 `capture_at_point`：

```
当前流程：
  ElementFromPoint(x, y) → hit_elem → build_ancestor_chain(hit_elem)

改进流程：
  ElementFromPoint(x, y) → hit_elem
  → light_bfs_to_leaf(hit_elem, x, y, max_depth=5) → leaf_elem
  → build_ancestor_chain(leaf_elem)

新增函数：
  fn light_bfs_to_leaf(
      elem: &UIElement,
      x: i32, y: i32,
      max_depth: u32,
  ) -> Result<UIElement>
  
  BFS 策略：
  - Walker: ControlViewWalker
  - 最大深度: 5
  - 选择: 面积最小的包含 (x,y) 的子元素
  - 超时: 100ms
  - 无 Cross-HWND、无自身进程过滤
```

#### 步骤 2.2：捕获时自动决定 LocateMode + 生成 SearchContext（REQ-C-2）

**文件**：`src/core/uia/capture.rs`

改造 `capture_at_point` 和 `capture_enhanced_at_point`：

```
capture_at_point 改造：
  1. 现有逻辑获取 hit_elem (含 light_bfs_to_leaf)
  2. 检测是否跨进程（hit_elem PID vs 窗口 PID）
  3. if 跨进程 {
       locate_mode = LocateMode::FastChild;
       截断层级链;
       记录 child_hwnd_hint;
     } else {
       locate_mode = LocateMode::Fast;
     }
  4. 构建 SearchContext { locate_mode, child_hwnd_hint, search_root }
  5. CaptureResult { capture_mode: Normal, locate_mode, search_context, ... }

capture_enhanced_at_point 改造：
  1. 现有 BFS + Cross-HWND 逻辑
  2. if Cross-HWND 改变了 hit_elem {
       locate_mode = LocateMode::FullChild;
       记录 child_hwnd_hint;
     } else {
       locate_mode = LocateMode::Full;
     }
  3. 构建 SearchContext
  4. CaptureResult { capture_mode: Enhanced, locate_mode, search_context, ... }
```

#### 步骤 2.3：提取跨进程检测公共函数

**文件**：`src/core/uia/capture.rs`（或新建 `src/core/uia/cross_hwnd.rs`）

```rust
/// 检测元素是否跨进程，并记录子 HWND 信息
fn detect_cross_process(
    hit_elem: &UIElement,
    window_pid: u32,
) -> Option<CrossProcessInfo>

pub struct CrossProcessInfo {
    pub child_hwnd_class: String,
    pub child_hwnd_title: String,
    pub child_hwnd: HWND,
}
```

---

### 阶段三：定位/校验流程改造（P0 + P1）

**目标**：定位/校验使用 SearchContext 显式决定策略 + Child 精确匹配 + SearchStrategy

#### 步骤 3.1：一次定位接受 SearchContext（REQ-L1-1）

**文件**：`src/core/uia/find.rs`

改造 `find_all_elements_detailed`：

```
当前签名：
  fn find_all_elements_detailed(selector, xpath) -> Result<Vec<ElementData>>

改进签名：
  fn find_all_elements_detailed(
      selector: &str,
      xpath: &str,
      search_context: Option<&SearchContext>,  // 新增
  ) -> Result<Vec<ElementData>>

逻辑变更：
  if let Some(ctx) = search_context {
      match ctx.locate_mode {
          Fast => ControlViewWalker 搜索
          FastChild => 枚举子 HWND + ControlViewWalker 搜索
          Full => RawViewWalker 搜索
          FullChild => 枚举子 HWND + RawViewWalker 搜索
      }
  } else {
      // 兼容：解析 xpath 前缀（当前行为）
  }
```

#### 步骤 3.2：Child 模式精确匹配子 HWND（REQ-C-3, REQ-L1-2）

**文件**：`src/core/uia/find.rs`（子 HWND 枚举逻辑）

```
当前：enum_child_hwnds(hwnd) → 对每个子 HWND 都搜索

改进：
  if let Some(hint) = &search_context.child_hwnd_hint {
      enum_child_hwnds(hwnd)
          .filter(|child| class_matches(child, &hint.hwnd_class)
                       && title_matches(child, &hint.hwnd_title))
          .collect()
  } else {
      // 兼容：搜索所有子 HWND
  }
```

#### 步骤 3.3：二次定位接受 SearchStrategy（REQ-L2-1, REQ-L2-2）

**文件**：`src/core/uia/find.rs`

改造 `find_from_element_cached`：

```
当前签名：
  fn find_from_element_cached(runtime_id, xpath) -> Result<Vec<ElementData>>

改进签名：
  fn find_from_element_cached(
      runtime_id: &str,
      xpath: &str,
      strategy: Option<SearchStrategy>,  // 新增
  ) -> Result<Vec<ElementData>>

逻辑变更：
  let base_elem = element_cache::get(runtime_id)?;
  match strategy.unwrap_or(SearchStrategy::Adaptive) {
      Fast { max_depth } => find_by_xpath_with_control_view(base_elem, xpath, max_depth),
      Full { max_depth } => find_by_xpath_with_raw_view(base_elem, xpath, max_depth),
      Adaptive => find_by_xpath_with_fallback(base_elem, xpath),  // 当前默认行为
  }
```

#### 步骤 3.4：校验使用 SearchContext + 返回 NotFoundReason（REQ-V-1）

**文件**：`src/core/uia/validation.rs`

改造 `validate_selector_and_xpath_detailed`：

```
当前签名：
  fn validate_selector_and_xpath_detailed(selector, xpath) -> DetailedValidationResult

改进签名：
  fn validate_selector_and_xpath_detailed(
      selector: &str,
      xpath: &str,
      search_context: Option<&SearchContext>,  // 新增
  ) -> DetailedValidationResult

逻辑变更：
  1. 窗口匹配失败 → NotFoundReason::WindowNotFound
  2. 子 HWND 未找到 → NotFoundReason::ChildHwndNotFound { class }
  3. XPath 步骤失败 → NotFoundReason::XPathStepFailed { step, detail }
  4. 搜索超时 → NotFoundReason::Timeout { budget_ms, elapsed_ms }
  5. 其他 → NotFoundReason::ElementGone
```

---

### 阶段四：API 层改造（P0）

**目标**：API 端点支持新增参数

#### 步骤 4.1：FindFromElementRequest 增加 searchStrategy

**文件**：`src/api/types.rs`

```rust
pub struct FindFromElementRequest {
    pub runtime_id: String,
    pub xpath: String,
    pub search_strategy: Option<SearchStrategy>,  // 新增
}
```

#### 步骤 4.2：ElementQuery 增加 searchContext（用于一次定位）

**文件**：`src/api/types.rs`

```rust
pub struct ElementQuery {
    // ...现有字段...
    pub search_context: Option<SearchContext>,  // 新增
}
```

#### 步骤 4.3：API 端点更新

**文件**：`src/api/element.rs`

- `find_all_elements`: 传递 `search_context` 给 `find_all_elements_detailed`
- `find_from_element`: 传递 `search_strategy` 给 `find_from_element_cached`
- 校验端点：传递 `search_context` 给 `validate_selector_and_xpath_detailed`

---

### 阶段五：GUI 层改造（P0）

**目标**：GUI 适配新的数据模型

#### 步骤 5.1：GUI 捕获调用适配

**文件**：`src/gui/iced_app.rs`

- 捕获结果现在包含 `locate_mode` + `search_context`
- 存储 `search_context` 以供后续定位/校验使用

#### 步骤 5.2：GUI 定位/校验调用适配

**文件**：`src/gui/iced_app.rs`

- 定位/校验时传入存储的 `search_context`
- 校验结果展示 `NotFoundReason`（如果有）

---

### 阶段六：BuildCache 优化（P1）

**目标**：在属性读取热点使用 BuildCache 批量预取

#### 步骤 6.1：捕获层级链 BuildCache

**文件**：`src/core/uia/capture.rs`

- `build_ancestor_chain` 中的属性读取改用 `build_updated_cache(Element)`
- 预估加速：5-10x

#### 步骤 6.2：BFS 深搜 BuildCache

**文件**：`src/core/uia/capture.rs`

- BFS 每层遍历改用 `build_updated_cache(Children)`
- 预估加速：3-5x

#### 步骤 6.3：校验属性比对 BuildCache

**文件**：`src/core/uia/validation.rs`

- 属性比对改用 `build_updated_cache(Element)`
- 预估加速：3-5x

#### 步骤 6.4：窗口匹配 BuildCache

**文件**：`src/core/uia/window.rs`

- 窗口属性读取改用 `build_updated_cache(Element)`
- 预估加速：2-3x

---

## 执行顺序与依赖关系

```
阶段一（数据模型）─┐
                   ├→ 阶段二（捕获流程）─┐
                   │                     ├→ 阶段四（API层）─→ 阶段五（GUI层）
                   ├→ 阶段三（定位/校验）┘
                   │
                   └→ 阶段六（BuildCache）← 独立，可在阶段二~五完成后进行
```

**建议执行顺序**：
1. **阶段一** → 2. **阶段二** → 3. **阶段三** → 4. **阶段四** → 5. **阶段五** → 6. **阶段六**

阶段一必须先完成，因为后续所有阶段都依赖新的数据模型。阶段二和阶段三可以并行开发（但共享 model.rs，建议串行）。阶段六独立，最后做。

---

## 兼容性策略

### 向后兼容

- 所有新参数使用 `Option<>`，`None` 时回退到当前行为
- `SearchContext = None` → 解析 XPath 前缀决定策略（当前行为）
- `SearchStrategy = None` → Adaptive（当前默认行为）
- XPath 前缀 `[fast]`/`[fast-child]`/`[full]`/`[full-child]` 保持不变

### 迁移路径

1. 阶段一完成 → 编译通过但行为不变（新字段全部默认值）
2. 阶段二完成 → 捕获结果包含新字段，但定位/校验仍走旧路径
3. 阶段三完成 → 定位/校验可以使用新字段，旧路径作为 fallback
4. 阶段四~五完成 → 全链路打通
5. 阶段六完成 → 性能优化

---

## 风险与注意事项

| 风险 | 应对 |
|------|------|
| CaptureMode 从 4 值改为 2 值，影响面大 | 全局搜索 `CaptureMode::Fast` 等引用，逐一替换 |
| `strip_xpath_prefix` 返回类型从 `CaptureMode` 改为 `LocateMode` | 所有调用点需更新 |
| GUI 层需要存储和传递 `SearchContext` | 可能需要修改 GUI 的状态管理 |
| BuildCache 需要 uiautomation-rs 支持且行为验证 | 先写不使用 BuildCache 的版本，验证通过后再替换 |
| 轻量 BFS 可能影响 Normal 捕获性能 | BFS 限制 5 层 + 100ms 超时，影响可控 |

---

## 验收检查清单

### 阶段一验收
- [ ] `CaptureMode` 只有 Normal/Enhanced 两个值
- [ ] `LocateMode` 有 Fast/FastChild/Full/FullChild 四个值
- [ ] `SearchContext` / `SearchStrategy` / `NotFoundReason` 类型已定义
- [ ] `CaptureResult` 包含 `locate_mode` + `search_context`
- [ ] `cargo build` 编译通过

### 阶段二验收
- [ ] Normal 捕获的 `is_target` 是叶子类型而非 Group/Pane
- [ ] 捕获结果包含正确的 `locate_mode`
- [ ] 跨进程场景下 `locate_mode` = FastChild/FullChild
- [ ] `child_hwnd_hint` 正确记录命中的子 HWND 信息

### 阶段三验收
- [ ] `find_all_elements_detailed` 接受 `SearchContext` 参数
- [ ] `find_from_element_cached` 接受 `SearchStrategy` 参数
- [ ] Child 模式只搜索匹配的子 HWND
- [ ] 校验结果包含 `NotFoundReason`

### 阶段四验收
- [ ] API 端点接受新参数
- [ ] `searchContext=None` 时行为与当前一致

### 阶段五验收
- [ ] GUI 捕获/定位/校验全链路打通
- [ ] 校验失败时展示具体原因

### 阶段六验收
- [ ] 捕获层级链构建使用 BuildCache
- [ ] BFS 深搜使用 BuildCache
- [ ] 校验属性比对使用 BuildCache
- [ ] 性能基准测试通过
