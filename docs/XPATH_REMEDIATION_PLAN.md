# XPath 需求对齐整改计划

## 版本历史

| 版本 | 日期 | 作者 | 变更说明 |
|------|------|------|----------|
| 1.0 | 2026-06-05 | System | 基于需求 vs 测试偏差分析的整改计划 |

---

## 1. 背景

基于 `需求规格-定位.md` v1.3 与当前 159 个测试的对比分析（详见 `2026-06-05.md` 偏差分析），当前代码在需求实现上存在系统性负偏差。本计划制定分阶段整改方案，目标是**消除所有 P0/P1 负偏差，使需求覆盖率从 ~38% 提升至 ~90%+**。

### 1.1 偏差总览

| 类别 | 数量 | 说明 |
|------|------|------|
| **严重负偏差（P0/P1）** | ~35 个缺失 | findOne 唯一性、findAll Filter、超时、二次定位、校验API |
| **中等负偏差（P1）** | ~15 个缺失 | L2 执行测试空白 |
| **合理正偏差** | 85 个 | 保留，继续维护 |
| **需求无关** | 41 个 | 保留，属于其他子系统 |

### 1.2 已完成工作

阶段 1 (XPath 解析重构) 和 阶段 2 (执行策略重构) 已完成：
- `XPathStepPrefix` / `StepExecutionStrategy` 枚举 ✅
- `parse_xpath_step` 支持 `/*n/` 前缀 ✅
- `find_with_depth_limit` / `find_with_depth_limit_timeout` BFS ✅
- `execute_xpath_steps` 无隐式 fallback 的主调度 ✅
- `find_by_xpath_with_fallback` 已标记 deprecated ✅
- 34 个 L1 解析+策略测试通过 ✅

---

## 2. 整改分阶段计划

### 阶段 3：定位语义完善（P0-P1）—— 当前阶段

#### 3.1 `findOne` 叶子唯一性验证 【P0 · 预计 3h】

**需求** (§5.3.1, §12.1)：
- 路径中间节点：`FindFirst`，不验证唯一性 ✅（已实现）
- 叶子节点：`FindAll` 最多取 2 个，count > 1 → `LeafNotUnique` ❌（仅做全局 count）

**当前实现** (`apply_search_mode_ui`, find.rs:1762-1786)：
```rust
SearchMode::OnlyOne => {
    if results.len() > 1 {
        log::warn!("...found {} UIElements — returning empty", results.len());
        vec![]
    }
}
```
**问题**：在全局做 count，未在父节点下验证。

**改动**：
1. `execute_xpath_steps_filtered` 中：当 `search_mode == SearchMode::OnlyOne` 且为最后一步时，在 `execute_direct_child`/`execute_descendant_*`/`DepthLimitedBfs` 路径中：
   - 使用 `FindAll(scope, condition)` 限制返回 2 个
   - 若 count > 1 → 返回错误/空（`LeafNotUnique`）
   - 若 count == 1 → 正常返回
   - 若 count == 0 → `StepNotFound`

2. `NotFoundReason` 新增 `LeafNotUnique { candidates: usize }` 变体（需求 §7.2）

3. `LocateError` 新增 `LeafNotUnique` 变体（需求 §8.5）

**文件**：`src/core/uia/find.rs`（修改 `execute_xpath_steps_filtered`）、`src/core/model.rs`（新增枚举变体）

**测试（5个 L2）**：
- `test_find_one_leaf_unique` — 父节点下仅 1 匹配 → Ok
- `test_find_one_leaf_not_unique` — 父节点下 2 匹配 → `LeafNotUnique`
- `test_find_one_leaf_not_found` — 父节点下 0 匹配 → `StepNotFound`
- `test_find_one_mid_node_not_checked` — 中间节点不验证唯一性
- `test_find_one_leaf_descendant_unique` — `//` 叶子唯一性（祖先范围内）

---

#### 3.2 `findAll` FilterCondition 【P0 · 预计 3h】

**需求** (§5.3, §8.4)：
- `findAll` 支持 `FilterCondition`（Eq/NotEq/Contains/Regex/Exists）
- 非 Eq 操作：`FindAll` 后客户端过滤

**当前实现**：`FindAllFilter` 只处理 offscreen/zero-size/out-of-bounds，不支持属性过滤。

**改动**：
1. `core/model.rs` 新增：
```rust
pub enum FilterOp {
    Eq, NotEq, Contains, Regex, Exists,
}

pub struct AttributeFilter {
    pub property: String,
    pub operator: FilterOp,
    pub value: String,
}
```

2. `execute_xpath_steps_filtered` 中：最后一步执行完后，若 `search_mode == SearchMode::All` 且有 `AttributeFilter`，对结果做客户端过滤：
```rust
fn apply_attribute_filter(
    elements: Vec<UIElement>,
    filter: &AttributeFilter,
) -> Vec<UIElement> {
    match filter.operator {
        FilterOp::Eq => elements, // 已在 UIA 条件中处理
        _ => elements.into_iter()
            .filter(|elem| element_matches_attribute_filter(elem, filter))
            .collect(),
    }
}
```

3. `element_matches_attribute_filter`：使用 `get_uia_property_for_xpath` 获取属性值，按 operator 比较

4. 序列化/反序列化（用于 API 传输）

**文件**：`src/core/model.rs`（新增类型）、`src/core/uia/find.rs`（过滤逻辑）

**测试（7个 L1/L2）**：
- `test_filter_eq` — Eq 过滤
- `test_filter_not_eq` — NotEq 过滤
- `test_filter_contains` — Contains 过滤
- `test_filter_regex` — Regex 过滤
- `test_filter_exists` — Exists 过滤
- `test_filter_combined` — 多个 FilterCondition 组合
- `test_filter_serialization` — 序列化/反序列化

---

#### 3.3 超时保护 【P1 · 预计 2h】

**需求** (§5.5)：
- Fast 默认 1500ms，Full 默认 3000ms
- 不允许自动重试，超时返回 `Timeout` 错误

**当前实现**：
- `find_by_xpath_with_fallback` 有 `XPATH_FALLBACK_BUDGET_MS = 3000`，但只记录日志，不返回 Timeout 错误
- `validate_selector_and_xpath_detailed` 有 `effective_timeout` 但未传递到 `execute_xpath_steps`

**改动**：
1. `execute_xpath_steps_filtered` 增加 `timeout_ms: Option<u64>` 参数
2. 每步执行前后检查 `start.elapsed() > timeout`，超时则返回 `LocateError::Timeout`
3. `LocateError` 新增 `Timeout` 变体
4. `find_all_elements_detailed` 等 public API 传递 `_timeout_ms` 参数（当前已接受但未使用）
5. `validate_selector_and_xpath_detailed` 传递 `effective_timeout` 给 `find_by_xpath_with_fallback`

**文件**：`src/core/uia/find.rs`、`src/core/model.rs`、`src/core/uia/validation.rs`

**测试（3个 L1/L2）**：
- `test_timeout_fast_default_1500ms` — Fast 模式超时验证
- `test_timeout_full_default_3000ms` — Full 模式超时验证
- `test_no_auto_retry_on_timeout` — 超时后不重试

---

### 阶段 4：二次定位 + RuntimeId 缓存（P1）

#### 4.1 二次定位 API 【P1 · 预计 3h】

**需求** (§6)：
- `findFirstFrom(parent_runtime_id, relative_xpath, strategy)` 
- `findOneFrom(parent_runtime_id, relative_xpath, strategy)` — 叶子唯一性
- `findAllFrom(parent_runtime_id, relative_xpath, strategy, filter)` — FilterCondition
- `InvalidParent` 错误

**当前实现**：`find_from_element_cached` / `find_from_element_cached_filtered` 已支持 RuntimeId 查找，但：
- 使用 `SearchStrategy` 而非 `LocateMode`
- 内部有 Adaptive fallback（Fast→Full），违反"无自动 fallback"
- 没有独立的 `findOneFrom` 语义

**改动**：
1. 新增 public API：
```rust
pub fn locate_first_from(runtime_id: &str, relative_xpath: &str, strategy: SearchStrategy) -> Result<ElementData>;
pub fn locate_one_from(runtime_id: &str, relative_xpath: &str, strategy: SearchStrategy) -> Result<ElementData>;
pub fn locate_all_from(runtime_id: &str, relative_xpath: &str, strategy: SearchStrategy, filter: Option<&AttributeFilter>) -> Result<Vec<ElementData>>;
```

2. 移除 `find_from_element_cached` 中的 Adaptive fallback 分支（`_ => {...}` 1986-2028行）

3. 父元素不存在时返回 `LocateError::InvalidParent`

4. `locate_one_from` 复用阶段 3.1 的叶子唯一性验证

**文件**：`src/core/uia/find.rs`、`src/core/model.rs`

**测试（5个 L2）**：
- `test_locate_first_from_valid_parent` — 有效父元素查找
- `test_locate_first_from_invalid_parent` — 无效父元素 → `InvalidParent`
- `test_locate_one_from_leaf_unique` — 叶子唯一性验证
- `test_locate_all_from_with_filter` — FilterCondition
- `test_locate_from_no_fallback` — 无 Adaptive fallback

---

#### 4.2 RuntimeId 缓存 【P2 · 预计 2h】

**需求** (§12.4)：
- LRU 缓存，最大容量 500
- Key: RuntimeId 字符串，Value: `UIElement`

**当前实现**：`element_cache.rs` 已有 LRU 缓存（`get_cached_element`, `cache_element`），容量 512。

**差距**：无专门测试覆盖 RuntimeId 缓存的容量、LRU 驱逐、失效场景。

**改动**：
1. 确认 `element_cache.rs` 的 LRU 实现符合需求
2. 如容量不是 500，调整为 500

**文件**：`src/core/element_cache.rs`

**测试（3个 L1）**：
- `test_runtime_cache_capacity_500` — 容量上限验证
- `test_runtime_cache_lru_eviction` — LRU 驱逐验证
- `test_runtime_cache_invalid_id` — 无效 RuntimeId 返回 None

---

### 阶段 5：校验 API 完善（P1）

#### 5.1 校验失败原因分支 【P1 · 预计 3h】

**需求** (§7)：
- `ValidateRequest` / `ValidateResponse` 结构体
- `NotFoundReason` 枚举：WindowNotFound, ChildHwndNotFound, StepNotFound, LeafNotUnique, Timeout
- 失败步骤诊断

**当前实现** (`validation.rs`)：
- `validate_selector_and_xpath_detailed` 已有 `DetailedValidationResult` 含 `not_found_reason: Option<NotFoundReason>`
- `NotFoundReason` 已有 WindowNotFound / ChildHwndNotFound / ElementGone / Timeout
- **缺失**：`StepNotFound`（含 step 索引）、`LeafNotUnique`（含 candidates 数量）

**改动**：
1. `NotFoundReason` 新增变体（`core/model.rs`）：
```rust
StepNotFound { step: usize, xpath_step: String },
LeafNotUnique { candidates: usize },
```

2. `execute_xpath_steps_filtered` 中：当某步骤找不到时，生成 `StepNotFound` 而非简单返回空

3. `validate_selector_and_xpath_detailed` 中：
   - 从 `execute_xpath_steps` 的 `segment_results` 提取失败步骤
   - 设置 `not_found_reason = Some(NotFoundReason::StepNotFound { step, xpath_step })`

4. 接入阶段 3.1 的 `LeafNotUnique` 到校验流程

**文件**：`src/core/model.rs`、`src/core/uia/find.rs`、`src/core/uia/validation.rs`

**测试（6个 L1/L2）**：
- `test_validate_window_not_found` — WindowNotFound
- `test_validate_child_hwnd_not_found` — ChildHwndNotFound
- `test_validate_step_not_found` — StepNotFound 含 step 索引
- `test_validate_leaf_not_unique` — LeafNotUnique
- `test_validate_timeout` — Timeout
- `test_validate_success` — 成功路径

---

### 阶段 6：L2 执行测试补充（P1）

#### 6.1 XPath 执行策略测试 【预计 4h】

**需求覆盖**：验证四种 XPath 语法 (`/A`, `//A`, `/*/A`, `/*n/A`) 的 TreeScope 和行为正确性。

**当前状态**：L1 解析测试完整 (34个)，L2 执行测试空白。

**测试文件**：`src/core/uia/tests/xpath_executor_tests.rs`（新增）

| # | 测试函数 | 验证内容 |
|---|----------|----------|
| 1 | `test_direct_child_uses_tree_scope_children` | `/A` 使用 `FindFirst(Children)` |
| 2 | `test_descendant_uses_tree_scope_descendants` | `//A` 使用 `FindFirst(Descendants)` |
| 3 | `test_depth_2_bfs_limited` | `/*/A` BFS 深度=2 |
| 4 | `test_depth_n_bfs_limited` | `/*4/A` BFS 深度=5 |
| 5 | `test_depth_limit_not_exceeded` | 不搜索超出深度限制的节点 |
| 6 | `test_no_descendants_for_fixed_depth` | 禁止对 `/*n/` 使用 Descendants |
| 7 | `test_fast_mode_uses_control_view` | Fast 模式使用 ControlViewWalker |
| 8 | `test_full_mode_uses_raw_view` | Full 模式使用 RawViewWalker |
| 9 | `test_chain_find_first_multi_step` | `//A//B//C` 链式逐层搜索 |
| 10 | `test_no_implicit_fallback` | 策略失败时不尝试替代策略 |
| 11 | `test_timeout_protection_large_depth` | `/*100/` 超时保护触发 |
| 12 | `test_build_uia_condition_from_step` | UIA 条件构建正确性 |

**注意**：L2 测试需要 mock/fake UIA 环境。可使用现有 mock 数据或构造假的 UIA 树。如果 mock 成本过高，部分测试标记为 `#[ignore]`，文档记录预期行为。

---

### 阶段 7：代码优化（P2）

#### 7.1 消除 `apply_search_mode` 重复 【P2 · 预计 1h】

**改动**：合并 `apply_search_mode_ui` 和 `apply_search_mode` 为泛型函数，或者让 `ElementData` 实现某 trait。

**文件**：`src/core/uia/find.rs`

**测试**：`test_apply_search_mode_behavior_consistent` — UIElement 和 ElementData 行为一致

#### 7.2 删除 deprecated 函数 【P2 · 预计 2h】

**改动**：
1. 所有调用点从 `find_by_xpath_with_fallback` 迁移到 `execute_xpath_steps`
2. 删除 `find_by_xpath_with_fallback` 和 `find_by_xpath_with_fallback_filtered`
3. 删除 `record_and_return` 等旧 fallback 辅助函数

**影响范围**：
- `validation.rs`：`validate_selector_and_xpath_detailed`
- `find.rs`：`find_all_elements_detailed`, `find_from_element_cached`
- `inspect.rs`：可能引用

**文件**：`src/core/uia/find.rs`、`src/core/uia/validation.rs`、`src/core/uia/inspect.rs`

#### 7.3 函数命名语义化 【P2 · 预计 2h】

按照 `XPATH_REFACTOR_PLAN.md` §4 的命名方案，逐步重命名函数。

**优先重命名**：
| 旧名 | 新名 | 优先级 |
|------|------|--------|
| `find_all_elements_detailed` | `find_elements_by_xpath` | P1 |
| `find_from_element_cached` | `locate_by_runtime_id` | P1 |
| `validate_selector_and_xpath_detailed` | `validate_xpath` | P1 |
| `find_by_xpath_control_descendants` | `search_descendants_via_control_view` | P2 |
| `find_by_xpath_raw_descendants` | `search_descendants_via_raw_view` | P2 |

---

## 3. 执行顺序与工时估算

| 阶段 | 优先级 | 编码 | 测试 | 合计 | 依赖 |
|------|--------|------|------|------|------|
| **3.1** findOne 唯一性 | **P0** | 2h | 1h | **3h** | 无 |
| **3.2** FilterCondition | **P0** | 2h | 1h | **3h** | 无 |
| **3.3** 超时保护 | P1 | 1h | 1h | **2h** | 无 |
| **4.1** 二次定位 | P1 | 2h | 1h | **3h** | 3.1, 3.2 |
| **4.2** RuntimeId 缓存 | P2 | 0.5h | 1h | **1.5h** | 无 |
| **5.1** 校验失败原因 | P1 | 2h | 1h | **3h** | 3.1 |
| **6.1** L2 执行测试 | P1 | 0h | 4h | **4h** | 阶段 2 完成 |
| **7.1** 消除重复 | P2 | 1h | 0h | **1h** | 无 |
| **7.2** 删 deprecated | P2 | 2h | 0h | **2h** | 4.1, 5.1 |
| **7.3** 命名语义化 | P2 | 2h | 0h | **2h** | 7.2 |

**总计**：约 **24.5 小时**

### 执行顺序

```
阶段 3.1 (P0) ──→ 阶段 3.2 (P0) ──→ 阶段 3.3 (P1)
     │                                    │
     └────────── 阶段 4.1 (P1) ←─────────┘
                     │
     ┌───────────────┘
     ▼
阶段 5.1 (P1) ──→ 阶段 7.2 (删deprecated) ──→ 阶段 7.3 (命名)

阶段 4.2 (P2)  ← 独立，可并行

阶段 6.1 (P1)  ← 独立，可并行

阶段 7.1 (P2)  ← 独立，可并行
```

**推荐执行顺序**：3.1 → 3.2 → 3.3 → 4.1 → 5.1 → 7.2 → 7.3（串行），4.2 / 6.1 / 7.1 可并行穿插。

---

## 4. 验收标准

### 功能验收
- [ ] `findOne` 叶子节点在父节点下唯一性验证，>1 时报 `LeafNotUnique`
- [ ] `findAll` 支持 Eq/NotEq/Contains/Regex/Exists 五种 FilterCondition
- [ ] 超时保护：Fast 1500ms、Full 3000ms，超时不重试
- [ ] 二次定位：`findFirstFrom`/`findOneFrom`/`findAllFrom` + `InvalidParent`
- [ ] 校验 API 返回 `NotFoundReason` 所有分支（StepNotFound, LeafNotUnique）
- [ ] RuntimeId 缓存 LRU 500 容量

### 测试验收
- [ ] 新增 ~40 个测试全部通过
- [ ] 159 个现有测试无回归
- [ ] L2 执行测试覆盖四种 XPath 语法的 TreeScope 和行为

### 需求覆盖率
- [ ] 当前 ~38% → 目标 ~90%+
- [ ] P0/P1 负偏差全部消除

### 编译验收
- [ ] `cargo build` 通过（debug 模式）
- [ ] 0 lint 错误
- [ ] 0 新引入的 lint 警告

---

## 5. 风险

1. **L2 测试 mock 成本**：若 UIA mock 环境搭建复杂，部分测试标记 `#[ignore]`，文档记录预期行为
2. **向后兼容**：public API 重命名（阶段 7.3）需"先增后删"，API 层调用点同步更新
3. **性能回归**：`findOne` 唯一性验证多一次 `FindAll(scope, cond, 2)`，额外 0.1-0.5ms（需求 §9.1 已说明可接受）
4. **删除 deprecated 影响范围**：`find_by_xpath_with_fallback` 有 3+ 个调用点，需逐一验证迁移
