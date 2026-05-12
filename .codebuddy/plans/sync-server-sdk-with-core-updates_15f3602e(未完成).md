---
name: sync-server-sdk-with-core-updates
overview: 根据 element-selector 核心层（model.rs）的属性扩展，同步更新 element-selector-server 的 API 类型定义和 Node.js SDK 的类型定义，确保新属性（isPassword, acceleratorKey, accessKey, itemType, itemStatus）从 UIA 捕获层到 HTTP API 响应再到 SDK 端完整传递。
todos:
  - id: extend-api-types
    content: 扩展 src/api/types.rs 中 ElementInfo 新增 5 个字段
    status: pending
  - id: update-uia-queries
    content: 更新 src/core/uia.rs 中 find_all_elements_detailed 填充新属性
    status: pending
    dependencies:
      - extend-api-types
  - id: fix-get-element
    content: 改造 src/api/element.rs 中 get_element 用真实 UIA 属性替代硬编码值
    status: pending
    dependencies:
      - extend-api-types
      - update-uia-queries
  - id: update-sdk-types
    content: 更新 sdk/nodejs/src/types.ts 中 ElementInfo 接口新增字段
    status: pending
    dependencies:
      - extend-api-types
  - id: build-verify
    content: 编译验证 Rust 和 SDK，确保无错误
    status: pending
    dependencies:
      - fix-get-element
      - update-sdk-types
---

## Product Overview

element-selector 核心库新增了 5 个 UIA 属性字段（is_password, accelerator_key, access_key, item_type, item_status）到 HierarchyNode 模型，需要将 server 端 API 响应类型 ElementInfo 和 Node.js SDK 类型同步扩展这些字段，并确保 API 层能从 UIA 元素中正确提取和返回这些新属性。

## Core Features

- 扩展 Rust API 响应类型 `ElementInfo` 新增 5 个字段（isPassword, acceleratorKey, accessKey, itemType, itemStatus）
- 扩展 Node.js SDK 类型 `ElementInfo` 新增对应 5 个字段
- 更新 `find_all_elements_detailed()` 从 UIA 元素提取新属性
- 改造 `get_element()` 从硬编码值改为从 UIA 元素获取真实属性
- SDK fluent-chain 可选增强：基于新属性的断言方法

## Tech Stack

- 后端: Rust + actix-web (现有项目)
- SDK: TypeScript + axios (现有项目)
- 构建: Cargo (Rust), tsc (TypeScript)

## Implementation Approach

### 核心策略

1. **Rust API 层**：扩展 `ElementInfo` 结构体，添加新字段并使用 `#[serde(default)]` 确保向后兼容；修改 UIA 查询函数从元素对象提取新属性值
2. **SDK 层**：在 `types.ts` 中扩展 `ElementInfo` 接口，新字段设为可选（`?`），保持与旧版本 server 的兼容性
3. **数据流改造**：`get_element()` 当前只获取 `DetailedValidationResult`（仅含 rect），需增加一步 UIA 查询来获取元素完整属性；`find_all_elements_detailed()` 已持有 UIA 元素引用，直接读取新属性

### 关键技术决策

- **向后兼容**：Rust 新字段使用 `#[serde(default)]`，TypeScript 新字段使用 `?` 可选标记，确保旧版 SDK/客户端仍可正常工作
- **get_element() 改造方式**：在 `validate_selector_and_xpath_detailed` 返回找到元素后，需额外调用 `find_all_elements_detailed` 或复用 UIA 查询来获取元素属性，而非仅使用硬编码值。推荐方案：将 `validate_selector_and_xpath_detailed` 的结果中增加首元素的属性信息，或让 `get_element` 在 validation 成功后额外查询一次元素属性
- **避免重复查询**：`get_element` 可复用 `find_all_elements_detailed` 的逻辑，先 validate 获取 rect，再通过 XPath 查询获取完整属性，减少代码重复

## Implementation Notes

- `get_element()` 当前硬编码 `control_type: "Element"`, `name: ""`, `is_enabled: true`，必须改为从 UIA 获取真实值
- `find_all_elements_detailed()` 已有 UIA 元素引用 `elem`，可直接调用 `elem.CurrentIsPassword()` 等方法获取新属性
- UIA COM 调用必须注意错误处理：每个 `Current*` 方法可能失败，需 `unwrap_or_default` 降级
- SDK 编译后需检查 `dist/` 输出的 `.d.ts` 文件是否正确包含新类型

## Architecture Design

无需架构变更，在现有分层架构上扩展即可：

- `core/model.rs`（HierarchyNode）-> 已完成扩展
- `api/types.rs`（ElementInfo 响应类型）-> 本次扩展
- `core/uia.rs`（数据查询）-> 本次扩展属性读取
- `sdk/types.ts`（SDK 类型）-> 本次同步扩展

## Directory Structure

```
d:/repos/element-selector/
├── src/api/types.rs               # [MODIFY] ElementInfo 新增 5 个字段 + From impl 更新
├── src/core/uia.rs                # [MODIFY] find_all_elements_detailed() 填充新属性；新增 get_element_info() 辅助函数
├── src/api/element.rs             # [MODIFY] get_element() 改用真实 UIA 属性替代硬编码值
├── sdk/nodejs/src/types.ts        # [MODIFY] ElementInfo 接口新增 5 个可选字段
├── sdk/nodejs/src/v2/fluent-chain.ts  # [MODIFY] (可选) 新增 assertPassword/assertEnabled 等增强断言
└── sdk/nodejs/src/index.ts        # [MODIFY] (可选) 导出新类型
```

## Key Code Structures

```rust
// src/api/types.rs - 扩展后的 ElementInfo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementInfo {
    pub rect: Rect,
    pub center: Point,
    pub center_random: Point,
    pub control_type: String,
    pub name: String,
    #[serde(rename = "isEnabled")]
    pub is_enabled: bool,
    // 新增字段
    #[serde(rename = "isPassword", default)]
    pub is_password: bool,
    #[serde(rename = "acceleratorKey", default)]
    pub accelerator_key: String,
    #[serde(rename = "accessKey", default)]
    pub access_key: String,
    #[serde(rename = "itemType", default)]
    pub item_type: String,
    #[serde(rename = "itemStatus", default)]
    pub item_status: String,
}
```

```typescript
// sdk/nodejs/src/types.ts - 扩展后的 ElementInfo
export interface ElementInfo {
    rect: Rect;
    center: Point;
    centerRandom: Point;
    controlType: string;
    name: string;
    isEnabled: boolean;
    // 新增字段（可选，兼容旧版 server）
    isPassword?: boolean;
    acceleratorKey?: string;
    accessKey?: string;
    itemType?: string;
    itemStatus?: string;
}
```