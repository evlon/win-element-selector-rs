# 项目记忆

## 项目结构
- **win-element-selector-rs**: Rust 后端，提供 Windows UI 自动化 HTTP API（actix-web）
- **win-element-selector-sdk**: Node.js/TypeScript SDK，封装后端 API

## SDK 关键设计决策
- 元素属性方法（isVisible/isEnabled 等）使用本地缓存，`refresh()` 是唯一主动刷新方法
- `el.selector` — 原始查询 XPath（用户传入的）
- `el.listSelector` — 元素所属集合的选择器（后端 `elementSelector` 字段映射而来）
- `el.windowSelector` — 窗口选择器
- `findOne` 匹配多个报错，`findFirst` 容许多个匹配，`find` 是 `findOne` 别名（deprecated）
- `dist/` 由 `npm run build` 生成，不手动编辑

## scrollIntoView 修复历程
1. **Round 1**: `scrollIntoView` 用 `listSelector` 作 `wait`，匹配所有 14 个按钮，`is_offscreen` 只查第一个（已可见）→ 改用 `buildXpathFromProps()` 生成唯一 XPath
2. **Round 2**: 后端 `scrollMouse` 硬编码 `"Window"` 选择器，遍历 29 个窗口；`buildXpathFromProps` 生成重复谓词 → 添加 `window` 参数 + `parseExistingAttrs()` 去重
3. **Round 3**: `auto_delta` 计算丢失原始 delta 符号（-120→+444，方向反转）→ 保留 `delta.signum()` 符号

## 用户偏好
- 使用 debug 模式开发和测试，除非说发布才用 release 编译
- 使用中文沟通
