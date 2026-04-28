# Element Selector SDK V2 设计文档

## 设计目标

**核心理念**: 流式 XPath 自动化，简单直接，失败自动截图退出，方便人工/Agent介入。

## 目标代码样式

```typescript
// 极简示例
await sdk
    .humanize()                             // 开启拟人化
    .window("微信")
    .find("//Edit[@Name='输入']")           // 找不到自动截图退出
    .click()
    .type("你好")
    .run();
```

---

## 场景示例

### 1. 微信发送消息 - 简单拟人化

```typescript
await sdk
    .humanize()
    .window("微信")
    .find("//Edit[@Name='输入']")
    .click()
    .type("你好，这是一条测试消息")
    .wait(500, 1500)                         // 随机等待 500-1500ms
    .find("//Button[@Name='发送']")
    .click()
    .run();
```

### 2. 记事本编辑 - 精确控制

```typescript
await sdk
    .window({ title: "*.txt - 记事本", processName: "Notepad" })
    .humanize({ speed: 'slow' })
    .find("//Document")
    .click()
    .type("第一行内容\n")
    .wait(300)
    .type("第二行内容\n")
    .shortcut("Ctrl+S")
    .run();
```

### 3. 多分支选择 - 备用路径

```typescript
await sdk
    .humanize()
    .window({ className: "Chrome_WidgetWin_1" })
    .tryFind("//Button[@Name='登录']")
        .orElse("//Button[@AutomationId='loginBtn']")
        .orElse("//Button[contains(@Name, '登录')]")
    .click()
    .run();
```

### 4. 空闲移动 + 自动化

```typescript
await sdk
    .idle({
        window: "微信",
        xpath: "//Pane[@ClassName='ChatView']",
        speed: 'normal'
    })
    .humanize()
    .window("微信")
    .find("//Edit[@Name='输入']")
    .click()
    .type("自动回复")
    .find("//Button[@Name='发送']")
    .click()
    .stopIdle()
    .run();
```

### 5. 条件判断

```typescript
await sdk
    .humanize()
    .window("微信")
    .ifFind("//Text[@Name='新消息']")
        .click()
        .then()
    .find("//Edit[@Name='输入']")
    .click()
    .type("回复内容")
    .run();
```

### 6. 元素信息查询

```typescript
const info = await sdk
    .window("微信")
    .find("//Edit[@Name='输入']")
    .inspect();

console.log(info.name);           // "输入"
console.log(info.className);      // "Edit"
console.log(info.rect);           // { x: 100, y: 200, width: 300, height: 50 }
console.log(info.isEnabled);      // true
console.log(info.value);          // "当前内容"
```

### 7. 等待策略

```typescript
await sdk
    .humanize()
    .window("Chrome")
    .waitFor("//Button[@Name='登录']", { timeout: 10000 })
    .click()
    .waitUntilGone("//Spinner[@Name='加载中']", { timeout: 5000 })
    .waitForText("//Text[@AutomationId='status']", "登录成功")
    .run();
```

### 8. 数据提取

```typescript
// 表格数据
const tableData = await sdk
    .window("Excel")
    .find("//Table")
    .extractTable();
// [["姓名", "年龄"], ["张三", "25"]]

// 列表数据
const listItems = await sdk
    .window("微信")
    .find("//List[@ClassName='ContactList']")
    .extractList();
// ["张三", "李四"]

// 多元素属性
const buttons = await sdk
    .window("微信")
    .findAll("//Button")
    .extract(['name', 'automationId', 'rect']);
```

### 9. 断言验证

```typescript
await sdk
    .humanize()
    .window("微信")
    .assertExists("//Edit[@Name='输入']")
    .assertNotExists("//Button[@Name='旧按钮']")
    .assertText("//Text[@AutomationId='title']", "微信")
    .assertVisible("//Button[@Name='发送']")
    .run();
```

### 10. 高级鼠标操作

```typescript
await sdk
    .humanize()
    .window("Excel")
    .find("//Cell[@Name='A1']")
    .doubleClick()
    .type("100")
    .find("//Cell[@Name='B1']")
    .rightClick()
    .wait(500)
    .find("//MenuItem[@Name='复制']")
    .click()
    .dragTo("//Cell[@Name='C5']")
    .scrollTo("//Cell[@Name='Z100']")
    .run();
```

### 11. 截图功能

```typescript
// 整屏截图
await sdk.screenshot("全屏.png");

// 窗口截图
await sdk.window("微信").screenshotWindow("微信.png");

// 元素截图
await sdk
    .window("微信")
    .find("//Edit")
    .screenshotElement("输入框.png");

// 自动命名
const path = await sdk.screenshotAuto();
// "screenshots/2024-04-28_15-30-45.png"
```

### 12. 多窗口操作

```typescript
const windows = await sdk.listWindows();

await sdk
    .switchTo("微信")
    .find("//Edit")
    .click()
    .switchTo("Chrome")
    .find("//Input")
    .click()
    .run();

await sdk.waitForWindow("新对话", { timeout: 5000 });
await sdk.closeWindow("临时窗口");
```

### 13. 重试机制

```typescript
await sdk
    .humanize()
    .window("Chrome")
    .retry(3, 2000)                         // 失败重试3次，间隔2秒
    .find("//Button[@Name='提交']")
    .click()
    .run();
```

### 14. 特殊键和组合键

```typescript
await sdk
    .humanize()
    .window("Excel")
    .find("//Cell[@Name='A1']")
    .click()
    .key("Home")
    .key("End")
    .key("Tab")
    .shortcut("Ctrl+Shift+End")
    .shortcut("Alt+F4")
    .arrow("down", 3)
    .arrow("right", 5)
    .run();
```

### 15. 性能监控

```typescript
const stats = await sdk
    .profile()
    .humanize()
    .window("微信")
    .find("//Edit")
    .click()
    .type("测试")
    .run();

console.log(stats.totalTime);    // 3500ms
console.log(stats.steps);        // [{ step, xpath, time }, ...]
```

---

## 失败时的输出

找不到元素时自动截图退出：

```
[FAILED] XPath not found: //Button[@Name='发送']
Window: 微信 (mmui::MainWindow, WeChat.exe)

Available elements in window:
  - Pane[@ClassName='ChatView']
  - Edit[@Name='输入']
  - Button[@Name='表情']
  - Button[@Name='更多']

Screenshot saved: ./screenshots/failure-2024-04-28_15-30-45.png
Process exiting for manual intervention...
```

---

## 指令汇总表

### 初始化类

| 指令 | 说明 | 返回 |
|------|------|------|
| `.humanize(opts?)` | 开启拟人化模式 | 链 |
| `.debug()` | 开启调试日志 | 链 |
| `.profile()` | 开启性能监控 | 链 |
| `.logToFile(path)` | 日志写入文件 | 链 |
| `.timeout(ms)` | 整链超时控制 | 链 |

### 窗口类

| 指令 | 说明 | 返回 |
|------|------|------|
| `.window(selector)` | 激活并锁定窗口 | 链 |
| `.switchTo(title)` | 切换窗口 | 链 |
| `.waitForWindow(title, opts)` | 等待窗口出现 | 链 |
| `.closeWindow(title)` | 关闭窗口 | void |
| `.windowState()` | 获取窗口状态 | 状态对象 |
| `.listWindows()` | 获取所有窗口 | Window[] |
| `.screenshotWindow(path)` | 窗口截图 | path |

### 查找类

| 指令 | 说明 | 返回 |
|------|------|------|
| `.find(xpath)` | 找元素，失败退出 | 链 |
| `.tryFind(xpath)` | 尝试找，不报错 | 条件链 |
| `.findAll(xpath)` | 找所有匹配元素 | 链 |
| `.waitFor(xpath, opts)` | 等待元素出现 | 链 |
| `.waitUntilGone(xpath, opts)` | 等待元素消失 | 链 |
| `.waitForText(xpath, text, opts)` | 等待文本变化 | 链 |
| `.ifExists(xpath)` | 条件判断 | 条件链 |
| `.ifFind(xpath)` | 条件判断 | 条件链 |

### 操作类

| 指令 | 说明 | 返回 |
|------|------|------|
| `.click()` | 点击当前元素 | 链 |
| `.doubleClick()` | 双击 | 链 |
| `.rightClick()` | 右键 | 链 |
| `.type(text)` | 打字 | 链 |
| `.key(key)` | 单个按键 | 链 |
| `.shortcut(keys)` | 组合键 | 链 |
| `.arrow(dir, count)` | 方向键 | 链 |
| `.dragTo(xpath)` | 拖拽到目标 | 链 |
| `.scrollTo(xpath)` | 滚动到元素 | 链 |
| `.moveRelative(x, y)` | 相对偏移移动 | 链 |

### 查询类

| 指令 | 说明 | 返回 |
|------|------|------|
| `.inspect()` | 元素完整信息 | ElementInfo |
| `.capture()` | 获取元素信息 | ElementData |
| `.extract(attrs)` | 提取属性数组 | Object[] |
| `.extractTable()` | 提取表格数据 | string[][] |
| `.extractList()` | 提取列表文本 | string[] |
| `.ocr()` | OCR识别 | string |
| `.mousePosition()` | 鼠标坐标 | Point |

### 断言类

| 指令 | 说明 | 返回 |
|------|------|------|
| `.assertExists(xpath)` | 断言元素存在 | 链 |
| `.assertNotExists(xpath)` | 断言元素不存在 | 链 |
| `.assertText(xpath, text)` | 断言文本内容 | 链 |
| `.assertVisible(xpath)` | 断言元素可见 | 链 |
| `.assertEnabled(xpath)` | 断言元素可用 | 链 |
| `.assertValue(xpath, value)` | 断言元素值 | 链 |

### 控制类

| 指令 | 说明 | 返回 |
|------|------|------|
| `.wait(ms, random?)` | 等待 | 链 |
| `.retry(count, delay)` | 重试机制 | 链 |
| `.idle(opts)` | 启动空闲移动 | 链 |
| `.stopIdle()` | 停止空闲移动 | 链 |
| `.forEach(xpath, cb)` | 循环处理 | 链 |
| `.onNotFound(cb)` | 错误回调 | 链 |

### 截图类

| 指令 | 说明 | 返回 |
|------|------|------|
| `.screenshot(path)` | 整屏截图 | path |
| `.screenshotElement(path)` | 元素截图 | path |
| `.screenshotAuto()` | 自动命名截图 | path |
| `.ocrRegion(rect)` | 区域OCR | string |

### 执行类

| 指令 | 说明 | 返回 |
|------|------|------|
| `.run()` | 执行整条链 | void/Stats |

---

## ElementInfo 结构

```typescript
interface ElementInfo {
    name: string;              // 元素名称
    className: string;         // 类名
    automationId: string;      // 自动化ID
    controlType: string;       // 控件类型
    rect: Rect;                // 位置和大小
    isEnabled: boolean;        // 是否可用
    isVisible: boolean;        // 是否可见
    isFocused: boolean;        // 是否有焦点
    value: string;             // 当前值
    text: string;              // 文本内容
    processId: number;         // 进程ID
    frameworkId: string;       // 框架ID
}
```

---

## 实现计划

### Phase 1: 核心链式结构
- FluentChain 类
- find/window/click/type/run 基础方法
- 自动截图失败处理

### Phase 2: 查询能力
- inspect/capture/extract
- findAll/extractTable/extractList

### Phase 3: 高级操作
- waitFor/waitUntilGone
- doubleClick/rightClick/dragTo
- shortcut/key/arrow

### Phase 4: 断言和控制
- assert 系列
- retry/onNotFound
- ifExists/tryFind 条件链

### Phase 5: 增强功能
- profile/debug/logToFile
- OCR/截图
- 空闲移动集成

---

## 设计原则

1. **流式简洁**: 一条链完成整个流程
2. **失败即退**: 找不到元素自动截图退出
3. **信息充足**: 打印可用元素列表，方便定位
4. **拟人优先**: humanize 自动应用随机参数
5. **零 try-catch**: 链式调用无需错误处理
6. **Agent友好**: 截图+日志便于 AI介入