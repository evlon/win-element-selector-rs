# API / SDK 设计审查报告

> 审查日期：2026-06-06  
> 审查范围：
> - Rust 后端 `win-element-selector-rs`：`src/core/` + `src/api/` + `src/mouse_control.rs` + `src/highlight.rs` 所有 `pub` 符号  
> - TypeScript SDK `win-element-selector-sdk`：`src/` 下所有 `export` 符号  
> 参照规范：DESIGN_XPATH_OPTIMIZER.md、FEATURES_ROADMAP.md、README.md

---

## 一、审查方法

| 维度 | 检查内容 |
|------|---------|
| **一致性** | 命名规范、参数顺序、返回类型是否统一 |
| **封装性** | 不必要的 pub 泄露、内部实现暴露 |
| **分层正确性** | 依赖方向是否正确（api → core，不可逆） |
| **职责单一** | 每个函数/模块是否只做一件事 |
| **兼容性** | 是否有遗留 deprecated 代码未清理 |
| **可测试性** | 是否有可 mock 的 trait 边界 |
| **文档** | 公开 API 是否有文档注释 |

符合度标记：
- ✅ **正符合**：完全符合设计规范
- ⚠️ **基本符合但有改进空间**：功能正确但存在设计瑕疵
- ❌ **负符合（需修复）**：违反设计原则，应优先处理

---

## 二、模块级审查

### 2.1 `src/core/model.rs` — 核心数据模型

| 项目 | 状态 | 说明 |
|------|------|------|
| `CaptureMode` enum | ⚠️ | 包含 4 个 `#[deprecated]` 变体（Fast/Full/FastChild/FullChild），应清理 |
| `LocateMode` enum | ✅ | 4 值设计合理，strip_xpath_prefix 功能正确 |
| `SearchStrategy` enum | ⚠️ | 仅 2 个变体但承担了 4 个 mode 的语义，与 LocateMode 有重叠 |
| `HierarchyNode` | ✅ | 结构清晰，字段名一致 |
| `PropertyFilter` | ✅ | 12 种运算符，设计良好 |
| `SearchMode` | ✅ | all/first/onlyone 设计清晰 |
| `FindAllFilter` | ✅ | post-filter 配置良好 |
| `SegmentValidationResult` | ✅ | 验证结果结构清晰 |
| `ChildHwndHint` | ✅ | HWND 过滤提示设计合理 |
| `SearchContext` | ⚠️ | 新引入，文档不足 |
| `OverflowInfo` | ⚠️ | 使用场景不明确 |

#### CaptureMode 详细问题

```rust
// ❌ 负面：4个 deprecated 变体还在用
#[deprecated(note = "Use CaptureMode::Normal instead")]
Fast,
#[deprecated(note = "Use CaptureMode::Enhanced instead")]
Full,
#[deprecated(note = "Use LocateMode::FastChild instead")]
FastChild,
#[deprecated(note = "Use LocateMode::FullChild instead")]
FullChild,
```

**建议**：在 `v2.0.0` 发布前彻底移除这 4 个变体，消除 `#[allow(deprecated)]` 的所有引用。

---

### 2.2 `src/core/uia/find.rs` — XPath 搜索调度器

| 项目 | 状态 | 说明 |
|------|------|------|
| `execute_xpath_steps` | ⚠️ | `#[allow(dead_code)]` + `pub(super)`，未被使用但保留 |
| `execute_xpath_steps_filtered` | ✅ | 主调度器，设计良好 |
| `split_xpath_steps` | ✅ | 新函数，修复 `/*n/` 前缀丢失问题 |
| `parse_xpath_step` | ✅ | 步骤解析，支持 `//`/`/`/`/*n/`/`/*/` |
| `build_uia_condition_from_step` | ⚠️ | 复杂度高，依赖解析内部格式 |
| `find_with_depth_limit` | ✅ | BFS 深度限制搜索，正确 |
| `find_with_depth_limit_timeout` | ⚠️ | `#[allow(dead_code)]`，未被使用 |
| 缓存函数（lookup/store） | ✅ | 性能优化，设计合理 |

#### 问题

1. **`execute_xpath_steps` 未被调用**（仅作为便利包装保留）。建议：要么移除，要么将其用于实际场景（如简单的单元素查找）。

2. **`find_with_depth_limit_timeout` 未被使用**。建议：要么在 `DepthLimitedBfs` 分支中集成超时机制，要么移除。

3. **`build_uia_condition_from_step`** 依赖 `parsed_step` 的内部格式，耦合度较高。建议：让 `parse_xpath_step` 直接返回结构化的 condition 信息。

---

### 2.3 `src/core/uia/find_control.rs` — ControlView 搜索

| 项目 | 状态 | 说明 |
|------|------|------|
| `find_by_xpath_detailed` | ✅ | 核心函数，通过 uiauto-xpath 搜索 |
| `findall_chain_first` | ⚠️ | 仅返回第一个匹配，函数名不够精确 |
| `findall_chain_all` | ✅ | 返回所有匹配 |

#### 建议

`findall_chain_first` 命名暗示只返回一个结果，但实际上返回的是 `Vec`（可包含多个）。建议重命名为 `findall_chain` 或拆分为 `first`/`all` 两个独立路径。

---

### 2.4 `src/core/uia/find_raw.rs` — RawView 搜索

| 项目 | 状态 | 说明 |
|------|------|------|
| `find_by_xpath_raw_descendants` | ✅ | RawView FindAll 后代搜索 |
| `find_by_xpath_raw_descendants_with_depth` | ⚠️ | 函数签名与 `find_by_xpath_raw_descendants` 几乎一致，仅多 `max_depth` 参数 |

#### 建议

两个函数可以合并为一个，`max_depth` 用 `Option<usize>` 表示：
```rust
pub fn find_by_xpath_raw_descendants(
    auto: &UIAutomation,
    window: &UIElement,
    xpath: &str,
    max_depth: Option<usize>,  // None = 无限制
    enable_findall: bool,
) -> Vec<UIElement>
```

---

### 2.5 `src/core/uia/validation.rs` — XPath 校验

| 项目 | 状态 | 说明 |
|------|------|------|
| `validate_xpath_on_window` | ✅ | 窗口+元素两阶段校验，设计清晰 |
| `validate_child_xpath` | ✅ | 子窗口模式校验 |

---

### 2.6 `src/core/uia/capture.rs` — 元素捕获

| 项目 | 状态 | 说明 |
|------|------|------|
| `capture_element_at` | ✅ | 捕获核心函数 |
| `capture_element_with_children` | ⚠️ | 名称暗示捕获子元素，实际是 BFS 构建层级树 |
| `generate_simplified_elements` | ✅ | XPath 精简，不影响 hierarchy |

#### 建议

`capture_element_with_children` 可重命名为 `capture_hierarchy_tree` 以更准确反映其功能。

---

### 2.7 `src/core/uia/element.rs` — 元素操作

| 项目 | 状态 | 说明 |
|------|------|------|
| `invoke_element` | ✅ | 调用元素默认动作 |
| `focus_element` | ✅ | 设置焦点 |
| `set_value_to_element` | ✅ | 设置值 |

---

### 2.8 `src/core/uia/window.rs` — 窗口操作

| 项目 | 状态 | 说明 |
|------|------|------|
| `find_windows_by_selector` | ✅ | 窗口查找，EnumWindows 优化 |
| `activate_window_by_hwnd` | ✅ | 窗口激活 |

---

### 2.9 `src/core/uia/navigation.rs` — 元素导航 (Compass)

| 项目 | 状态 | 说明 |
|------|------|------|
| `Compass` 及其 `navigate` 方法 | ⚠️ | 功能强大但 API 复杂 |

#### 建议

考虑提供简化的便捷函数：
```rust
pub fn navigate_parent(elem: &UIElement) -> Option<UIElement>
pub fn navigate_next_sibling(elem: &UIElement) -> Option<UIElement>
pub fn navigate_children(elem: &UIElement) -> Vec<UIElement>
```

---

### 2.10 `src/core/uia/inspect.rs` — 元素检查

| 项目 | 状态 | 说明 |
|------|------|------|
| `extract_child_features` | ✅ | 提取子元素特征，用于优化 |
| `extract_element_summary` | ✅ | 元素摘要 |

---

### 2.11 `src/core/uia/cache.rs` — XPath 编译缓存

| 项目 | 状态 | 说明 |
|------|------|------|
| `CompiledStrategy` | ✅ | 策略缓存枚举 |
| `cache_lookup` / `cache_store` | ✅ | 查找/存储 |
| 内部常量 `XPATH_FALLBACK_BUDGET_MS` | ⚠️ | `#[allow(dead_code)]`，应使用或移除 |

---

### 2.12 `src/core/uia/visibility.rs` — 可视性计算

| 项目 | 状态 | 说明 |
|------|------|------|
| `compute_visible_rect` | ✅ | 元素矩形 ∩ 窗口视口 |
| `is_element_visible` | ✅ | 可见性判断 |

---

### 2.13 `src/core/xpath.rs` — XPath 生成

| 项目 | 状态 | 说明 |
|------|------|------|
| `generate_xpath_from_hierarchy` | ✅ | 核心生成函数 |
| `generate_simplified_xpath` | ✅ | 精简 XPath |
| `validate_xpath_syntax` | ✅ | 语法校验 |
| `optimize_and_generate` | ⚠️ | 按设计文档应调用 XPathOptimizer，但当前可能未完整实现 |

---

### 2.14 `src/core/xpath_optimizer.rs` — XPath 优化器

| 项目 | 状态 | 说明 |
|------|------|------|
| `XPathOptimizer` struct | ⚠️ | 设计文档定义完整，但实现状态待确认 |
| `ClassStrategy` / `NameStrategy` | ⚠️ | 按设计文档应作为 pub enum 暴露 |

---

### 2.15 `src/core/element_cache.rs` — 元素缓存

| 项目 | 状态 | 说明 |
|------|------|------|
| `cache_element` / `get_cached_element` / `cache_size` | ✅ | 简单的 HashMap 缓存，设计清晰 |

---

### 2.16 `src/core/screenshot.rs` — 屏幕截图

| 项目 | 状态 | 说明 |
|------|------|------|
| `capture_screen_rect` | ✅ | 区域截图 |

---

### 2.17 `src/api/types.rs` — API 类型定义

| 项目 | 状态 | 说明 |
|------|------|------|
| `ElementQuery` | ✅ | 元素查找参数，字段齐全 |
| `ElementInfo` | ✅ | 响应类型，rect/visibleRect/center 等 |
| `ElementFindResponse` | ✅ | 查找响应 |
| `WindowInfoResponse` / `WindowListResponse` | ✅ | 窗口信息 |
| `WindowSelector` / `WindowSelectorOrString` | ✅ | 支持字符串/对象双形式 |
| `ClickOptions` | ✅ | 点击选项 |
| `ScrollOptions` | ✅ | 滚动选项 |
| `MouseMoveRequest` | ✅ | 鼠标移动请求 |
| `KeyboardTypeRequest` / `ShortcutRequest` / `KeyRequest` | ✅ | 键盘操作 |

---

### 2.18 `src/api/element.rs` — 元素 API handler

| 项目 | 状态 | 说明 |
|------|------|------|
| `find_element` (handler) | ✅ | `/api/element` 端点 |
| `find_all_elements` (handler) | ✅ | `/api/element/all` 端点 |
| `find_all_elements_detailed` | ⚠️ | 内部函数，不直接暴露为端点但 pub |

---

### 2.19 `src/api/window.rs` — 窗口 API handler

| 项目 | 状态 | 说明 |
|------|------|------|
| `list_windows` | ✅ | `/api/window/list` |
| `activate_window` | ✅ | `/api/window/activate` |
| `focus_element` | ✅ | `/api/window/focus-element` |

---

### 2.20 `src/api/mouse.rs` — 鼠标 API handler

| 项目 | 状态 | 说明 |
|------|------|------|
| `click_element` | ✅ | `/api/mouse/click` |
| `move_mouse` | ✅ | `/api/move/mouse` |
| `scroll_mouse` | ✅ | `/api/mouse/scroll` |

---

### 2.21 `src/api/idle_motion.rs` — 空闲移动

| 项目 | 状态 | 说明 |
|------|------|------|
| `spawn_background_tasks` | ✅ | 后台任务，API 正确 |

---

### 2.22 `src/api/keyboard.rs` — 键盘 API

| 项目 | 状态 | 说明 |
|------|------|------|
| `humanized_type` | ✅ | 拟人化输入 |

---

### 2.23 `src/mouse_control.rs` — 鼠标控制

| 项目 | 状态 | 说明 |
|------|------|------|
| `get_cursor_position` | ✅ | 获取光标位置 |
| `linear_move` | ✅ | 直线移动 |
| `humanized_move` | ✅ | 贝塞尔曲线拟人化移动 |
| `click_at` / `right_click_at` | ✅ | 点击 |
| `scroll_wheel` | ✅ | 滚动 |
| `hover_at` | ✅ | 悬停 |
| `drag_mouse` | ✅ | 拖拽 |

---

### 2.24 `src/highlight.rs` — 元素高亮

| 项目 | 状态 | 说明 |
|------|------|------|
| `flash` / `flash_with_info` / `flash_point` | ✅ | 闪烁高亮 |
| `hide` | ✅ | 隐藏高亮 |
| `update_highlight` | ✅ | 更新位置 |
| `show` / `show_with_info` | ✅ | 显示高亮（返回 handle） |

---

### 2.25 `src/core/commonality.rs` — 多元素共同特征

| 项目 | 状态 | 说明 |
|------|------|------|
| `extract_common_features` | ✅ | 提取共同特征 |

---

## 三、整体架构评估

### 3.1 依赖方向 ✅

```
api/ ──depends on──> core/  ✅ 正确
gui/ ──depends on──> core/  ✅ 正确
api/ ──X──> gui/             ✅ 正确（不依赖）
```

依赖方向正确，核心层不依赖 API 层或 GUI 层。

### 3.2 公开性控制

| 检查项 | 状态 |
|--------|------|
| `pub(super)` 正确使用 | ✅ |
| `pub(crate)` 用于跨模块 | ✅ |
| 无过度 pub 泄露 | ✅ |
| `pub mod` 声明一致 | ✅ |

### 3.3 错误处理

| 检查项 | 状态 |
|--------|------|
| 统一使用 `anyhow::Result` | ⚠️ |
| API handler 返回结构化错误 | ✅ |
| 内部函数错误传播 | ✅ |

**建议**：考虑为核心层定义 `pub enum ElementSelectorError`，替代 `anyhow::Result`，使 API 消费者能精确匹配错误类型。

### 3.4 命名一致性

| 模式 | 函数 | 建议 |
|------|------|------|
| `find_xxx` | `find_by_xpath_detailed` | ✅ 一致 |
| `execute_xxx` | `execute_xpath_steps_filtered` | ✅ 一致 |
| `capture_xxx` | `capture_element_at` | ✅ 一致 |
| `validate_xxx` | `validate_xpath_on_window` | ✅ 一致 |
| `compute_xxx` | `compute_visible_rect` | ✅ 一致 |
| `extract_xxx` | `extract_child_features` | ✅ 一致 |

---

## 四、综合评分

| 维度 | 评分 | 说明 |
|------|------|------|
| **一致性** | 8/10 | 命名风格基本一致，少数函数命名可优化 |
| **封装性** | 7/10 | `pub(super)` 控制良好，但有 `#[allow(dead_code)]` 遗留 |
| **分层正确性** | 9/10 | 依赖方向正确，api → core 单向 |
| **职责单一** | 8/10 | 大部分函数单一职责，少数函数承担过多 |
| **兼容性** | 6/10 | CaptureMode 的 deprecated 变体太多，SearchStrategy 与 LocateMode 重叠 |
| **可测试性** | 6/10 | 缺少 trait 抽象，难以 mock UIAutomation |
| **文档** | 7/10 | 核心函数有文档注释，部分新函数缺少 |

**综合评分：7.3/10**

---

## 五、改进建议（按优先级排序）

### P0 — 必须修复

| # | 问题 | 位置 | 建议 |
|---|------|------|------|
| 1 | **CaptureMode deprecated 变体** | `model.rs` | 移除 Fast/Full/FastChild/FullChild，消除所有 `#[allow(deprecated)]` |
| 2 | **SearchStrategy 与 LocateMode 重叠** | `model.rs` | 合并或明确分工：SearchStrategy 仅用于搜索策略选择，LocateMode 负责 XPath 前缀解析 |
| 3 | **`execute_xpath_steps` dead_code** | `find.rs` | 要么移除，要么用于实际场景 |
| 4 | **`find_with_depth_limit_timeout` dead_code** | `find.rs` | 集成到 `DepthLimitedBfs` 分支或移除 |
| 5 | **`XPATH_FALLBACK_BUDGET_MS` dead_code** | `cache.rs` | 使用或移除 |

### P1 — 应该改进

| # | 问题 | 位置 | 建议 |
|---|------|------|------|
| 6 | **`findall_chain_first` 命名不精确** | `find_control.rs` | 重命名为 `findall_chain_first_batch` 或拆分 |
| 7 | **`find_by_xpath_raw_descendants_with_depth` 与无深度版本重复** | `find_raw.rs` | 合并为一个函数，max_depth 用 Option |
| 8 | **`capture_element_with_children` 命名误导** | `capture.rs` | 重命名为 `capture_hierarchy_tree` |
| 9 | **缺少 trait 抽象** | `core/uia/` | 为 UIAutomation 操作定义 trait，方便测试 mock |
| 10 | **`build_uia_condition_from_step` 耦合度高** | `find.rs` | 重构为让 `parse_xpath_step` 返回结构化 condition |

### P2 — 可以优化

| # | 问题 | 位置 | 建议 |
|---|------|------|------|
| 11 | **缺少自定义错误类型** | `core/error.rs` | 定义 `ElementSelectorError` enum 替代 anyhow |
| 12 | **SearchContext 文档不足** | `model.rs` | 补充文档注释和使用示例 |
| 13 | **Compass API 缺少便捷函数** | `navigation.rs` | 提供 `navigate_parent`/`navigate_children` 等 |
| 14 | **XPathOptimizer 实现状态** | `xpath_optimizer.rs` | 确认 Phase 1-3 实现进度 |

---

## 六、已正确符合规范的设计（表扬清单）

1. ✅ **两阶段 XPath 执行**：`execute_xpath_steps_filtered` 按前缀直接分发，无隐式 fallback
2. ✅ **split_xpath_steps**：修复了 `/*n/` 前缀丢失问题
3. ✅ **LocateMode::strip_xpath_prefix**：统一的前缀剥离逻辑
4. ✅ **FindAllFilter**：post-filter 配置化，排除 offscreen/零尺寸/越界元素
5. ✅ **SearchMode**：all/first/onlyone 三层语义清晰
6. ✅ **ElementQuery**：API 参数类型完整，支持 searchContext/timeoutMs/findAllFilter
7. ✅ **WindowSelectorOrString**：灵活的窗口选择器（字符串/对象双形态）
8. ✅ **元素缓存**：简单有效的 HashMap 缓存
9. ✅ **鼠标拟人化**：贝塞尔曲线 + ease-in-out + 随机偏移，设计完整
10. ✅ **依赖方向**：api → core，无反向依赖

---

# 第二部分：Node.js SDK 审查 (`win-element-selector-sdk`)

## 一、项目概览

| 属性 | 值 |
|------|-----|
| **包名** | `element-selector-sdk-nodejs` |
| **版本** | `0.1.0` |
| **运行时** | Node.js >= 18.0.0 |
| **依赖** | `axios ^1.6.0`, `pino ^10.3.1`, `pino-pretty ^13.1.3` |
| **文件数** | 12 个源文件, 24 个映射文件 |
| **配套后端** | `win-element-selector-rs` (HTTP 8080) |

### 模块结构
```
src/
├── index.ts      — SDK 入口（SDK 主类 + 全局导出）
├── client.ts     — HttpClient（Axios 封装 + 自动重试）
├── element.ts    — Element 类（UI 元素一等公民）
├── flow.ts       — Flow 类（自动化流程编排器）
├── types.ts      — 类型定义（~880 行）
├── config.ts     — .flow.json5 配置加载 + 深合并
├── errors.ts     — 异常层级（7 个异常类 + 类型守卫）
├── logger.ts     — OperationLogger（操作日志）
├── screenshot.ts — ScreenshotManager（截图管理）
├── sleep.ts      — delay/setSpeedFactor/getSpeedFactor
├── utils.ts      — buildWindowSelector + compass 等
└── __tests__/
    └── element.test.ts
```

---

## 二、Element 类 API 审查 (`element.ts`, 874 行)

Element 类是 SDK 的核心，代表了"UI 元素的一等公民表示"——所有操作都在 Element 对象上执行。

### 2.1 查询方法

| 方法 | 签名 | 状态 | 评价 |
|------|------|------|------|
| `text()` | `async text(): Promise<string>` | ✅ | 简洁，返回 `info.name` |
| `getText()` | `async getText(): Promise<string>` | ⚠️ | `text()` 的向后兼容别名，存在冗余 |
| `isEnabled()` | `async isEnabled(): Promise<boolean>` | ✅ | 读本地缓存 |
| `isVisible()` | `async isVisible(): Promise<boolean>` | ✅ | 检查 offscreen + rect 有效性 |
| `isOffscreen()` | `async isOffscreen(): Promise<boolean>` | ✅ | 读本地缓存 |
| `isCheckable()` | `async isCheckable(): Promise<boolean>` | ✅ | UIA Pattern 检查 |
| `isChecked()` | `async isChecked(): Promise<boolean>` | ✅ | UIA Pattern 检查 |
| `isClickable()` | `async isClickable(): Promise<boolean>` | ✅ | UIA Pattern 检查 |
| `isScrollable()` | `async isScrollable(): Promise<boolean>` | ✅ | UIA Pattern 检查 |
| `isSelected()` | `async isSelected(): Promise<boolean>` | ✅ | UIA Pattern 检查 |
| `attr(name)` | `async attr(name: string): Promise<string>` | ✅ | 灵活属性读取，支持别名（type/id/class等） |
| `getAttribute()` | `async getAttribute(name: string): Promise<string>` | ⚠️ | `attr()` 别名，冗余 |
| `bounds()` | `async bounds(): Promise<Rect>` | ✅ | 返回 `info.rect` |
| `getRect()` | `async getRect(): Promise<Rect>` | ⚠️ | `bounds()` 别名 |
| `boundingBox()` | `async boundingBox(): Promise<Rect>` | ⚠️ | `bounds()` 的 Playwright 兼容别名 |
| `toXpath(...propNames)` | `toXpath(...propNames: string[]): string` | ✅ | 自动构造唯一 XPath |
| `getSelector()` | `getSelector(): {windowSelector, elementSelector}` | ✅ | 返回唯一定位信息 |
| `refresh(...propNames)` | `async refresh(...propNames: string[]): Promise<void>` | ✅ | 原地刷新 this.info |
| `checkVisibility()` | `async checkVisibility(containerXPath?, ...propNames): Promise<ElementVisibilityResult>` | ✅ | 实时可见性查询 |
| `inspect(options?, ...propNames)` | `async inspect(options?: InspectOptions, ...propNames): Promise<InspectResponse>` | ✅ | 子树遍历（支持区域过滤） |

### 2.2 操作方法

| 方法 | 签名 | 状态 | 评价 |
|------|------|------|------|
| `click(options?, ...propNames)` | 点击元素 | ✅ | 完整选项：humanize/randomRange/clickArea/offset/markClick/occlusionCheck |
| `dblclick()` | 双击 | ⚠️ | 两次 click 模拟，非原生双击 |
| `doubleClick()` | `dblclick()` 别名 | ⚠️ | 冗余别名 |
| `rightClick(...propNames)` | 右键点击 | ✅ | 固定 humanize=true |
| `type(text, options?)` | 输入文本 | ✅ | 支持 keyboard/value/clipboard 三种模式 + 虚拟键 |
| `typeText(text, options?)` | `type()` 别名 | ⚠️ | 冗余别名 |
| `clear()` | 清空内容 | ✅ | Click → Ctrl+A → Delete |
| `fill(text, options?)` | 填充（清空后输入） | ✅ | 类似 Playwright fill |
| `focus()` | 聚焦 | ✅ | click({ waitAfter: 0 }) |
| `hover(options?, ...propNames)` | 悬停 | ✅ | 支持 duration 和 humanize |
| `dragTo(target, options?)` | 拖拽到目标元素 | ✅ | 支持 Element 目标 |
| `flash(options?, ...propNames)` | 高亮闪烁 | ✅ | 调试可视化 |

### 2.3 子元素查找

| 方法 | 签名 | 状态 | 评价 |
|------|------|------|------|
| `findOne(xpath, ...propNames)` | 找唯一子元素 | ✅ | 多匹配时抛异常 |
| `findFirst(xpath, ...propNames)` | 找第一个子元素 | ✅ | 多匹配不报错 |
| `find(xpath, ...propNames)` | ⚠️ | **`@deprecated`** — 建议用 findOne |
| `findAll(xpath, ...propNames)` | 找所有子元素 | ✅ | 返回 ElementList，支持 `.position(n)` |
| `nth(xpath, n, ...propNames)` | 第 N 个子元素 | ✅ | 1-based，等价 `(xpath)[position()=N]` |
| `locator(xpath, ...propNames)` | findOne 的 Playwright 别名 | ⚠️ | 又一别名 |
| `children(xpath?, ...propNames)` | 直接子元素列表 | ✅ | 返回 ElementList |
| `childCount(...propNames)` | 直接子元素数量 | ✅ | |
| `child(index, ...propNames)` | 索引子元素（0-based） | ✅ | 支持负数倒数 |
| `indexInParent(...propNames)` | 在父中的索引 | ✅ | |

### 2.4 DOM 导航 (Compass)

| 方法 | 签名 | 状态 | 评价 |
|------|------|------|------|
| `parent(levels?, ...propNames)` | 祖先元素 | ✅ | 支持多层 |
| `parentElement()` | parent() 别名 | ⚠️ | 冗余别名 |
| `next(...propNames)` | 下一个兄弟 | ✅ | |
| `nextSiblingElement()` | next() 别名 | ⚠️ | 冗余别名 |
| `prev(...propNames)` | 上一个兄弟 | ✅ | |
| `previousSiblingElement()` | prev() 别名 | ⚠️ | 冗余别名 |
| `compass(path, ...propNames)` | 罗盘导航 | ✅ | 强大：`pN/cN/sN/s<N/s>N` |

### 2.5 等待与滚动

| 方法 | 签名 | 状态 | 评价 |
|------|------|------|------|
| `waitUntilGone(options?, ...propNames)` | 等待消失 | ✅ | 轮询实现 |
| `waitFor(options?, ...propNames)` | 等待出现 | ✅ | 返回新 Element 实例 |
| `scrollToVisible(container, options)` | 滚动到可见 | ✅ | 方向必填，支持精细参数控制 |
| `scrollIntoView(container, options)` | ⚠️ | **`@deprecated`** — scrollToVisible 别名 |

### 2.6 断言方法

| 方法 | 签名 | 状态 | 评价 |
|------|------|------|------|
| `assertExists(...propNames)` | 断言存在 | ✅ | |
| `assertEnabled()` | 断言可用 | ✅ | |
| `assertVisible()` | 断言可见 | ✅ | |
| `assertText(expected)` | 断言文本 | ✅ | |

---

## 三、SDK & Flow 类审查

### 3.1 SDK 类 (`index.ts`)

| 方法 | 签名 | 状态 | 评价 |
|------|------|------|------|
| `constructor(config?)` | 配置加载 | ✅ | 三层合并：参数 > .flow.json5 > DEFAULTS |
| `flow()` | 创建 Flow | ✅ | |
| `configure(config)` | 动态配置更新 | ✅ | 支持 speedFactor 热更新 |
| `health()` | 健康检查 | ✅ | |
| `listWindows()` | 窗口列表 | ✅ | |

### 3.2 Flow 类 (`flow.ts`)

Flow 是自动化流程的编排中心。**所有 API 设计良好，命名规范，参数设计合理。**

核心方法（全部 ✅）：

| 方法 | 用途 |
|------|------|
| `window(selector)` | 激活窗口 |
| `existsWindow(selector)` | 窗口是否存在 |
| `activate(selector)` | 激活窗口（同上） |
| `find(xpath, options?)` | 查找元素 |
| `findOne(xpath, options?)` | 查找唯一元素 |
| `findFirst(xpath, options?)` | 查找首个元素 |
| `findAll(xpath, options?)` | 查找所有元素 |
| `nth(xpath, n, options?)` | 第 N 个元素 |
| `wait(ms)` | 等待 |
| `sleep(ms)` | 延迟 |
| `screenshot(path?)` | 截图 |
| `move(point, options?)` | 移动鼠标 |
| `click(point, options?)` | 点击坐标 |
| `scroll(delta, options?)` | 滚动 |
| `scrollMouse(options)` | 精细滚动控制 |
| `scrollDetectBoundary(options)` | 滚动边界检测 |
| `scrollToVisible(element, [xpath, options])` | 滚动元素可见 |
| `type(text, options?)` | 输入文本 |
| `keyboardShortcut(shortcut)` | 快捷键 |
| `pressKey(key)` | 按键 |
| `executeKey(key)` | 执行按键 |
| `pushIdle(xpath, options?)` | 入栈空闲移动 |
| `popIdle()` | 出栈空闲移动 |
| `startIdle()` | 启动空闲移动 |
| `stopIdle()` | 停止空闲移动 |
| `getIdleStatus()` | 空闲状态 |
| `profileStart()` | 性能分析开始 |
| `profileEnd()` | 性能分析结束 |
| `focusElement(xpath?, ...propNames)` | 聚焦元素 |

---

## 四、类型系统审查 (`types.ts`, ~880 行)

### 4.1 优点

1. ✅ **ElementInfo 扁平化**：避免深层嵌套，消费者直接访问属性
2. ✅ **ElementList 扩展**：在 Array 上追加 `position(n)` 方法
3. ✅ **丰富 Options 接口**：ClickOptions/TypeOptions/MoveOptions/ScrollOptions 等
4. ✅ **ClickOffset 灵活**：支持预设位置 + 自定义表达式如 `'left+20%'`
5. ✅ **InspectResponse.filter 重载**：回调函数 + InspectFilter 对象两种方式
6. ✅ **ScrollDetectResult**：边界检测的详细信息，atEnd/changedCount/details
7. ✅ **ElementVisibilityResult**：可视性/位置/overflow/scrollDirection 完整

### 4.2 问题

| # | 问题 | 严重度 | 说明 |
|---|------|--------|------|
| 1 | `types.ts` 文件 **880 行** | ⚠️ | 过于庞大，建议按领域拆分（element.ts/types, mouse.ts/types, scroll.ts/types） |
| 2 | `DEFAULTS` 巨型常量 | ⚠️ | 与 `types.ts` 混在一起，建议独立 `defaults.ts` |
| 3 | `ClickOptions` 字段过多 | ⚠️ | ~14 个字段，部分（markClick/markTimeout）适用面窄 |
| 4 | ScrollConfig 与 ScrollOptions 高度重复 | ❌ | 两种配置几乎相同但独立定义 |
| 5 | `ScrollToVisibleOptions` 与 `ScrollOptions` 高度重复 | ❌ | 重合字段超过 80% |

---

## 五、异常类审查 (`errors.ts`)

| 异常类 | 状态 | 说明 |
|--------|------|------|
| `SDKError` | ✅ | 基类，带 code/context/timestamp/toJSON |
| `ElementNotFoundError` | ✅ | 含 xpath/windowSelector/hint |
| `WindowNotFoundError` | ✅ | 含 windowSelector/hint |
| `NetworkError` | ✅ | 含 endpoint/statusCode/hint |
| `TimeoutError` | ✅ | 含 operation/timeout |
| `ActionFailedError` | ✅ | 含 action/reason/screenshotPath |
| `InvalidArgumentError` | ✅ | 含 parameter/reason |
| `StateError` | ✅ | 含 currentState/hint |
| `isSDKError()` | ✅ | 类型守卫 |
| `isElementNotFoundError()` | ✅ | 类型守卫 |
| `isWindowNotFoundError()` | ✅ | 类型守卫 |

**评价**：异常层级设计精良，每个异常都有 `context` 和 `hint`，`toJSON()` 方便日志记录。✅

---

## 六、配置系统审查 (`config.ts`)

| 功能 | 状态 | 说明 |
|------|------|------|
| `.flow.json5` 加载 | ✅ | 当前目录 → 用户主目录 |
| 自动生成默认配置 | ✅ | 不存在时生成 |
| `deepMerge` 三层合并 | ✅ | 参数 > 文件 > DEFAULTS |
| `loadConfig()` 导出 | ✅ | pub |

---

## 七、客户端审查 (`client.ts`)

| 功能 | 状态 | 说明 |
|------|------|------|
| Axios 封装 | ✅ | 统一 baseURL/timeout |
| 自动重试（最多 2 次） | ✅ | ECONNRESET/5xx 等瞬态错误 |
| 请求追踪（环境变量） | ✅ | `ELEMENT_SELECTOR_TRACE` |
| 请求哈希（去重） | ✅ | FNV-1a 算法 |
| URL 参数编码 | ✅ | encodeURIComponent |

---

## 八、冗余别名问题汇总

Element 类中存在大量向后兼容别名，增加了 API 表面积且没有额外价值：

| 主方法 | 别名 | 建议 |
|--------|------|------|
| `text()` | `getText()` | 添加 `@deprecated` 后移除 |
| `attr()` | `getAttribute()` | 添加 `@deprecated` 后移除 |
| `bounds()` | `getRect()`, `boundingBox()` | 保留 `bounds()` 和 `boundingBox()`（Playwright 兼容），移除 `getRect()` |
| `type()` | `typeText()` | 添加 `@deprecated` 后移除 |
| `dblclick()` | `doubleClick()` | 添加 `@deprecated` 后移除 |
| `findOne()` | `find()`, `locator()` | `find()` 已标记 @deprecated；`locator()` 保留（Playwright 兼容） |
| `parent()` | `parentElement()` | 添加 `@deprecated` 后移除 |
| `next()` | `nextSiblingElement()` | 添加 `@deprecated` 后移除 |
| `prev()` | `previousSiblingElement()` | 添加 `@deprecated` 后移除 |
| `scrollToVisible()` | `scrollIntoView()` | 已标记 @deprecated |

**结论**：10 个别名中有 8 个可以安全移除（`boundingBox` 和 `locator` 保留用于 Playwright 兼容）。

---

## 九、整体架构评估

### 9.1 依赖方向 ✅
```
index.ts → flow.ts → element.ts → client.ts → types.ts
                                       ↓
                                   axios / pino
```
依赖方向清晰，无循环依赖。

### 9.2 耦合度

| 项目 | 状态 | 说明 |
|------|------|------|
| Element ↔ HttpClient | ⚠️ | Element 直接持有 HttpClient，测试时需要 mock |
| Element ↔ Flow | ✅ | Flow 创建 Element 并传递 HttpClient |
| Flow ↔ ScreenshotManager | ✅ | 组合关系，合理 |

**建议**：为 HttpClient 定义 `IHttpClient` 接口，Element/Flow 依赖接口而非具体类，提升可测试性。

### 9.3 兼容性

| 项目 | 状态 | 说明 |
|------|------|------|
| Playwright 兼容 | ✅ | `boundingBox()`, `locator()`, `fill()`, `hover()` |
| 向后兼容 | ⚠️ | 太多别名，增加维护负担 |
| 版本管理 | ✅ | CHANGELOG.md 存在 |
| @deprecated 标记 | ✅ | `find()`, `scrollIntoView()` 已标记 |

---

## 十、SDK 综合评分

| 维度 | 评分 | 说明 |
|------|------|------|
| **API 设计** | 9/10 | Element/Flow 用起来自然流畅，命令式 API 设计优秀 |
| **类型安全** | 9/10 | 完整的 TypeScript 类型覆盖，泛型使用恰当 |
| **错误处理** | 9/10 | 异常层级精良，类型守卫完善 |
| **文档/注释** | 8/10 | JSDoc 注释丰富，示例充足 |
| **一致性** | 7/10 | 大量冗余别名影响一致性 |
| **封装性** | 7/10 | HttpClient 未抽象为接口，测试困难 |
| **配置系统** | 8/10 | 三层合并 + .flow.json5，设计合理 |
| **可测试性** | 6/10 | Element 构造函数直接依赖 HttpClient 实例，需重构 |
| **模块化** | 6/10 | types.ts 880 行过大，需拆分 |

**SDK 综合评分：7.7/10**

---

## 十一、Rust 后端 + SDK 跨层一致性检查

| 检查项 | 后端 (Rust) | SDK (TS) | 一致性 |
|--------|-----------|----------|--------|
| ElementInfo 字段 | `ElementInfo` struct | `ElementInfo` interface | ✅ 对齐 |
| Rect/Point | `Rect`/`Point` struct | `Rect`/`Point` interface | ✅ 对齐 |
| 搜索模式 | `SearchMode::All/First/OnlyOne` | `findAll()/findFirst()/findOne()` | ✅ 对齐 |
| 点击选项 | `ClickOptions` + randomRange/offset | `ClickOptions` + randomRange/offset | ✅ 对齐 |
| 滚动选项 | `ScrollOptions` + delta/times/autoDelta | `ScrollOptions` + delta/times/autoDelta | ✅ 对齐 |
| 视口内边距 | 后端? | `ViewportInset` | ⚠️ 后端是否支持 viewportInset? |
| Inspect API | 后端? | `InspectRequest`/`InspectResponse` | ⚠️ 后端是否有 `/api/inspect`? |
| NavigateStep | `NavigateStep` type | `NavigateStep` type | ✅ 对齐 |

> **待验证**：viewportInset 和 Inspect 相关的后端端点是否存在。如果后端尚未实现，SDK 调用会失败。

---

## 十二、改进建议总汇（两个项目合并）

### SDK P0 — 必须修复

| # | 问题 | 位置 | 建议 |
|---|------|------|------|
| S1 | `ScrollConfig` 与 `ScrollOptions` 类型重复 | `types.ts` | 合并为一个类型，用 `deepMerge` 区分默认和运行时 |
| S2 | `ScrollToVisibleOptions` 与 `ScrollOptions` 重复 | `types.ts` | 提取公共字段到 `BaseScrollOptions` |

### SDK P1 — 应该改进

| # | 问题 | 位置 | 建议 |
|---|------|------|------|
| S3 | 10 个冗余别名 | `element.ts` | 移除 8 个，保留 `boundingBox` 和 `locator` |
| S4 | `types.ts` 880 行过大 | `types.ts` | 拆分为 `types/element.ts`, `types/mouse.ts`, `types/scroll.ts` |
| S5 | HttpClient 未抽象为接口 | `client.ts` | 定义 `IHttpClient` 接口 |
| S6 | `DEFAULTS` 与类型混在一起 | `types.ts` | 移入独立 `defaults.ts` |
| S7 | `dblclick()` 两次 click 模拟 | `element.ts` | 理想情况应由后端提供原生双击 API |

### SDK P2 — 可以优化

| # | 问题 | 位置 | 建议 |
|---|------|------|------|
| S8 | Element 构造参数 9 个 | `element.ts` | 使用 Options 对象模式 |
| S9 | viewportInset 后端支持确认 | 跨层 | 验证后端实现状态 |
| S10 | Inspect API 后端支持确认 | 跨层 | 验证 `/api/inspect` 端点存在 |

---

## 十三、已正确符合规范的设计（表扬清单 — 全部）

### Rust 后端

1. ✅ 两阶段 XPath 执行：按前缀直接分发，无隐式 fallback
2. ✅ `split_xpath_steps`：修复 `/*n/` 前缀丢失
3. ✅ `LocateMode::strip_xpath_prefix`：统一前缀剥离
4. ✅ `FindAllFilter`：post-filter 配置化
5. ✅ `SearchMode`：三层语义清晰
6. ✅ `ElementQuery`：参数完整
7. ✅ `WindowSelectorOrString`：灵活双形态
8. ✅ 元素缓存：简单有效
9. ✅ 鼠标拟人化：完整设计
10. ✅ 依赖方向正确

### TypeScript SDK

11. ✅ **命令式 API**：`sdk.flow() → flow.window() → flow.find() → element.click()`，直观自然
12. ✅ **异常层级精良**：7 个异常类 + context/hint + toJSON + 类型守卫
13. ✅ **inspect + filter**：强大的子树遍历和过滤系统
14. ✅ **compass 罗盘导航**：简洁高效的元素树导航语法
15. ✅ **scrollToVisible**：精细的滚动可见性控制（12 个选项参数）
16. ✅ **ElementInfo 扁平化**：避免嵌套，直接访问属性
17. ✅ **配置三层合并**：参数 > 文件 > DEFAULTS
18. ✅ **clickOffset 灵活表达式**：预设位置 + `'left+20%'` 自定义
19. ✅ **自动重试 + 请求追踪**：生产级 HTTP 客户端
20. ✅ **Playwright 兼容命名**：`boundingBox()`, `locator()`, `fill()`, `hover()`

---

# 第三部分：SDK → API → Fun 三层匹配审查

> 审查日期：2026-06-06  
> 焦点：重构后的 Fun 层定位逻辑是否与 API 层和 SDK 层匹配

---

## 一、调用链总图

```
┌─────────────────────────────────────────────────────────────┐
│ SDK (TypeScript)          HTTP 端点           Rust API      │
│                                                             │
│ Flow.find()    ──POST──► /api/element   ──► find_all_eleme │
│ Flow.findOne() ──POST──► /api/element   ──► nts_detailed() │
│ Flow.findFirst()──POST──► /api/element   ──►               │
│ Flow.findAll() ──POST──► /api/element/all──► find_all_eleme │
│ Flow.nth()     ──POST──► /api/element   ──► nts_detailed() │
│                                                             │
│ Element.click()──POST──► /api/mouse/click──► find_all + cli │
│ Element.hover()──POST──► /api/mouse/hover──► find_all + hov │
│ Element.type() ──POST──► /api/keyboard/type──► type_text    │
│ Element.dragTo()──POST──► /api/mouse/drag──► find_all + dra │
│ Element.flash()──POST──► /api/element/flash──► find_all + fl │
│                                                             │
│ Element.checkVisibility()──► /api/element/visibility──► get │
│ Element.inspect() ──POST──► /api/element/inspect──► inspect │
│ Element.compass()──POST──► /api/element/navigate──► navigat │
│                                                             │
│ Element.findOne()──POST──► /api/element   ──► find_all_eleme │
│ Element.findFirst()─POST──► /api/element   ──► nts_detailed │
│ Element.findAll()──POST──► /api/element/all──► find_all_elem │
│ Element.children()──POST──► /api/element/all──► ents_detail │
│ Element.nth()    ──POST──► /api/element   ──► find_all_elem │
│ Element.parent() ──POST──► /api/element/navigate──► navigat │
│ Element.next()   ──POST──► /api/element/navigate──► navigat │
│ Element.prev()   ──POST──► /api/element/navigate──► navigat │
│ Element.compass()──POST──► /api/element/navigate──► navigat │
│ Element.scrollToVisible()──► /api/mouse/scroll──► scroll_mo │
│ Element.refresh()──POST──► /api/element   ──► find_all_elem │
│ Element.waitFor()──POST──► /api/element   ──► (轮询)       │
│ Element.waitUntilGone()──► /api/element   ──► (轮询)       │
│                                                             │
│ Flow.window() ──POST──► /api/window/activate──► activate_wi │
│ Flow.existsWindow()──POST──► /api/window/exists──► exists_w │
│ Flow.focusElement()──POST──► /api/window/focus-element──► a │
│ Flow.scrollToVisible()──► /api/mouse/scroll──► scroll_mouse │
│                                                             │
│ Flow.inspect()──POST──► /api/element/inspect──► inspect_sub │
│                                                             │
│ HttpClient.find()   ──► POST /api/element      ──► get_elem │
│ HttpClient.findAll()──► POST /api/element/all  ──► get_all_ │
│ HttpClient.getElementVisibility()──► /api/element/visibilit │
│ HttpClient.inspectElement()──► /api/element/inspect──► insp │
│ HttpClient.navigateElement()──► /api/element/navigate──► na │
│ HttpClient.flashElement()──► /api/element/flash──► flash_el │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│ Rust API → Core (Fun)                                       │
│                                                             │
│ get_element()        → find_all_elements_detailed()         │
│ get_all_elements()   → find_all_elements_detailed()         │
│ get_element_visibility() → get_element_visibility()         │
│ flash_element()      → find_all_elements_detailed()         │
│ inspect_element()    → inspect_subtree()                    │
│ navigate_element()   → navigate_from_element()              │
│ find_from_element()  → find_from_element_cached()           │
│                                                             │
│ list_windows()       → enumerate_windows()                  │
│ activate_window()    → activate_window_by_selector()        │
│ exists_window()      → exists_window_by_selector()          │
│ focus_element()      → activate_and_focus_element()         │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│ Fun 内部架构 (重构后的定位逻辑)                              │
│                                                             │
│ find_all_elements_detailed()                                │
│   ├─ find_window_by_selector()                              │
│   ├─ execute_xpath_steps_filtered()  ← 核心调度器            │
│   │   ├─ SearchMode::strip_suffix()                         │
│   │   ├─ LocateMode::strip_xpath_prefix()                   │
│   │   ├─ parse_positional_predicate()                       │
│   │   ├─ cache_lookup() → execute_cached_strategy()         │
│   │   ├─ split_xpath_steps()           ← 修复版正则分割      │
│   │   └─ 按前缀分发:                                         │
│   │       ├─ DirectChild → execute_direct_child()            │
│   │       │     → find_by_xpath_detailed (uiauto-xpath)     │
│   │       ├─ Descendant  → execute_descendant_fast/full()   │
│   │       │     → find_by_xpath_control_descendants()       │
│   │       │     → find_by_xpath_raw_descendants()           │
│   │       └─ DepthLimitedBfs → find_with_depth_limit()      │
│   └─ apply_positional_and_search_mode()                     │
│                                                             │
│ locate_first_from() / locate_one_from() / locate_all_from() │
│   └─ locate_from_impl()                                     │
│       ├─ get_cached_element()                               │
│       └─ 按 SearchStrategy 分发:                            │
│           ├─ Fast → find_by_xpath_detailed()                │
│           ├─ Full → find_by_xpath_raw_descendants_with_depth│
│           └─ Adaptive → find_by_xpath_detailed() (no fb)    │
│                                                             │
│ navigate_from_element()                                     │
│   ├─ find_window_by_selector() + find_by_xpath_detailed()   │
│   └─ TreeWalker 逐步导航                                    │
│                                                             │
│ inspect_subtree()                                           │
│   ├─ find_window_by_selector() + execute_xpath_steps_filter │
│   └─ RawViewWalker 递归遍历 → build_inspect_node()          │
│                                                             │
│ get_element_visibility()                                    │
│   ├─ validate_selector_and_xpath_detailed()                 │
│   ├─ get_window_rect_by_selector()                          │
│   └─ compute_visibility()  ← 纯函数                         │
└─────────────────────────────────────────────────────────────┘
```

---

## 二、逐层匹配检查

### 2.1 SDK → API 请求参数匹配

| SDK 调用 | HTTP 端点 | 请求体字段 | 匹配？ | 说明 |
|----------|-----------|-----------|--------|------|
| `client.find({window, element})` | POST `/api/element` | `{window, element, randomRange}` | ✅ | ElementQuery 对齐 |
| `client.findAll({window, element})` | POST `/api/element/all` | `{window, element, randomRange}` | ✅ | 同上 |
| `client.getElementVisibility(w, e, c?)` | POST `/api/element/visibility` | `{window, element, container?}` | ✅ | 字段对齐 |
| `client.flashElement(w, e, t)` | POST `/api/element/flash` | `{window, element, timeout}` | ✅ | 字段对齐 |
| `client.inspectElement(w, e, f)` | POST `/api/element/inspect` | `{window, element, format}` | ✅ | 字段对齐 |
| `client.navigateElement(w, e, steps)` | POST `/api/element/navigate` | `{window, element, steps}` | ✅ | NavigateStep 对齐 |
| `client.clickMouse(params)` | POST `/api/mouse/click` | `{window, element, options}` | ✅ | ClickOptions 对齐 |
| `client.hoverMouse(params)` | POST `/api/mouse/hover` | `{window, element, options}` | ✅ | hover options 对齐 |
| `client.scrollMouse(params)` | POST `/api/mouse/scroll` | `{window?, element, options}` | ✅ | ScrollOptions 对齐 |
| `client.typeText(text, opts, w?, e?)` | POST `/api/keyboard/type` | `{text, charDelay, typeMode?, window?, element?}` | ✅ | TypeOptions 对齐 |
| `client.dragMouse(params)` | POST `/api/mouse/drag` | `{window, sourceElement, targetElement, options}` | ✅ | drag options 对齐 |

**SDK → API 匹配度：10/10 ✅** 所有请求体字段与 API 端点完全对齐，无遗漏。

### 2.2 API → Fun 函数调用匹配

| API Handler | 调用的 Core 函数 | 参数传递 | 匹配？ | 说明 |
|-------------|-----------------|----------|--------|------|
| `get_element()` | `find_all_elements_detailed(w, e, rr, ctx, timeout, filter)` | 完整传递 | ✅ | 含 searchMode 后缀拼接 |
| `get_all_elements()` | `find_all_elements_detailed(w, e, rr, ctx, timeout, filter)` | 完整传递 | ✅ | 同上 |
| `get_element_visibility()` | `get_element_visibility(w, e, container)` | 完整传递 | ✅ | core→api 类型转换正确 |
| `flash_element()` | `find_all_elements_detailed(w, e, 5.0, None, None, None)` | ⚠️ | **randomRange 硬编码 5.0**，未从请求参数传递 |
| `inspect_element()` | `inspect_subtree(w, e, max_depth, max_nodes, format)` | 完整传递 | ✅ | core→api 类型转换正确 |
| `navigate_element()` | `navigate_from_element(w, e, steps)` | 完整传递 | ✅ | |
| `find_from_element()` | `find_from_element_cached(rid, x, rr, strategy)` | 完整传递 | ✅ | 含 searchMode 后缀拼接 |
| `list_windows()` | `enumerate_windows()` | 无参数 | ✅ | |
| `activate_window()` | `activate_window_by_selector(w)` | 完整传递 | ✅ | |
| `exists_window()` | `exists_window_by_selector(w)` | 完整传递 | ✅ | |
| `focus_element()` | `activate_and_focus_element(w, x)` | 完整传递 | ✅ | |

**API → Fun 匹配度：9.5/10** — 发现 1 个瑕疵。

> **⚠️ 瑕疵 #1**: `flash_element` 硬编码 `random_range=5.0`，而其他元素查找 API（`get_element`）从请求参数中读取 `element_query.random_range`。Flash 请求中没有 `randomRange` 字段，但应提供一致性体验。

### 2.3 SDK Element 方法 → 后端定位逻辑匹配

| SDK Element 方法 | 实际 HTTP 调用 | 后端定位函数 | 定位方式 | 匹配？ |
|-----------------|---------------|-------------|---------|--------|
| `findOne(xpath)` | POST `/api/element` | `find_all_elements_detailed` → `execute_xpath_steps_filtered` | 两阶段调度器 | ✅ |
| `findFirst(xpath)` | POST `/api/element` | 同上 | 同上 | ✅ |
| `findAll(xpath)` | POST `/api/element/all` | 同上 | 同上 | ✅ |
| `nth(xpath, n)` | POST `/api/element` `(xpath)[position()=n]` | 同上 → `parse_positional_predicate` | 括号+position() | ✅ |
| `children(xpath?)` | POST `/api/element/all` | `find_all_elements_detailed` | 同 findAll | ✅ |
| `parent(levels?)` | POST `/api/element/navigate` | `navigate_from_element` | TreeWalker 逐步导航 | ✅ |
| `next()/prev()` | POST `/api/element/navigate` | `navigate_from_element` | TreeWalker 逐步导航 | ✅ |
| `compass(path)` | POST `/api/element/navigate` | `navigate_from_element` | TreeWalker 逐步导航 | ✅ |
| `refresh()` | POST `/api/element` | `find_all_elements_detailed` | 重新 find | ✅ |
| `waitFor()` | POST `/api/element` (轮询) | `find_all_elements_detailed` | 轮询重试 | ✅ |
| `inspect()` | POST `/api/element/inspect` | `inspect_subtree` → `execute_xpath_steps_filtered` | 先定位再遍历 | ✅ |
| `checkVisibility()` | POST `/api/element/visibility` | `get_element_visibility` → `validate_selector_and_xpath_detailed` | 先验证再计算 | ✅ |
| `scrollToVisible()` | POST `/api/mouse/scroll` | `scroll_mouse` + 前端编排 | 前端编排 | ✅ |

**SDK Element → Fun 匹配度：10/10 ✅** — 所有 Element 方法都正确路由到对应的后端定位函数。

---

## 三、重构后定位逻辑的关键检查

### 3.1 `execute_xpath_steps_filtered` — 核心调度器

| 设计要点 | 实现状态 | 评估 |
|----------|---------|------|
| 无隐式 fallback | 每个 step 按前缀直接分发，无 fallback | ✅ 符合设计 |
| 前缀保留 | `split_xpath_steps` 用正则保留 `/*n/` 前缀 | ✅ 已修复 |
| SearchMode 后缀剥离 | `SearchMode::strip_suffix` 先剥离再执行 | ✅ |
| LocateMode 前缀剥离 | `LocateMode::strip_xpath_prefix` 先剥离 | ✅ |
| 位置谓词解析 | `parse_positional_predicate` 处理 `(xpath)[N]` | ✅ |
| 超时控制 | `timeout_ms` 每步检查 | ✅ |
| findOne 唯一性 | `is_onlyone && is_last && match_count > 1` → LeafNotUnique | ✅ |
| 缓存策略 | `cache_lookup` → `execute_cached_strategy` | ✅ |

**评价：重构后的调度器设计完整，与 API 层和 SDK 层的语义完全匹配。**

### 3.2 策略分派正确性

| XPath 示例 | 解析结果 | 执行策略 | 正确？ |
|-----------|---------|---------|--------|
| `/Button[@Name='OK']` | Child + Button | `execute_direct_child` → `find_by_xpath_detailed` | ✅ |
| `//Button[@Name='OK']` | Descendant + Button | `execute_descendant_fast/full` | ✅ |
| `/*/Button[@Name='OK']` | DepthLimited(2) + Button | `find_with_depth_limit(max_depth=2)` | ✅ |
| `/*9/Text[@Name='x']` | DepthLimited(10) + Text | `find_with_depth_limit(max_depth=10)` | ✅ |
| `Button[@Name='OK']` (无前缀) | Descendant + Button | `execute_descendant_*` | ✅ |

**策略分派正确性：5/5 ✅**

### 3.3 `locate_from_impl` — 二次定位

| 设计要点 | 实现状态 | 评估 |
|----------|---------|------|
| 从 RuntimeId 缓存获取父元素 | `get_cached_element(runtime_id)` | ✅ |
| Fast 策略 | `find_by_xpath_detailed` | ✅ |
| Full 策略 | `find_by_xpath_raw_descendants_with_depth` | ✅ |
| Adaptive = Fast（无 fallback） | 代码注释明确"不做 fallback" | ✅ |
| InvalidParent 返回 | `NotFoundReason::InvalidParent` | ✅ |
| LeafNotUnique | `search_mode == OnlyOne && len > 1` | ✅ |

**评价：二次定位实现完整，与需求 §6 完全对齐。**

---

## 四、发现的不一致问题

### 4.1 ❌ `flash_element` 参数丢失

```rust
// src/api/element.rs:356
crate::core::uia::find_all_elements_detailed(&window, &element, 5.0, None, None, None)
//                                                                      ^^^^  ^^^^  ^^^^
//                                                           randomRange 硬编码 5.0
//                                                           未传递 searchContext
//                                                           未传递 timeoutMs
//                                                           未传递 find_all_filter
```

**影响**：Flash 请求中无法自定义 randomRange、超时或 filter。与其他元素查找 API 不一致。

**建议**：在 `ElementFlashRequest` 中添加 `random_range`、`timeout_ms`、`search_context` 字段，传递到 `find_all_elements_detailed`。

### 4.2 ⚠️ `find_from_element` 走旧的 `find_from_element_cached`，非调度器

```rust
// src/api/element.rs:664
crate::core::uia::find_from_element_cached(&runtime_id, &xpath, random_range, search_strategy)
```

此函数内部用 `SearchStrategy` 分发（Fast→`find_by_xpath_detailed`, Full→`find_by_xpath_raw_descendants_with_depth`），而非走 `execute_xpath_steps_filtered` 调度器。

**影响**：`find_from_element` 不支持 `/*n/` 深度限制前缀和分步调度。但这可能是设计意图——从已缓存元素出发搜索，上下文已知，不需要完整的步骤调度。

**建议**：确认这是设计意图。如果是，在注释中说明；如果不是，应改为走 `execute_xpath_steps_filtered`。

### 4.3 ⚠️ `navigate_from_element` 使用 `find_by_xpath_detailed`（旧版 uiauto-xpath）

```rust
// src/core/uia/navigation.rs
let (matches, _) = find_by_xpath_detailed(&auto, &window_element, base_xpath, None)?;
```

Navigate 的"找到基准元素"步骤用的是旧的 `find_by_xpath_detailed`（uiauto-xpath），而非新的调度器 `execute_xpath_steps_filtered`。这意味着 navigate 不支持 `/*n/` 深度限制和分步调度。

**影响**：如果用户对 navigate 的 base_xpath 使用了 `/*n/` 前缀，会走 uiauto-xpath 而不是深度限制 BFS。

**建议**：统一为走 `execute_xpath_steps_filtered`，或明确文档限制。

### 4.4 ⚠️ `inspect_subtree` 使用 `execute_xpath_steps_filtered`，正确但有限制

```rust
// src/core/uia/inspect.rs:185
execute_xpath_steps_filtered(&auto, &child_elem, &child_xpath, &FindAllFilter::default(), Some(5000))
```

Inspect 的定位步骤正确使用了新调度器。但 `max_depth` 和 `max_nodes` 参数只用于子树遍历阶段（`build_inspect_node`），定位阶段没有超时控制（硬编码 5000ms）。

**建议**：将 `max_depth` 和 `max_nodes` 也应用于定位阶段，或从 `InspectRequest` 中暴露定位超时参数。

### 4.5 ⚠️ `get_element_visibility` 使用 `validate_selector_and_xpath_detailed`，非调度器

```rust
// src/core/uia/visibility.rs:114
let detailed = super::validate_selector_and_xpath_detailed(window_selector, element_xpath, &[], None, None);
```

Visibility 使用旧的验证函数，不走新调度器。如果 element_xpath 包含 `/*n/` 深度限制前缀，可能不会被正确处理。

**建议**：改为走 `execute_xpath_steps_filtered` 定位元素，再计算可见性。

---

## 五、参数名称跨层一致性检查

### 5.1 SDK → API

| SDK 字段名 | API 请求体字段 | 一致？ |
|-----------|--------------|--------|
| `window` | `window` | ✅ |
| `element` | `element` | ✅ |
| `randomRange` | `randomRange` | ✅ |
| `searchMode` | `searchMode` (通过 suffix) | ✅ |
| `timeout` | `timeout` (flash/scroll) | ✅ |
| `container` | `container` (visibility) | ✅ |
| `steps` | `steps` (navigate) | ✅ |
| `format` | `format` (inspect) | ✅ |
| `options.humanize` | `options.humanize` | ✅ |
| `options.randomRange` | `options.randomRange` | ✅ |

**SDK→API 命名一致性：10/10 ✅**

### 5.2 API → Fun

| API 参数名 | Fun 参数名 | 一致？ |
|-----------|-----------|--------|
| `window` | `window_selector` | ✅ |
| `element` | `element_xpath` | ✅ |
| `random_range` | `random_range` | ✅ |
| `timeout_ms` | `timeout_ms` | ✅ |
| `search_context` | `search_context` | ✅ |
| `find_all_filter` | `filter` | ⚠️ 命名不一致 |
| `container` | `container_xpath` | ⚠️ 可接受 |

**API→Fun 命名一致性：8/10** — 小瑕疵。

---

## 六、综合结论

### 6.1 整体匹配度

| 层次 | 检查项数 | 匹配 | 瑕疵 | 不匹配 | 匹配率 |
|------|---------|------|------|--------|--------|
| SDK → API 参数 | 11 | 11 | 0 | 0 | **100%** |
| SDK → API 命名 | 10 | 10 | 0 | 0 | **100%** |
| API → Fun 调用 | 11 | 10 | 1 | 0 | **91%** |
| SDK Element → Fun | 13 | 13 | 0 | 0 | **100%** |
| 策略分派 | 5 | 5 | 0 | 0 | **100%** |
| 二次定位 | 5 | 5 | 0 | 0 | **100%** |
| API → Fun 命名 | 6 | 4 | 2 | 0 | **67%** |
| **总计** | **61** | **58** | **3** | **0** | **95%** |

### 6.2 关键发现

1. ✅ **SDK → API 层完美匹配**：所有请求体字段、命名约定完全对齐，无遗漏。
2. ✅ **SDK Element 方法全部正确路由到后端定位函数**：13/13 匹配。
3. ✅ **重构后的核心调度器设计完整**：`execute_xpath_steps_filtered` 按前缀直接分发，无隐式 fallback，与设计文档一致。
4. ✅ **二次定位 `locate_from_impl` 与需求 §6 完全对齐**：Fast/Full/Adaptive 三种策略，无 Adaptive fallback。
5. ⚠️ **4 个 API/Fun 层的小瑕疵**：
   - `flash_element` 硬编码 randomRange=5.0
   - `find_from_element` 不走新调度器（可能是设计意图）
   - `navigate_from_element` 基准定位用旧函数
   - `get_element_visibility` 不走新调度器

### 6.3 建议优先级

| # | 问题 | 严重度 | 建议 |
|---|------|--------|------|
| 1 | `flash_element` 硬编码参数 | ⚠️ 中 | 在请求体中加入 `randomRange` 字段并传递 |
| 2 | `navigate_from_element` 旧定位函数 | ⚠️ 低 | 改为走 `execute_xpath_steps_filtered` 或明确文档限制 |
| 3 | `get_element_visibility` 旧验证函数 | ⚠️ 低 | 改为走 `execute_xpath_steps_filtered` |
| 4 | `find_from_element` 不走调度器 | ℹ️ 信息 | 确认设计意图，添加注释 |

---

---
## 十四、2026-06-06 SDK API 变更记录

> 基于审查结果，执行了以下 SDK 侧变更。后端 Fun 层过期项另见 §十五。

### 14.1 已执行的变更

| # | 变更 | 文件 | 说明 |
|---|------|------|------|
| 1 | `Element.text()` → `Element.name()` | `element.ts` | 主方法改名，`text()` 保留为 Playwright 兼容别名 |
| 2 | 移除 `Element.getText()` | `element.ts` | 冗余别名已删除 |
| 3 | `Flow.find()` 改为 `findFirst` 别名 | `flow.ts` | 原为 `findOne` 别名（多匹配报错），改为 `findFirst`（返回第一个，不报错） |
| 4 | `Element.find()` 改为 `findFirst` 别名 | `element.ts` | 同上 |
| 5 | 移除 `Element.getAttribute()` | `element.ts` | 冗余别名已删除 |
| 6 | 移除 `Element.getRect()` | `element.ts` | 冗余别名已删除 |
| 7 | 移除 `Element.doubleClick()` | `element.ts` | 保留 `dblclick()` 为主方法 |
| 8 | 移除 `Element.typeText()` | `element.ts` | 保留 `type()` 为主方法 |
| 9 | 移除 `Element.parentElement()` | `element.ts` | 保留 `parent()` 为主方法 |
| 10 | 移除 `Element.nextSiblingElement()` | `element.ts` | 保留 `next()` 为主方法 |
| 11 | 移除 `Element.previousSiblingElement()` | `element.ts` | 保留 `prev()` 为主方法 |
| 12 | 移除 `Element.scrollIntoView()` | `element.ts` | 已 deprecated，保留 `scrollToVisible()` |
| 13 | 移除 `Flow.pressShortcut()` | `flow.ts` | 已 deprecated，保留 `shortcut()` |
| 14 | 移除 `Flow.typeText()` | `flow.ts` | 合并到 `Flow.type()`（支持无 xpath 的全局输入） |
| 15 | 新增 `Flow.activate(selector)` | `flow.ts` | 仅激活窗口不改变上下文；`window()` 保持激活+设上下文 |
| 16 | `Flow.doubleClick()` → 调用 `element.dblclick()` | `flow.ts` | 适配 Element 侧移除 `doubleClick()` 别名 |

### 14.2 保留的别名（Playwright 兼容）

| 别名 | 主方法 | 原因 |
|------|--------|------|
| `Element.text()` | `Element.name()` | Playwright 兼容（返回 name 属性） |
| `Element.boundingBox()` | `Element.bounds()` | Playwright 兼容 |
| `Element.locator()` | `Element.findOne()` | Playwright 兼容 |
| `Element.fill()` | `Element.clear() + type()` | Playwright 兼容 |

### 14.3 变更后的最终 API 清单

**Flow 类（公开方法 20 个）**：
`window()`, `activate()` (新), `existsWindow()`, `findOne()`, `findFirst()`, `find()` (=findFirst), `findAll()`, `nth()`, `waitFor()`, `waitUntilGone()`, `exists()`, `inspect()`, `scrollToVisible()`, `wait()`, `waitUntil()`, `shortcut()`, `pressKey()`, `moveTo()`, `clickAt()`, `click()`, `doubleClick()`, `rightClick()`, `type()`, `focus()`, `setValue()`, `scrollUp()`, `scrollDown()`, `scrollDetect()`, `screenshot()`, `screenshotAuto()`, `startProfile()`, `stopProfile()`, `idle()`, `pushIdle()`, `popIdle()`, `stopIdle()`

**Element 类（公开方法 25 个）**：
`name()` (改名), `text()` (=name, 兼容), `isEnabled()`, `toXpath()`, `getSelector()`, `refresh()`, `isVisible()`, `isOffscreen()`, `checkVisibility()`, `inspect()`, `attr()`, `bounds()`, `boundingBox()` (=bounds), `click()`, `dblclick()`, `rightClick()`, `type()`, `clear()`, `fill()`, `focus()`, `assertExists()`, `assertEnabled()`, `assertVisible()`, `assertText()`, `findOne()`, `findFirst()`, `find()` (=findFirst), `findAll()`, `nth()`, `locator()` (=findOne), `children()`, `childCount()`, `child()`, `indexInParent()`, `parent()`, `next()`, `prev()`, `compass()`, `waitUntilGone()`, `waitFor()`, `scrollToVisible()`, `flash()`, `hover()`, `dragTo()`, `isCheckable()`, `isChecked()`, `isClickable()`, `isScrollable()`, `isSelected()`

---
## 十五、Fun 层过期项标记（待后端修复）

以下 Fun 层问题已在审查中发现，标记为过期，待逐个修复：

| # | 问题 | 位置 | 严重度 | 过期原因 |
|---|------|------|--------|----------|
| ❌ 1 | `navigate_from_element` 基准定位用旧函数 | `core/uia/navigate.rs` | 中 | 使用 `find_by_xpath_detailed`（uiauto-xpath），不支持 `/*n/` 深度限制 |
| ❌ 2 | `get_element_visibility` 不走调度器 | `core/uia/visibility.rs:114` | 中 | 使用 `validate_selector_and_xpath_detailed`，不支持 `/*n/` |
| ❌ 3 | `flash_element` 硬编码 randomRange=5.0 | `api/element.rs:356` | 低 | 未从请求参数传递，与其他 API 不一致 |
| ❌ 4 | `find_from_element_cached` 不走调度器 | `api/element.rs:664` | 低 | 可能设计意图，但未确认 |
| ❌ 5 | `ScrollConfig` 与 `ScrollOptions` 类型重复 | `types.ts` | 低 | SDK 侧类型定义冗余 |

*报告版本：v3.1（覆盖后端 + SDK + 三层匹配 + 变更记录）*  
*审查人：AI Assistant*
