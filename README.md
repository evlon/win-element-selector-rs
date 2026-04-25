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
├── main.rs        入口：COM STA 初始化，eframe 启动
├── error.rs       统一错误类型（thiserror）
├── model.rs       数据模型：HierarchyNode / PropertyFilter / Operator / ValidationResult
├── capture.rs     UI Automation 捕获（Windows: IUIAutomation COM；其他: mock）
├── highlight.rs   高亮覆盖窗口（Windows: WS_EX_LAYERED click-through window；其他: stub）
├── xpath.rs       XPath 生成 / 精简 / lint，含单元测试
└── app.rs         全部 Egui 界面逻辑
```

### 关键设计决策

#### COM 线程模型
`CoInitializeEx(COINIT_APARTMENTTHREADED)` 在 main 线程调用一次。
`IUIAutomation` 实例通过 `thread_local!` 懒初始化，保证始终在同一 STA 线程使用，避免跨线程 COM 调用。

#### 高亮窗口
使用 `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE` 创建一个完全穿透鼠标的覆盖窗口，在目标元素的 `BoundingRectangle` 上画红色边框，闪烁后自动销毁。运行在独立线程，不阻塞 UI。

#### 捕获流程
```
F4 按下
  → CaptureState::WaitingClick（5s 倒计时）
  → 用户点击屏幕
  → ElementFromPoint(cursor) → IUIAutomationTreeWalker 向上遍历祖先
  → 构建 HierarchyNode 链（最多 32 层）
  → 计算目标节点 sibling index
  → 更新 UI + 生成 XPath + 高亮闪烁
```

#### XPath 生成规则
- 每个 `included=true` 的节点生成一个 `//ControlType[@Attr='val' and ...]` 片段
- 精简模式：跳过没有 AutomationId 也没有 Name 的中间节点
- Lint 检查：括号平衡、引号闭合、必须以 `//` 开头

#### 校验（Validation）
解析 XPath 片段序列，对每段用 `IUIAutomationCondition` + `FindAll(TreeScope_Subtree)` 逐层缩小候选集，最终返回匹配数量。

---

## 扩展方向

- [ ] `IUIAutomationCondition` 原生条件组合替代 post-filter（性能更好）
- [ ] 支持更多 UIA 属性：`IsEnabled`、`IsKeyboardFocusable`、`RuntimeId`
- [ ] 鼠标悬停实时预览（hover 捕获模式，不需要点击）
- [ ] 导出定位配置（JSON / CSV）
- [ ] 深色/浅色主题切换
- [ ] 多监视器 DPI 感知（`SetProcessDpiAwarenessContext`）
