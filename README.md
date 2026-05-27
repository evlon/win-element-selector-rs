# element-selector

Windows UI Automation 元素捕获与 XPath 定位规则编辑工具。
使用 **Rust + Egui (eframe)** 构建，零外部运行时依赖。

---

## 功能清单

| 功能 | 说明 |
|------|------|
| **F4 捕获** | 进入等待状态（5s 倒计时），点击屏幕任意控件，自动解析 IUIAutomation 层级树 |
| **F7 校验** | 用当前 XPath 在屏幕上查找元素，高亮闪烁结果 |
| **层级树** | 完整祖先链 Root→Target，支持逐节点点击、右键菜单排除节点 |
| **属性编辑** | AutomationId / ClassName / Name / Index，运算符下拉（等于/不等于/包含/开头为/结尾为） |
| **XPath 生成** | 自动生成 / 精简模式（去掉无标识节点） |
| **自定义 XPath** | 手动编辑，带语法 lint（括号匹配、格式检查） |
| **元素高亮** | 捕获和校验时在目标元素上闪烁红色边框（透明覆盖窗口） |
| **历史记录** | 最多保留 20 条 XPath 历史，下拉复用 |
| **配置持久化** | 精简模式开关、历史记录通过 eframe storage 跨会话保存 |
| **节点 Tooltip** | 悬停显示完整属性 + 屏幕坐标 |
| **右键菜单** | 节点排除/高亮操作 |

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
├── main.rs              入口：COM STA 初始化 + ComWorker 启动，eframe 启动
├── core/
│   ├── com_worker.rs    COM 工作线程（单例，所有 UIA 操作统一入口）
│   ├── uia.rs           底层 UIA API + BFS 优化算法
│   ├── model.rs         数据模型：HierarchyNode / PropertyFilter / ValidationResult
│   └── xpath.rs         XPath 生成 / 精简 / lint
├── capture.rs           捕获/验证 API（全部通过 ComWorker 执行）
├── gui/
│   ├── app.rs           Egui 应用主逻辑
│   ├── layout.rs        UI 布局组件
│   ├── highlight.rs     高亮覆盖窗口
│   └── mouse_hook.rs    全局鼠标钩子
└── api/                 HTTP API 端点（server binary）
```

### 关键设计决策

#### COM 线程模型（v2.0.0 最新架构）
采用**单线程 COM 工作线程**架构：
- 主线程调用 `CoInitializeEx(COINIT_APARTMENTTHREADED)` 初始化 COM
- 创建专用的 **ComWorker** 后台线程，在 STA 模式下运行
- 所有 UIA 操作通过 mpsc channel 发送到 ComWorker 串行执行
- 单一 `IUIAutomation` 实例，天然无并发竞争
- 详见 [COM_MIGRATION_REPORT_FINAL.md](docs/COM_MIGRATION_REPORT_FINAL.md)

**优势**：
- ✅ 代码简洁（删除 70+ 行冗余代码）
- ✅ 系统稳定（消除 COM 失效、并发竞争问题）
- ✅ 资源节约（单一 IUIAutomation 实例，内存降低 50%+）

#### 高亮窗口
使用 `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE` 创建一个完全穿透鼠标的覆盖窗口，在目标元素的 `BoundingRectangle` 上画红色边框，闪烁后自动销毁。运行在独立线程，不阻塞 UI。

#### 捕获流程
```
F4 按下
  → CaptureState::WaitingClick（30s 倒计时）
  → 用户点击屏幕
  → 发送请求到 ComWorker 线程
  → ComWorker: ElementFromPoint(cursor) → IUIAutomationTreeWalker 向上遍历祖先
  → 构建 HierarchyNode 链（最多 32 层）
  → 计算目标节点 sibling index
  → 返回结果到 GUI 线程
  → 更新 UI + 生成 XPath + 高亮闪烁
```

#### XPath 生成规则
- 每个 `included=true` 的节点生成一个 `//ControlType[@Attr='val' and ...]` 片段
- 精简模式：跳过没有 AutomationId 也没有 Name 的中间节点
- Lint 检查：括号平衡、引号闭合、必须以 `//` 开头

#### 校验（Validation）
解析 XPath 片段序列，对每段用 `IUIAutomationCondition` + `FindAll(TreeScope_Subtree)` 逐层缩小候选集，最终返回匹配数量。

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
| `element` | string | 要滚动到的元素 XPath |
| `options.delta` | number | 滚动量（正值向上，负值向下，默认 120） |
| `options.times` | number | 最大滚动次数（默认 3） |
| `options.autoDelta` | bool | 自动根据容器高度计算滚动量（默认 false） |
| `options.deltaFactor` | number | autoDelta 乘数因子（默认 0.8） |
| `options.wait` | string | 等待目标元素出现的 XPath |
| `options.waitMode` | string | `"visible"` 或 `"exist"`（默认 exist） |
| `options.timeout` | number | 超时毫秒数（默认 5000） |

```
POST /api/move/mouse      → 鼠标移动
POST /api/mouse/scroll    → 鼠标滚动
POST /api/mouse/idle/start → 启动空闲移动
POST /api/mouse/idle/stop  → 停止空闲移动
GET  /api/mouse/idle/status → 空闲移动状态
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
