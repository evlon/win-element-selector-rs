---
name: enrich-uia-property-filters
overview: 丰富 UIA 属性过滤器种类，从当前4个（ControlType/AutomationId/ClassName/Name）扩展到更多 UIA 标准属性，以提升元素唯一匹配能力
todos:
  - id: extend-hierarchy-node
    content: 扩展 HierarchyNode 结构体，添加新字段和 build_extended_filters 方法
    status: completed
  - id: extract-uia-props
    content: 在 uia.rs element_to_node 中提取新 UIA 属性并调用 build_extended_filters
    status: completed
    dependencies:
      - extend-hierarchy-node
  - id: extend-uiauto-xpath
    content: 在 uiauto-xpath 中添加新属性访问器和 get_property 映射
    status: completed
  - id: update-optimizer
    content: 更新 xpath_optimizer 新属性的保留/禁用策略
    status: completed
    dependencies:
      - extend-hierarchy-node
---

## 产品概述

UIA 元素选择器中，当前属性面板仅展示 4 个属性（ControlType/AutomationId/ClassName/Name），部分场景下这 4 个属性不足以唯一匹配到目标元素，需要丰富可用属性的种类。

## 核心功能

- 扩展 HierarchyNode 结构体，添加更多 UIA 标准属性字段（IsPassword/AcceleratorKey/AccessKey/ItemType/ItemStatus）
- 在元素捕获阶段提取新的 UIA Current* 属性值
- 将已有但未加入 filters 的属性（FrameworkId/HelpText/LocalizedControlType/IsEnabled/IsOffscreen/IsPassword）自动加入属性过滤器列表
- 添加策略：有值时添加字符串属性，特殊值时添加布尔属性（IsPassword=true, IsEnabled=false, IsOffscreen=true）
- 在 uiauto-xpath 的 get_property() 中注册新属性，确保 XPath 查询和校验能正确匹配
- 优化器中为新属性添加合理的保留/禁用策略

## 技术栈

- 语言：Rust
- GUI：egui
- UIA 框架：windows-rs (Win32::UI::Accessibility)
- XPath 引擎：uiauto-xpath（本地依赖）
- 序列化：serde (Serialize/Deserialize)

## 实现方案

### 核心策略

在现有 HierarchyNode + PropertyFilter 架构上扩展，不引入新的设计模式。三处改动点：属性提取（uia.rs）、数据建模（model.rs）、XPath匹配（uiauto-xpath）。GUI 层无需改动——已有的 filters 动态渲染逻辑会自动展示新增的 filter 条目。

### 属性选择与过滤策略

| 属性 | 类型 | 添加条件 | 理由 |
| --- | --- | --- | --- |
| FrameworkId | String | 非空时 | 区分 Win32/WPF/Chrome 等框架 |
| HelpText | String | 非空时 | 辅助描述文本，区分度高 |
| LocalizedControlType | String | 非空时 | 本地化控件类型 |
| AcceleratorKey | String | 非空时 | 快捷键，区分度高 |
| AccessKey | String | 非空时 | 访问键，区分度高 |
| ItemType | String | 非空时 | 项目类型描述 |
| ItemStatus | String | 非空时 | 项目状态描述 |
| IsPassword | bool | true 时 | 密码框是强区分特征 |
| IsEnabled | bool | false 时 | 禁用状态有区分意义 |
| IsOffscreen | bool | true 时 | 离屏状态有区分意义 |


### 关键设计决策

1. **过滤器集中化**：将 FrameworkId/HelpText 的 filter 添加逻辑从 uia.rs::element_to_node() 迁移到 model.rs::HierarchyNode::new() 之后统一的 `build_extended_filters()` 方法中，避免逻辑分散。

2. **新增字段使用 #[serde(default)]**：保证旧版本序列化数据的反序列化兼容性。

3. **不添加 IsContentElement/IsControlElement/ProcessId**：前两者几乎所有控件都为 true，无区分度；ProcessId 已在窗口选择器中处理，不应出现在元素 XPath 中。

4. **优化器策略**：新属性默认在 optimize_node_filters() 中保留（enabled=true），因为它们本就是条件性添加的（非空/特殊值），已具备区分度。字符串属性中 Name 仍保留长度限制逻辑。

### 性能影响

- UIA 属性提取为单次 COM 调用，每个属性约 0.01-0.1ms，新增 8 个属性约增加 <1ms，对整体捕获时间（通常 50-200ms）影响可忽略。
- 过滤器列表从平均 4-5 项增加到 6-10 项，XPath 生成和校验开销微增但仍在 1ms 以内。

## 修改文件

### 目录结构

```
d:\repos\element-selector\src\core\model.rs        # [MODIFY] HierarchyNode 结构体扩展 + 过滤器构建逻辑
d:\repos\element-selector\src\core\uia.rs          # [MODIFY] element_to_node() 提取新属性
d:\repos\element-selector\src\core\xpath_optimizer.rs # [MODIFY] optimize_node_filters() 新属性策略
d:\repos\uiauto-xpath\src\element.rs               # [MODIFY] 新属性访问器 + get_property() 映射
```

### 详细说明

**model.rs** — HierarchyNode 扩展：

- 在 `is_offscreen` 字段后添加新字段：`is_password: bool`、`accelerator_key: String`、`access_key: String`、`item_type: String`、`item_status: String`，均带 `#[serde(default)]`
- 在 `new()` 中初始化新字段默认值
- 添加 `build_extended_filters(&mut self)` 方法，统一处理所有扩展属性的 filter 添加逻辑（含 FrameworkId/HelpText 的现有逻辑迁移 + 新属性逻辑）
- 字符串属性：非空时 `PropertyFilter::new()`；布尔属性：特殊值时 `PropertyFilter::new()`

**uia.rs** — element_to_node() 扩展：

- 提取 `CurrentIsPassword`、`CurrentAcceleratorKey`、`CurrentAccessKey`、`CurrentItemType`、`CurrentItemStatus`
- 赋值给 HierarchyNode 新字段
- 移除现有的 FrameworkId/HelpText filter 添加代码（已迁移到 model.rs 的 build_extended_filters）
- 调用 `node.build_extended_filters()` 替代

**xpath_optimizer.rs** — 新属性策略：

- 为 FrameworkId、HelpText、LocalizedControlType、AcceleratorKey、AccessKey、ItemType、ItemStatus 添加保留策略：非空时保留
- 为 IsPassword、IsEnabled、IsOffscreen 添加策略：保留（这些只在有区分度时才添加）
- 修改通配符分支 `_` 的默认行为：从 `enabled = false` 改为 `enabled = f.enabled`（保持原样），因为这些属性已经是条件性添加的

**uiauto-xpath/element.rs** — 新属性支持：

- 添加访问器方法：`is_password()`、`accelerator_key()`、`access_key()`、`item_type()`、`item_status()`、`localized_control_type()`
- 在 `get_property()` 中注册新属性名映射：`ispassword`、`acceleratorkey`、`accesskey`、`itemtype`、`itemstatus`、`localizedcontroltype`