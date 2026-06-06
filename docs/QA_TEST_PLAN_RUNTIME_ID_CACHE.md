# RuntimeId 缓存优化 — QA 测试计划

> 日期：2026-06-06
> 版本：v1.0
> 范围：SDK (TypeScript) + API (Rust) + Core (Rust) 全链路

---

## 一、测试概览

### 1.1 功能简介

RuntimeId 缓存优化将全窗口 XPath 搜索（50-200ms）替换为缓存命中后的直接操作（~1ms）。核心链路：

```
SDK Element.runtimeId → API 路径A（缓存优先，无隐式fallback） → Core ElementCache
```

### 1.2 关键设计原则（影响测试策略）

| 原则 | 测试要求 |
|------|----------|
| **无隐式 fallback** | 缓存未命中/过期 → 直接返回错误，不会自动切到 XPath |
| **双路径架构** | 每个端点都有路径A（runtimeId缓存）和路径B（XPath搜索） |
| **可配置 TTL** | 全局 TTL + 单次查找 TTL，None = 永不过期 |
| **向后兼容** | 所有新增参数均为可选，旧调用方式不受影响 |

### 1.3 测试矩阵

| 维度 | 覆盖范围 |
|------|----------|
| 测试层级 | 单元测试 (Core)、集成测试 (API)、端到端测试 (SDK) |
| 测试类型 | 功能、性能、边界、异常、并发、回归 |
| 被测组件 | element_cache.rs、api/element.rs、SDK element.ts、SDK client.ts |

---

## 二、单元测试 (Core Layer)

### TC-CORE-01: ElementCache 基本 CRUD

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **模块** | `src/core/element_cache.rs` |
| **前置条件** | 缓存为空 |
| **步骤** | 1. 调用 `cache_element("1,100,1", mock_element)` 2. 调用 `get_cached_element("1,100,1")` 3. 验证返回 Some |
| **预期** | 命中返回 element；`cache_size() == 1` |

### TC-CORE-02: 缓存未命中返回 None

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **模块** | `src/core/element_cache.rs` |
| **步骤** | 1. `get_cached_element("nonexistent")` |
| **预期** | 返回 None，不 panic |

### TC-CORE-03: LRU 淘汰

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **模块** | `src/core/element_cache.rs` |
| **步骤** | 1. 插入 513 个元素（MAX=512） 2. 验证 size = 512 3. 验证第 1 个元素被淘汰 4. 验证第 2 个元素仍在缓存中 |
| **预期** | 最老的元素被淘汰，新元素被保留 |

### TC-CORE-04: LRU 提升

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **模块** | `src/core/element_cache.rs` |
| **步骤** | 1. 插入 A、B、C 三个元素 2. `get("A")` 提升 A 3. 插入 D 触发淘汰（容量设为3） 4. 验证 B 被淘汰（A 因提升而存活） |
| **预期** | 被访问的元素移到 MRU 端，不易被淘汰 |

### TC-CORE-05: 重复插入不覆盖

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **模块** | `src/core/element_cache.rs` |
| **步骤** | 1. `cache_element("1,100,1", elem1)` 2. `cache_element("1,100,1", elem2)` 3. `get("1,100,1")` 返回 elem1 |
| **预期** | 已存在的 key 不会被覆盖（`if contains_key → return`） |

### TC-CORE-06: TTL 过期检查

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **模块** | `src/core/element_cache.rs` |
| **步骤** | 1. `set_default_ttl(Some(50ms))` 2. `cache_element("1,100,1", elem)` 3. 立即 `get` → 命中 4. sleep 60ms 5. `get` → 返回 None 6. 验证缓存条目被清除 |
| **预期** | 过期元素自动移除，size 减少 |

### TC-CORE-07: TTL = None 永不过期

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **模块** | `src/core/element_cache.rs` |
| **步骤** | 1. `set_default_ttl(None)` 2. `cache_element` 3. sleep 1s 4. `get` 仍命中 |
| **预期** | None = 永不过期 |

### TC-CORE-08: get_with_ttl 覆盖全局 TTL

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **模块** | `src/core/element_cache.rs` |
| **步骤** | 1. `set_default_ttl(Some(100ms))` 2. `cache_element` 3. `get_cached_element_with_ttl("key", Some(1s))` → 命中 4. sleep 150ms 5. `get_cached_element_with_ttl("key", Some(1s))` → 命中（自定义TTL未过） 6. `get_cached_element("key")` → None（全局TTL已过） |
| **预期** | 自定义 TTL 覆盖全局 TTL |

### TC-CORE-09: clear_cache / remove_cached_element

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **模块** | `src/core/element_cache.rs` |
| **步骤** | 1. 插入 A、B、C 2. `remove_cached_element("B")` 3. 验证 A、C 仍在，B 不在 4. `clear_cache()` 5. 验证 size = 0 |
| **预期** | 精确删除和全清功能正常 |

### TC-CORE-10: 锁中毒恢复

| 项目 | 内容 |
|------|------|
| **优先级** | P2 |
| **模块** | `src/core/element_cache.rs` |
| **步骤** | 1. 模拟 `RwLock` 中毒（前一个持有者在 panic 中退出） 2. 调用 `get_cached_element` 3. 验证不 panic，正常返回 |
| **预期** | `recover_lock` 使用 `into_inner()` 优雅恢复 |

### TC-CORE-11: 线程安全并发读写

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **模块** | `src/core/element_cache.rs` |
| **步骤** | 1. 启动 10 个写线程同时插入不同 key 2. 启动 10 个读线程同时读取 3. 验证无数据竞争，size 正确 |
| **预期** | `RwLock` 保证并发安全，最终 size 与写入 key 数一致 |

---

## 三、集成测试 (API Layer)

### 3.1 路径 A: runtimeId 缓存优先

#### TC-API-01: get_element runtimeId 命中

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **端点** | `GET/POST /api/element` |
| **前置条件** | 缓存中有该 runtimeId 的元素 |
| **请求** | `{ "window": "...", "element": "...", "runtimeId": "42,123,1" }` |
| **预期** | `found: true`, `element` 不为 null, 耗时 < 10ms |

#### TC-API-02: get_element runtimeId 未命中

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **端点** | `GET/POST /api/element` |
| **请求** | `{ "runtimeId": "nonexistent_id" }` |
| **预期** | `found: false`, `error` 包含 "不在缓存中"，**不走 XPath** |

#### TC-API-03: get_element_all runtimeId 命中

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **端点** | `GET/POST /api/element/all` |
| **前置条件** | 缓存中有该元素 |
| **请求** | `{ "window": "...", "element": "...", "runtimeId": "..." }` |
| **预期** | `found: true`, `elements` 包含 1 个元素, `total: 1` |

#### TC-API-04: visibility runtimeId 命中

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **端点** | `POST /api/element/visibility` |
| **前置条件** | 缓存中有该元素 |
| **请求** | `{ "window": "...", "element": "...", "runtimeId": "..." }` |
| **预期** | 返回可见性信息，`found: true`，耗时 < 10ms |

#### TC-API-05: flash runtimeId 命中

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **端点** | `POST /api/element/flash` |
| **前置条件** | 缓存中有该元素，元素有矩形区域 |
| **请求** | `{ "window": "...", "element": "...", "runtimeId": "...", "timeout": 500 }` |
| **预期** | `success: true`, 高亮闪烁出现 |

#### TC-API-06: flash runtimeId 无矩形

| 项目 | 内容 |
|------|------|
| **优先级** | P2 |
| **端点** | `POST /api/element/flash` |
| **前置条件** | 缓存中有该元素，但元素无矩形信息 |
| **预期** | `success: false`, `error` 包含 "无矩形区域信息" |

#### TC-API-07: inspect runtimeId 命中

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **端点** | `POST /api/element/inspect` |
| **前置条件** | 缓存中有该元素 |
| **请求** | `{ "window": "...", "element": "...", "runtimeId": "...", "max_depth": 2 }` |
| **预期** | `success: true`, 返回子树节点列表 |

#### TC-API-08: navigate runtimeId 命中

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **端点** | `POST /api/element/navigate` |
| **前置条件** | 缓存中有基准元素 |
| **请求** | `{ "window": "...", "element": "...", "runtimeId": "...", "steps": [...] }` |
| **预期** | 导航成功返回目标元素信息 |

#### TC-API-09: refresh_by_runtime_id 命中

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **端点** | `POST /api/element/refresh` |
| **前置条件** | 缓存中有该元素 |
| **请求** | `{ "window": "...", "runtimeId": "..." }` |
| **预期** | `found: true`, 返回最新元素属性 |

#### TC-API-10: refresh_by_runtime_id 未命中

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **端点** | `POST /api/element/refresh` |
| **请求** | `{ "runtimeId": "nonexistent" }` |
| **预期** | `found: false`, `error` 包含 "不在缓存中" |

#### TC-API-11: find_from_element 成功

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **端点** | `POST /api/element/find-from` |
| **前置条件** | 缓存中有父元素 |
| **请求** | `{ "runtimeId": "...", "xpath": "//Button", "randomRange": 0.0 }` |
| **预期** | `found: true`, 从父元素子树中找到匹配子元素 |

#### TC-API-12: find_from_element 缓存未命中

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **端点** | `POST /api/element/find-from` |
| **请求** | `{ "runtimeId": "nonexistent", "xpath": "//Button" }` |
| **预期** | 返回错误，明确指示缓存未命中 |

### 3.2 路径 B: XPath 搜索（回归测试）

#### TC-API-13: get_element 无 runtimeId（XPath 路径）

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **端点** | `GET/POST /api/element` |
| **请求** | `{ "window": "...", "element": "/Window[1]/Pane[2]/Button[5]" }` （无 runtimeId） |
| **预期** | 走原有 XPath 搜索逻辑，功能不变 |

#### TC-API-14: 所有端点 XPath 路径回归

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **端点** | `/api/element/all`, `/api/element/visibility`, `/api/element/flash`, `/api/element/inspect`, `/api/element/navigate` |
| **请求** | 全部不传 runtimeId |
| **预期** | 原有 XPath 功能完全不受影响 |

### 3.3 缓存管理 API

#### TC-API-15: 设置缓存 TTL

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **端点** | `PUT /api/element/cache/config` |
| **步骤** | 1. PUT `{ "cacheTTL": 5000 }` 2. GET `/api/element/cache/stats` 3. 验证 `defaultTtlMs: 5000` |
| **预期** | TTL 设置成功并可在 stats 中看到 |

#### TC-API-16: TTL = null 永不过期

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **端点** | `PUT /api/element/cache/config` |
| **请求** | `{ "cacheTTL": null }` |
| **预期** | `defaultTtlMs: null`，缓存永不过期 |

#### TC-API-17: 获取缓存统计

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **端点** | `GET /api/element/cache/stats` |
| **预期** | 返回 `{ size, maxSize, defaultTtlMs }` 三者均有值 |

#### TC-API-18: 清除缓存

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **端点** | `POST /api/element/cache/clear` |
| **步骤** | 1. 确保缓存有数据 2. POST clear 3. GET stats 验证 `size: 0` |
| **预期** | `{ "cleared": true }`，缓存为空 |

---

## 四、端到端测试 (SDK Layer)

### 4.1 Element 动作方法 (P0)

#### TC-SDK-01: click 传递 runtimeId

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **文件** | `element.ts` `click()` |
| **前置条件** | `element.info.runtimeId` 存在 |
| **验证** | 1. 调用 `el.click()` 2. 检查 HTTP 请求体包含 `runtimeId` 字段 3. 后端走路径A（缓存命中） |
| **预期** | `POST /api/click` 请求体含 `runtimeId`，操作成功 |

#### TC-SDK-02: rightClick 传递 runtimeId

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **文件** | `element.ts` `rightClick()` |
| **验证** | HTTP 请求体含 `runtimeId` |
| **预期** | 右键操作成功 |

#### TC-SDK-03: type 传递 runtimeId (Value 模式)

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **文件** | `element.ts` `type()` |
| **验证** | `POST /api/type` 请求体含 `runtimeId` |
| **预期** | 文本输入成功 |

#### TC-SDK-04: hover 传递 runtimeId

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **文件** | `element.ts` `hover()` |
| **验证** | `POST /api/hover` 请求体含 `runtimeId` |
| **预期** | 鼠标悬停成功 |

#### TC-SDK-05: dragTo 传递 sourceRuntimeId + targetRuntimeId

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **文件** | `element.ts` `dragTo()` |
| **验证** | `POST /api/drag` 请求体含 `sourceRuntimeId` 和 `targetRuntimeId` |
| **预期** | 拖拽操作成功 |

### 4.2 Element 查询方法 (P1)

#### TC-SDK-06: refresh 无参数走 runtimeId

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **文件** | `element.ts` `refresh()` |
| **步骤** | 1. `el.refresh()` （无参数） 2. 检查调用了 `POST /api/element/refresh` 3. 请求体含 `runtimeId` |
| **预期** | 走缓存刷新路径，~1ms |

#### TC-SDK-07: refresh 有参数走 XPath

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **文件** | `element.ts` `refresh()` |
| **步骤** | 1. `el.refresh(['name', 'isEnabled'])` 2. 检查调用了 `POST /api/element` 3. 走 XPath 搜索路径 |
| **预期** | 有参数时走传统 XPath 路径 |

#### TC-SDK-08: refreshByXpath 显式 XPath 刷新

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **文件** | `element.ts` `refreshByXpath()` |
| **步骤** | 1. 调用 `el.refreshByXpath(findFn)` 2. 验证走 XPath 搜索路径 |
| **预期** | 即使有 runtimeId 也强制走 XPath |

#### TC-SDK-09: checkVisibility 传递 runtimeId

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **文件** | `element.ts` `checkVisibility()` |
| **验证** | `POST /api/element/visibility` 请求体含 `runtimeId` |
| **预期** | 可见性检查走缓存路径 |

#### TC-SDK-10: flash 传递 runtimeId

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **文件** | `element.ts` `flash()` |
| **验证** | `POST /api/element/flash` 请求体含 `runtimeId` |
| **预期** | 高亮闪烁走缓存路径 |

#### TC-SDK-11: assertExists runtimeId 优先

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **文件** | `element.ts` `assertExists()` |
| **步骤** | 1. 有 runtimeId 时调用 `refreshByRuntimeId` 2. 验证缓存命中时 assert 通过 |
| **预期** | 有 runtimeId 且缓存命中 → 快速验证通过 |

#### TC-SDK-12: inspect 传递 runtimeId

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **文件** | `element.ts` `inspect()` |
| **验证** | `POST /api/element/inspect` 请求体含 `runtimeId` |
| **预期** | 子树检查走缓存路径 |

### 4.3 Element 子元素查找 (P2)

#### TC-SDK-13: findElement runtimeId 走 findFromElement

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **文件** | `element.ts` `findElement()` |
| **步骤** | 1. `parent.findElement('//Button')` 2. 检查调用了 `POST /api/element/find-from` 3. 请求体含 `runtimeId` + `xpath` |
| **预期** | 子元素查找走 findFromElement API，~5-15ms |

#### TC-SDK-14: findElement 无 runtimeId 走 XPath

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **文件** | `element.ts` `findElement()` |
| **前置条件** | element 无 runtimeId |
| **步骤** | 1. `parent.findElement('//Button')` 2. 检查走 `findElementByXPath()` 回退 |
| **预期** | 回退到传统全窗口 XPath 搜索 |

#### TC-SDK-15: findAll runtimeId 走 findFromElement

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **文件** | `element.ts` `findAll()` |
| **步骤** | 1. `parent.findAll('//Button')` 2. 检查调用了 `POST /api/element/find-from` |
| **预期** | 批量查找走 findFromElement |

#### TC-SDK-16: nth runtimeId 走 findFromElement

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **文件** | `element.ts` `nth()` |
| **验证** | 使用 `findFromElement` 搜索 `//*[position()=N]` |
| **预期** | 第 N 个子元素查找走缓存路径 |

#### TC-SDK-17: children runtimeId 走 findFromElement

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **文件** | `element.ts` `children()` |
| **验证** | 使用 `findFromElement` 搜索 `/*` 直接子元素 |
| **预期** | 子元素列表走缓存路径 |

### 4.4 Element 等待/导航方法 (P3-P4)

#### TC-SDK-18: waitUntilGone runtimeId 走 refreshByRuntimeId

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **文件** | `element.ts` `waitUntilGone()` |
| **验证** | 轮询使用 `POST /api/element/refresh` 而非全窗口 XPath |
| **预期** | 每次轮询 ~1ms，总体等待效率大幅提升 |

#### TC-SDK-19: waitFor runtimeId 走 refreshByRuntimeId

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **文件** | `element.ts` `waitFor()` |
| **验证** | 轮询使用缓存刷新 |
| **预期** | 快速轮询，不走 XPath |

#### TC-SDK-20: compass 传递 runtimeId + cacheTTL

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **文件** | `element.ts` `compass()` |
| **验证** | `POST /api/element/navigate` 请求体含 `runtimeId` + `cacheTTL` |
| **预期** | 导航走缓存路径 |

### 4.5 缓存配置

#### TC-SDK-21: setCacheConfig / getCacheStats / clearElementCache

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **文件** | `client.ts` |
| **步骤** | 1. `client.setCacheConfig({ cacheTTL: 10000 })` 2. `client.getCacheStats()` → 验证 `defaultTtlMs: 10000` 3. `client.clearElementCache()` → 验证缓存为空 |
| **预期** | 三个缓存管理 API 工作正常 |

### 4.6 向后兼容性

#### TC-SDK-22: 旧代码不传 runtimeId 仍可工作

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **文件** | `element.ts` 所有方法 |
| **步骤** | 1. 构造 `ElementInfo` 不含 `runtimeId` 字段 2. 调用 `click()`, `type()`, `hover()`, `flash()` 等 3. 验证所有操作走 XPath 路径，功能正常 |
| **预期** | 无 runtimeId 时自动回退到 XPath 搜索，行为与优化前一致 |

#### TC-SDK-23: 构造函数的 cacheTTL 参数可选

| 项目 | 内容 |
|------|------|
| **优先级** | P2 |
| **文件** | `element.ts` 构造函数 |
| **验证** | `new Element(...)` 不传 `cacheTTL` → `this.cacheTTL === null` |
| **预期** | 构造函数签名向后兼容 |

---

## 五、性能测试

### TC-PERF-01: runtimeId 路径 vs XPath 路径性能对比

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **测试目标** | 量化缓存命中带来的性能提升 |
| **方法** | 对每个端点分别测试路径A和路径B，测量耗时 |
| **指标** | 路径A 耗时应在 1-10ms，路径B 应在 50-200ms |
| **报告** | 输出对比表格，计算加速比 |

### TC-PERF-02: waitUntilGone 轮询效率

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **测试目标** | 验证轮询不再每次全窗口搜索 |
| **方法** | 设置 waitUntilGone 100ms 轮询间隔，观察每次请求耗时 |
| **指标** | 每次轮询 < 5ms（vs 优化前 50-200ms） |

### TC-PERF-03: 批量操作性能

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **测试目标** | 连续 100 次 click 操作的性能 |
| **方法** | 循环调用 `el.click()`，测量总耗时 |
| **指标** | 总耗时应有显著改善（预期 100ms vs 优化前 5-20s） |

### TC-PERF-04: 缓存命中率统计

| 项目 | 内容 |
|------|------|
| **优先级** | P2 |
| **测试目标** | 典型 RPA 工作流中的缓存命中率 |
| **方法** | 执行完整的 RPA 场景（打开窗口→点击→输入→验证），统计 runtimeId 路径使用次数 |
| **指标** | 缓存命中率 > 80% |

---

## 六、异常测试

### TC-ERR-01: 缓存元素失效（UIElement 指向已销毁的控件）

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **场景** | 关闭目标窗口后通过 runtimeId 访问 |
| **预期** | 返回明确错误（如 "无法读取元素属性"），不 crash |

### TC-ERR-02: 窗口切换后 runtimeId 失效

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **场景** | 切换到不同窗口后使用旧 runtimeId |
| **预期** | 返回错误，不产生错误操作 |

### TC-ERR-03: 并发请求同一缓存 key

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **场景** | 10 个并发请求同时使用同一 runtimeId |
| **预期** | 无死锁，所有请求正常返回 |

### TC-ERR-04: 缓存满时的表现

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **场景** | 连续插入 600 个不同元素（超过 MAX=512） |
| **预期** | 旧元素被淘汰，新元素正常缓存，无 panic |

### TC-ERR-05: 空 runtimeId 字符串

| 项目 | 内容 |
|------|------|
| **优先级** | P2 |
| **场景** | 传入空字符串 `""` 作为 runtimeId |
| **预期** | 视为无 runtimeId，走 XPath 路径 |

### TC-ERR-06: 网络断开时 SDK 行为

| 项目 | 内容 |
|------|------|
| **优先级** | P2 |
| **场景** | 后端服务不可达时调用操作 |
| **预期** | SDK 抛出 `NetworkError`，有明确错误信息 |

### TC-ERR-07: 缓存配置非法值

| 项目 | 内容 |
|------|------|
| **优先级** | P2 |
| **场景** | `setCacheConfig({ cacheTTL: -1 })` |
| **预期** | 应有参数校验或后端忽略非法值 |

---

## 七、回归测试

### TC-REG-01: 完整 RPA 工作流（微信/QQ/记事本）

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **场景** | 打开应用 → 查找元素 → 点击 → 输入 → 读取文本 → 验证 |
| **预期** | 所有步骤正常完成，结果正确 |

### TC-REG-02: 现有单元测试全通过

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **方法** | 运行 `npx jest` 和 `cargo test` |
| **预期** | 所有已有测试通过，无新增失败 |

### TC-REG-03: wechat-rpa 场景回归

| 项目 | 内容 |
|------|------|
| **优先级** | P0 |
| **方法** | 运行 `d:\repos\uia-project\wechat-rpa\main.js` 完整流程 |
| **预期** | 微信 RPA 流程正常完成，操作速度明显提升 |

### TC-REG-04: XPath 优化器功能不变

| 项目 | 内容 |
|------|------|
| **优先级** | P1 |
| **方法** | 验证智能优化和极简优化策略仍正常工作 |
| **预期** | XPath 优化结果与优化前一致 |

---

## 八、测试执行计划

### 8.1 阶段划分

| 阶段 | 内容 | 预计耗时 |
|------|------|----------|
| **Phase 1: 单元测试** | TC-CORE-01 ~ TC-CORE-11 | 2h |
| **Phase 2: API 集成测试** | TC-API-01 ~ TC-API-18 | 3h |
| **Phase 3: SDK 端到端测试** | TC-SDK-01 ~ TC-SDK-23 | 4h |
| **Phase 4: 性能测试** | TC-PERF-01 ~ TC-PERF-04 | 2h |
| **Phase 5: 异常测试** | TC-ERR-01 ~ TC-ERR-07 | 2h |
| **Phase 6: 回归测试** | TC-REG-01 ~ TC-REG-04 | 2h |
| **总计** | | ~15h |

### 8.2 测试环境要求

| 环境 | 说明 |
|------|------|
| **操作系统** | Windows 10/11 (x64) |
| **Rust 工具链** | `rustc` + `cargo` (debug 模式) |
| **Node.js** | v18+ 用于 SDK 测试 |
| **测试应用** | 微信 (WeChat)、记事本 (Notepad)、计算器 (Calculator) |
| **后端服务** | `cargo run` 启动 HTTP 服务 |

### 8.3 优先级说明

| 级别 | 含义 | 阻塞发布？ |
|------|------|-----------|
| **P0** | 核心功能，必须通过 | 是 |
| **P1** | 重要功能，应该通过 | 是（需修复） |
| **P2** | 边缘情况，建议通过 | 否 |

---

## 九、测试工具与脚本建议

### 9.1 Rust 端测试

```rust
// 建议新增：src/core/element_cache_test.rs
#[cfg(test)]
mod element_cache_tests {
    use super::*;
    
    #[test]
    fn test_cache_insert_and_get() { /* TC-CORE-01 */ }
    
    #[test]
    fn test_cache_miss_returns_none() { /* TC-CORE-02 */ }
    
    #[test]
    fn test_lru_eviction() { /* TC-CORE-03 */ }
    
    #[test]
    fn test_ttl_expiry() { /* TC-CORE-06 */ }
    
    #[test]
    fn test_concurrent_access() { /* TC-CORE-11 */ }
}
```

### 9.2 SDK 端测试

```typescript
// 建议新增：src/__tests__/runtime-id-cache.test.ts
describe('RuntimeId Cache', () => {
    describe('Core Cache (Mock)', () => {
        it('should hit cache with valid runtimeId', () => {});
        it('should return error on cache miss (no fallback)', () => {});
        it('should expire after TTL', () => {});
        it('should support LRU eviction', () => {});
    });
    
    describe('Element Actions', () => {
        it('should pass runtimeId on click', () => {});
        it('should pass runtimeId on type', () => {});
        it('should pass runtimeId on hover', () => {});
        it('should pass runtimeId on flash', () => {});
    });
    
    describe('Element Find', () => {
        it('should use findFromElement with runtimeId', () => {});
        it('should fallback to XPath without runtimeId', () => {});
    });
    
    describe('Wait Operations', () => {
        it('should use refreshByRuntimeId for waitUntilGone', () => {});
        it('should use refreshByRuntimeId for waitFor', () => {});
    });
});
```

### 9.3 性能基准测试脚本

```bash
# 建议新增：scripts/benchmark_runtime_id.sh
# 对比 runtimeId 缓存路径 vs XPath 路径的性能
```

---

## 十、测试报告模板

### 测试结果汇总

| 测试类别 | 总数 | 通过 | 失败 | 阻塞 | 通过率 |
|----------|------|------|------|------|--------|
| 单元测试 (Core) | 11 | - | - | - | - |
| 集成测试 (API) | 18 | - | - | - | - |
| 端到端测试 (SDK) | 23 | - | - | - | - |
| 性能测试 | 4 | - | - | - | - |
| 异常测试 | 7 | - | - | - | - |
| 回归测试 | 4 | - | - | - | - |
| **总计** | **67** | - | - | - | - |

### 性能基准

| 指标 | 优化前 (XPath) | 优化后 (Cache) | 加速比 |
|------|---------------|---------------|--------|
| click 平均耗时 | 50-200ms | 1-3ms | 50-200x |
| refresh 平均耗时 | 50-200ms | 1-3ms | 50-200x |
| findElement 平均耗时 | 50-200ms | 5-15ms | 10-20x |
| waitUntilGone 轮询 | 50-200ms/次 | 1-3ms/次 | 50-200x |
