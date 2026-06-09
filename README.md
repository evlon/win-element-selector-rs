# element-selector

Windows UI Automation 元素捕获与 XPath 定位规则编辑工具。
使用 **Rust + Iced** 构建，零外部运行时依赖。

---

## 功能清单

| 功能 | 说明 |
|------|------|
| **F4 捕获** | 进入等待状态，点击屏幕任意控件，自动解析 IUIAutomation 层级树 |
| **F7 校验** | 用当前 XPath 在屏幕上查找元素，高亮闪烁结果 |
| **层级树** | 完整祖先链 Root→Target，支持逐节点点击、右键菜单排除节点 |
| **属性编辑** | AutomationId / ClassName / Name / Index，运算符下拉（等于/不等于/包含/开头为/结尾为/正则） |
| **XPath 生成** | 自动生成 / 精简模式 / 智能优化 / 极简优化 / Leaf-First |
| **自定义 XPath** | 手动编辑，带语法 lint（括号匹配、格式检查） |
| **元素高亮** | 捕获和校验时在目标元素上闪烁红色边框（透明覆盖窗口，两阶段高亮优化） |
| **历史记录** | 最多保留 20 条 XPath 历史，下拉复用 |
| **配置持久化** | 精简模式开关、历史记录跨会话保存 |
| **节点 Tooltip** | 悬停显示完整属性 + 屏幕坐标 |
| **右键菜单** | 节点排除/高亮操作 |
| **多模式定位** | Fast(ControlView) / Full(RawView) / FastChild / FullChild |
| **深度限制语法** | `/*n/` 限制搜索深度（n=1~N） |
| **position() 支持** | `position()=1` / `position()=N` / `position()=last()` 标准 XPath 1.0 |

---

## 构建

### 环境要求

- Rust 1.75+（`rustup update stable`）
- Windows 10/11（完整 UIA 功能）
- 其他平台：正常编译，捕获使用 mock 数据

### 开发构建

```bash
cargo run
```

### 发布构建（最小体积，无控制台窗口）

```bash
cargo build --release
# → target/release/element-selector.exe
```

### 跨平台编译检查

```bash
# 在 Linux/macOS 验证编译通过（mock 模式）
cargo check
cargo test
```

---

## 架构说明

```
src/
├── main.rs              入口：COM MTA 初始化，iced 启动
├── core/
│   ├── model.rs         数据模型：HierarchyNode / PropertyFilter / CaptureMode / LocateMode
│   ├── commonality.rs   公共类型与常量
│   ├── xpath.rs         XPath 生成 / 精简 / lint / Leaf-First
│   └── uia/
│       ├── mod.rs       模块入口，re-export 所有子模块
│       ├── helpers.rs   AutomationProvider + 工具函数
│       ├── capture.rs   捕获：ElementFromPoint + 两阶段高亮
│       ├── validation.rs 校验：SearchContext + NotFoundReason
│       ├── window.rs    窗口查找/激活/枚举
│       ├── element.rs   invoke/focus/set_value
│       ├── cache.rs     ParsedXPathStep + XPath 缓存
│       ├── find.rs      主调度：策略分发 + 共享解析函数
│       ├── find_control.rs  ControlView 搜索：Chain FindFirst/FindAll + uiauto-xpath
│       ├── find_raw.rs  RawView 搜索：Chain FindFirst/FindAll
│       ├── navigation.rs    导航：方向/可见元素查找
│       ├── inspect.rs   检查：子树结构分析
│       └── visibility.rs    可见性计算
├── gui/
│   ├── iced_app.rs      Iced 应用主逻辑
│   └── highlight.rs     高亮覆盖窗口（SetWindowPos 移动优化）
└── api/                 HTTP API 端点（Actix-web server binary）
    ├── element.rs       元素查找/操作
    ├── mouse.rs         鼠标/滚动/空闲
    ├── keyboard.rs      键盘输入
    ├── window.rs        窗口操作
    └── types.rs         API 数据类型
```

### 关键设计决策

#### COM 线程模型（v2.0.0 最新架构）
采用 **后台 MTA 线程** 架构：
- 主线程运行 Iced UI（OleInitialize STA）
- 专用的 `uia-mta-init` 后台线程初始化 COM MTA
- `AutomationProvider` 使用 `UIAutomation::new_direct()` 获取自动化实例
- 所有 UIA 操作通过 `AutomationProvider::get_healthy()` 获取实例

#### XPath 定位模式
| 前缀 | 定位模式 | Walker | 说明 |
|------|----------|--------|------|
| `[fast]` | Fast | ControlView | 默认，快速定位 |
| `[full]` | Full | RawView | 完整元素树 |
| `[fast-child]` | FastChild | ControlView | 跨进程子窗口 |
| `[full-child]` | FullChild | RawView | 跨进程子窗口(完整) |

#### 链式 FindFirst 优化
多层 descendant XPath（`//A//B//C`）逐层调用 `FindFirst(Subtree)`，避免 `FindAll(Subtree)` 遍历整棵子树。支持 `position()` 谓词加速：
- `position()=1` → `FindFirst`（最快）
- `2 <= position()=N <= FINDFIRST_NEXT_MAX_N` → `FindAll` + 取第 N 个
- `position()=last()` 或 N > MAX_N → 降级到 uiauto-xpath 引擎

#### XPath position() 标准语法
使用标准 XPath 1.0 语法替代自定义函数：
- `first()` → `position()=1`
- `last()` → `position()=last()`
- `position()=N` 保持不变

---

## HTTP API 端点

服务端监听 `127.0.0.1:8080`，提供 RESTful API 供 SDK 消费。

### 元素查找

```
POST /api/element
```

| 参数 | 类型 | 说明 |
|------|------|------|
| `window` | string | 窗口选择器 XPath |
| `element` | string | 元素 XPath |
| `randomRange` | number | 随机坐标范围（默认 0.55） |

**响应**（`ElementInfo` 属性扁平化到顶层，`elementSelector` 独立）：
```json
{
  "found": true,
  "elementSelector": "//Button[@Name='发送']",
  "rect": {"x": 0, "y": 0, "width": 60, "height": 30},
  "center": {"x": 30, "y": 15},
  "controlType": "Button",
  "name": "发送",
  ...
}
```

### 多元素查找

```
POST /api/element/all
```

参数同上，返回 `elements` 数组，每个元素含 `elementSelector` + 扁平化 `ElementInfo` 属性。

### 鼠标操作

```
POST /api/mouse/click
```

| 参数 | 类型 | 说明 |
|------|------|------|
| `window` | string/object | 窗口选择器 |
| `element` | string | 元素 XPath |
| `options` | object | 点击选项（button, humanize, randomRange, clickArea） |

```
POST /api/mouse/scroll
```

| 参数 | 类型 | 说明 |
|------|------|------|
| `window` | string | 窗口选择器（可选，默认 `"Window"`） |
| `element` | string | 要滚动的容器元素 XPath |
| `delta` | number | 滚动量（正值向上，负值向下，默认 120） |
| `times` | number | 最大滚动次数（默认 3） |
| `autoScrollAmount` | bool | 自动根据容器高度计算滚动量（默认 false） |
| `scrollAmountRatio` | number | 自动滚动量比率 |
| `scrollToCenter` | bool | 滚动到视口中心 |
| `centerAdjustTimes` | number | 居中调整最大次数（默认 5） |
| `scrollInterval` | number | 滚动间隔毫秒 |
| `autoScrollDelay` | number | 自动滚动延迟 |
| `minScrollRatio` | number | 最小滚动比率 |
| `centerSnapThreshold` | number | 居中吸附阈值 |
| `viewportInset` | object | 视口内边距 `{ top, bottom, left, right }` |
| `smoothStepDelta` | number | 平滑滚动步长（0=禁用，与 autoScrollAmount 互斥） |
| `wait` | string | 等待目标元素出现的 XPath |
| `waitMode` | string | `"visible"` 或 `"exist"`（默认 exist） |
| `timeout` | number | 超时毫秒数（默认 5000） |

```
POST /api/mouse/move       → 鼠标移动
POST /api/mouse/scroll     → 鼠标滚动
POST /api/mouse/idle/start → 启动空闲移动
POST /api/mouse/idle/stop  → 停止空闲移动
GET  /api/mouse/idle/status → 空闲移动状态
```

### 元素操作

```text
POST /api/element/invoke    → 调用元素
POST /api/element/focus     → 聚焦元素
POST /api/element/set-value → 设置值
POST /api/element/flash     → 闪烁高亮
POST /api/element/inspect   → 检查子树
POST /api/element/visibility → 可见性检查
POST /api/element/find-from → 从元素查找
POST /api/element/refresh   → 刷新缓存
```

### 缓存管理

```text
GET  /api/cache/stats       → 缓存统计
POST /api/cache/clear       → 清除缓存
POST /api/cache/config      → 缓存配置
GET  /api/cache/xpath/stats → XPath 缓存统计
POST /api/cache/xpath/clear → 清除 XPath 缓存
```

### 键盘操作

```
POST /api/keyboard/type     → 输入文本
POST /api/keyboard/shortcut → 快捷键组合
POST /api/keyboard/key      → 单个按键
```

### 窗口操作

```
POST /api/window/list          → 窗口列表
POST /api/window/activate      → 激活窗口
POST /api/window/focus-element → 聚焦元素
```

### 健康检查

```
GET /api/health → {"status": "ok", "version": "1.0.0"}
```

---

## 扩展方向

- [ ] `IUIAutomationCondition` 原生条件组合替代 post-filter（性能更好）
- [ ] 支持更多 UIA 属性：`IsEnabled`、`IsKeyboardFocusable`、`RuntimeId`
- [ ] 鼠标悬停实时预览（hover 捕获模式，不需要点击）
- [ ] 导出定位配置（JSON / CSV）
- [ ] 深色/浅色主题切换
- [ ] 多监视器 DPI 感知（`SetProcessDpiAwarenessContext`）
- [ ] Leaf-First XPath 执行层优化（当前仅生成，搜索引擎尚未支持 ancestor 轴）
- [ ] `FINDFIRST_NEXT_MAX_N` 可通过 API 动态配置
