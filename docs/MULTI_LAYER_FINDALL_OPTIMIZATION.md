# 多层 XPath FindAll 优化方案

## 问题

两层层级 XPath（如 `//Group[@AutomationId='791657059']//Text[@Name='...']`）在微信/Chrome WebView 上极慢（30s+），
而单层 XPath（如 `//Text[@Name='...']`）只需 441ms。

### 根因分析

1. **WebView 子窗口上 FindAll(Subtree) 极慢**：Chrome Raw 树有上万节点，`FindAll(Subtree, Group+AutomationId)` 需要 4400ms
2. **uiauto-xpath select_nodes_strict 更慢**：先 FindAllBuildCache 4077ms → 再 raw_children 14592ms → 总计 29089ms
3. **逐层搜索没有利用 FindFirst 快速短路**：单层用 FindFirst 441ms 就找到了，但两层没有类似的快速路径

### 关键洞察

单层 `//Text[@Name='...']` 走的是 `FindFirst(Subtree)` = 441ms（COM 原生 API，非常快）。

两层的正确做法应该是：
1. 先用 `FindFirst(Subtree, Group+AutomationId)` 找到第一个 Group → 应该比 FindAll 快得多
2. 再从找到的 Group 出发，用 `FindFirst(Subtree, Text+Name)` 找 Text

**关键**：`FindFirst` vs `FindAll` 在只需要 1 个结果时差异巨大，尤其在大型 WebView 子树上。

## 优化方案

### 核心思路：链式 FindFirst/FindAll（逐层搜索）

对于 `//A[@x='1']//B[@y='2']` 这类多层 descendant XPath：

```
Step 1: 从搜索根出发，FindFirst(Subtree, A+@x='1')
Step 2: 从 Step 1 结果出发，FindFirst(Subtree, B+@y='2')
```

而不是现在的：
```
Step 1: FindAll(Subtree, A+@x='1') → 获取所有 A 候选
Step 2: 对每个 A 候选，用 uiauto-xpath 或 raw walk 搜索 B
```

### 实现：3 层级策略

#### Strategy 1: 链式 FindFirst（SearchMode::First / 默认）

```
//Group[@AutomationId='791657059']//Text[@Name='xxx']
  ↓
Step 1: child_elem.FindFirst(Subtree, Group+AutomationId) → 1个结果
Step 2: result.FindFirst(Subtree, Text+Name) → 1个结果
```

**预期耗时**：~450ms + ~50ms = ~500ms（vs 现在 33000ms）

#### Strategy 2: 链式 FindAll（SearchMode::All 或需要所有匹配）

```
Step 1: child_elem.FindAll(Subtree, Group+AutomationId) → N个结果
Step 2: 对每个结果 FindAll(Subtree, Text+Name) → 合并
```

#### Strategy 3: 回退（FindFirst/FindAll 失败时）

保持现有的 raw walk + uiauto-xpath 作为最终回退。

### 代码改动范围

#### 1. `find_by_xpath_raw_descendants_with_depth` — 添加链式 FindFirst 路径

当前逻辑：
- 单步：用 FindFirst/FindAll → 返回结果（快）
- 多步：用 FindAll 获取第一步候选 → 对每个候选走 uiauto-xpath（慢）

新逻辑：
- 多步：用 FindFirst 获取第一个第一步匹配 → 链式 FindFirst/FindAll 后续步 → 快速路径
- 如果链式 FindFirst 失败，回退到 FindAll 候选 + 逐个搜索

#### 2. `find_by_xpath_control_descendants_with_depth` — 同样添加链式 FindFirst

#### 3. `find_by_xpath_raw_descendants` — Step 3 child HWND 循环中

当前 child HWND 循环对每个子窗口直接调用 `find_by_xpath_raw_descendants`，
新方案应该在子窗口上也使用链式 FindFirst。

### 具体实现步骤

#### Step 1: 新增 `findall_chain_first` 函数

```rust
/// 链式 FindFirst：逐层用 FindFirst(Subtree) 搜索多层 descendant XPath
/// 适用于 SearchMode::First，只需找到 1 个匹配
///
/// 对于 `//A[@x='1']//B[@y='2']//C[@z='3']`:
///   Step 1: root.FindFirst(Subtree, A+@x='1') → a
///   Step 2: a.FindFirst(Subtree, B+@y='2') → b
///   Step 3: b.FindFirst(Subtree, C+@z='3') → c (最终结果)
fn findall_chain_first(
    auto: &UIAutomation,
    root: &UIElement,
    xpath_parts: &[&str],       // 已解析的各步骤
    filter: &FindAllFilter,
) -> Option<Vec<UIElement>>
```

#### Step 2: 新增 `findall_chain_all` 函数

```rust
/// 链式 FindAll：逐层用 FindAll(Subtree) 搜索多层 descendant XPath
/// 适用于 SearchMode::All，需要所有匹配
fn findall_chain_all(
    auto: &UIAutomation,
    root: &UIElement,
    xpath_parts: &[&str],
    filter: &FindAllFilter,
) -> Option<Vec<UIElement>>
```

#### Step 3: 修改 `find_by_xpath_raw_descendants_with_depth`

在多步分支中，优先尝试链式 FindFirst/FindAll：

```rust
// 现有：先 FindAll 第一步 → 对每个候选走 uiauto-xpath/raw walk
// 新增：先尝试链式 FindFirst（快路径）

if xpath_parts.len() > 1 {
    // ★ 新增：链式 FindFirst 快速路径
    if search_mode != SearchMode::All {
        if let Some(results) = findall_chain_first(auto, window, &xpath_parts, filter) {
            if !results.is_empty() {
                return Ok((results, segments));
            }
        }
    } else {
        if let Some(results) = findall_chain_all(auto, window, &xpath_parts, filter) {
            if !results.is_empty() {
                return Ok((results, segments));
            }
        }
    }
    // 回退到现有逻辑...
}
```

#### Step 4: 修改 `find_by_xpath_control_descendants_with_depth`

同样的链式 FindFirst/FindAll 路径。

#### Step 5: 修改 `cached_descendant_child_hwnd` 和 Step 3 child HWND 循环

确保 child HWND 路径也利用链式 FindFirst。

### 性能预期

| 场景 | 当前耗时 | 优化后预期 |
|------|---------|-----------|
| 单层 `//Text[@Name='...']` | 441ms | 441ms（不变） |
| 两层 `//Group[@...]//Text[@...]` | 33000ms | ~500ms |
| 三层 `//A//B//C` | 60000ms+ | ~600ms |

### 注意事项

1. **复杂谓词**：starts-with/contains/matches 无法构建 UIA condition，回退到现有路径
2. **后过滤**：FindFirst/FindAll 的结果仍需经过 `filter_findall_results`（offscreen/零尺寸/越界）
3. **回退安全**：链式 FindFirst 失败时，自动回退到现有 FindAll+uiauto-xpath 路径
4. **`//` vs `/`**：链式 FindFirst 只适用于 `//` descendant 轴步骤之间的连接，
   `/` child 轴步骤用 `FindAll(Children)` 更合适
