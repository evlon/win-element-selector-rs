---
name: sync-server-sdk-full-properties
overview: 将 HierarchyNode 的所有有意义的动态属性完整暴露到 API 的 ElementInfo 和 SDK 的 ElementInfo，同时修复 get_element() 的硬编码问题，改用 find_all_elements_detailed 获取真实 UIA 属性。不需要向后兼容。
todos:
  - id: extend-api-types
    content: 扩展 src/api/types.rs 中 ElementInfo 新增 12 个字段
    status: completed
  - id: update-uia-queries
    content: 更新 src/core/uia.rs 中 find_all_elements_detailed 填充所有新属性
    status: completed
    dependencies:
      - extend-api-types
  - id: fix-get-element
    content: 改造 src/api/element.rs 中 get_element 用 find_all_elements_detailed 替代硬编码值
    status: completed
    dependencies:
      - extend-api-types
      - update-uia-queries
  - id: update-sdk-types
    content: 更新 sdk/nodejs/src/types.ts 中 ElementInfo 接口新增 12 个字段
    status: completed
    dependencies:
      - extend-api-types
  - id: build-verify
    content: 编译验证 Rust 和 SDK，确保无错误
    status: completed
    dependencies:
      - fix-get-element
      - update-sdk-types
---

## 产品概述

element-selector 核心库 HierarchyNode 已扩展了完整的 UIA 属性，需要将 server 端 API 响应类型 ElementInfo 和 Node.js SDK 类型同步扩展，覆盖所有有意义的动态属性，并修复 get_element() 使用硬编码值的问题。

## 核心特性

- 扩展 Rust API 响应类型 ElementInfo，新增 12 个字段（automationId, className, frameworkId, helpText, localizedControlType, isOffscreen, isPassword, acceleratorKey, accessKey, itemType, itemStatus, processId）
- 扩展 Node.js SDK 类型 ElementInfo，新增对应 12 个字段（无需可选标记，不向后兼容）
- 改造 get_element() 从硬编码值改为调用 find_all_elements_detailed 获取真实 UIA 属性
- 更新 find_all_elements_detailed() 填充所有新属性

## 技术栈

- 后端: Rust + actix-web（现有）
- SDK: TypeScript + axios（现有）
- 构建: Cargo (Rust), tsc (TypeScript)

## 实现方案

### 核心策略

1. **Rust API 层**：扩展 `ElementInfo` 结构体，新增 12 个字段；不使用 `#[serde(default)]`，直接要求完整数据
2. **get_element() 改造**：复用 `find_all_elements_detailed()` 获取第一个元素的完整属性，替代当前 `validate_selector_and_xpath_detailed()` + 硬编码值的方案。这样只需一次 UIA 查询即可获得 rect + 所有属性
3. **SDK 层**：在 `types.ts` 中扩展 `ElementInfo` 接口，所有新字段为必填（不向后兼容）

### 关键技术决策

- **get_element() 复用 find_all_elements_detailed**：当前 `validate_selector_and_xpath_detailed()` 只返回 `DetailedValidationResult`（仅含 rect 和 count），无法获取元素属性。而 `find_all_elements_detailed()` 已有完整的 UIA 元素引用和属性提取逻辑。改为调用 `find_all_elements_detailed()` 取 limit=1 的结果，即可同时得到 rect 和所有属性，消除硬编码
- **不加 serde(default)**：用户明确不需要向后兼容，新字段全部为必填
- **排除内部属性**：index, acc_role, filters, included, is_target, position_mode, sibling_count, depth_from_window 属于 XPath 内部逻辑专用，不暴露到 API

## 实现注意事项

- `find_all_elements_detailed()` 当前已从 UIA 元素读取 `control_type`, `name`, `is_enabled`，需要补充其余 12 个属性的读取
- `get_element()` 改造后，`ElementResponse` 中的 `ElementInfo` 将包含完整属性，SDK 端断言方法（如 assertEnabled）可直接使用新属性
- UIA COM 调用错误处理：每个 `Current*` 方法可能失败，需 `unwrap_or_default` 降级

## 架构设计

无需架构变更，在现有分层架构上扩展：

- `core/model.rs`（HierarchyNode）-> 已完成扩展
- `api/types.rs`（ElementInfo 响应类型）-> 本次扩展对齐 HierarchyNode 的公共属性
- `core/uia.rs`（find_all_elements_detailed 填充新属性）-> 本次扩展
- `api/element.rs`（get_element 改用 find_all_elements_detailed）-> 本次改造
- `sdk/types.ts`（ElementInfo 类型）-> 本次同步扩展

## 目录结构

```
d:/repos/element-selector/
├── src/api/types.rs               # [MODIFY] ElementInfo 新增 12 个字段
├── src/core/uia.rs                # [MODIFY] find_all_elements_detailed() 填充所有新属性
├── src/api/element.rs             # [MODIFY] get_element() 改用 find_all_elements_detailed 获取真实属性
├── sdk/nodejs/src/types.ts        # [MODIFY] ElementInfo 接口新增 12 个字段
└── sdk/nodejs/dist/               # [REBUILD] tsc 重新编译
```

## 关键代码结构

```rust
// src/api/types.rs - 扩展后的 ElementInfo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementInfo {
    pub rect: Rect,
    pub center: Point,
    #[serde(rename = "centerRandom")]
    pub center_random: Point,
    #[serde(rename = "controlType")]
    pub control_type: String,
    pub name: String,
    #[serde(rename = "automationId")]
    pub automation_id: String,
    #[serde(rename = "className")]
    pub class_name: String,
    #[serde(rename = "frameworkId")]
    pub framework_id: String,
    #[serde(rename = "helpText")]
    pub help_text: String,
    #[serde(rename = "localizedControlType")]
    pub localized_control_type: String,
    #[serde(rename = "isEnabled")]
    pub is_enabled: bool,
    #[serde(rename = "isOffscreen")]
    pub is_offscreen: bool,
    #[serde(rename = "isPassword")]
    pub is_password: bool,
    #[serde(rename = "acceleratorKey")]
    pub accelerator_key: String,
    #[serde(rename = "accessKey")]
    pub access_key: String,
    #[serde(rename = "itemType")]
    pub item_type: String,
    #[serde(rename = "itemStatus")]
    pub item_status: String,
    #[serde(rename = "processId")]
    pub process_id: u32,
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
    automationId: string;
    className: string;
    frameworkId: string;
    helpText: string;
    localizedControlType: string;
    isEnabled: boolean;
    isOffscreen: boolean;
    isPassword: boolean;
    acceleratorKey: string;
    accessKey: string;
    itemType: string;
    itemStatus: string;
    processId: number;
}
```