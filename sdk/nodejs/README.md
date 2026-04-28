# Element Selector SDK

Node.js SDK for element-selector-server. **流式 XPath 自动化，简单直接。**

## 安装

```bash
npm install element-selector-sdk
```

## 快速开始

```typescript
import { SDK } from 'element-selector-sdk';

const sdk = new SDK();

// 流式链式调用
await sdk.chain()
    .humanize()                     // 开启拟人化
    .window('微信')                  // 激活窗口
    .find('//Edit[@Name="输入"]')    // 找不到自动截图退出
    .click()                        // 点击
    .type('你好')                    // 打字
    .run();                         // 执行
```

## API 文档

### SDK 类

| 方法 | 说明 |
|------|------|
| `chain()` | 创建流式链式调用 |
| `humanize(options?)` | 快捷方式：开启拟人化 |
| `window(selector)` | 快捷方式：指定窗口 |
| `health()` | 健康检查 |
| `listWindows()` | 获取窗口列表 |

### FluentChain 流式链

#### 初始化

| 方法 | 说明 |
|------|------|
| `.humanize({ speed? })` | 开启拟人化，speed: 'slow'/'normal'/'fast' |
| `.debug()` | 开启调试日志 |
| `.profile()` | 开启性能监控 |

#### 窗口

| 方法 | 说明 |
|------|------|
| `.window(selector)` | 激活窗口，selector 可以是字符串或对象 |

#### 查找

| 方法 | 说明 |
|------|------|
| `.find(xpath)` | 查找元素，找不到自动截图退出 |
| `.tryFind(xpath)` | 尝试查找，失败返回 null |
| `.findAll(xpath)` | 查找所有匹配元素 |
| `.exists(xpath)` | 检查元素是否存在 |
| `.waitFor(xpath, options?)` | 等待元素出现 |
| `.waitUntilGone(xpath, options?)` | 等待元素消失 |
| `.inspect()` | 获取元素信息 |

#### 操作

| 方法 | 说明 |
|------|------|
| `.click()` | 点击当前元素 |
| `.doubleClick()` | 双击 |
| `.rightClick()` | 右键点击 |
| `.type(text)` | 打字 |
| `.key(keyName)` | 单个按键 (Enter, Tab, Escape 等) |
| `.shortcut(keys)` | 快捷键 (Ctrl+C, Alt+F4 等) |
| `.wait(ms, randomMax?)` | 等待指定时间 |

#### 断言

| 方法 | 说明 |
|------|------|
| `.assertExists(xpath)` | 断言元素存在 |
| `.assertNotExists(xpath)` | 断言元素不存在 |
| `.assertText(xpath, expected)` | 断言文本内容 |
| `.assertVisible(xpath)` | 断言元素可见 |
| `.assertEnabled(xpath)` | 断言元素可用 |

#### 数据提取

| 方法 | 说明 |
|------|------|
| `.extract(xpath, attrs)` | 提取属性数组 |
| `.extractList(xpath)` | 提取文本列表 |
| `.extractTable(xpath)` | 提取表格数据 |

#### 截图

| 方法 | 说明 |
|------|------|
| `.screenshot(path?)` | 全屏截图 |
| `.screenshotElement(path?)` | 元素截图 |
| `.screenshotAuto()` | 自动命名截图 |

#### 空闲移动

| 方法 | 说明 |
|------|------|
| `.idle({ xpath, speed? })` | 启动空闲移动 |
| `.stopIdle()` | 停止空闲移动 |

#### 控制流

| 方法 | 说明 |
|------|------|
| `.retry(count, delayMs)` | 设置重试机制 |
| `.run()` | 执行整条链，返回 ProfileStats（如开启 profile） |

## 使用示例

### 基础自动化

```typescript
await sdk.chain()
    .humanize()
    .window({ title: '微信', className: 'mmui::MainWindow' })
    .find('//Edit[@Name="输入"]')
    .click()
    .type('自动发送的消息')
    .wait(500, 1000)                     // 随机等待 500-1000ms
    .find('//Button[@Name="发送"]')
    .click()
    .run();
```

### 等待加载

```typescript
await sdk.chain()
    .window('Chrome')
    .waitFor('//Button[@Name="登录"]', { timeout: 10000 })
    .click()
    .waitUntilGone('//Spinner[@Name="加载中"]')
    .run();
```

### 快捷键操作

```typescript
await sdk.chain()
    .window('Excel')
    .find('//Cell[@Name="A1"]')
    .click()
    .shortcut('Ctrl+C')                 // 复制
    .key('Tab')                         // Tab 键
    .shortcut('Ctrl+V')                 // 粘贴
    .run();
```

### 数据提取

```typescript
// 提取列表项
const items = await sdk.chain()
    .window('微信')
    .findAll('//ListItem');

// 提取属性
const data = await sdk.chain()
    .window('Excel')
    .extract('//Cell', ['name', 'value', 'rect']);
```

### 性能监控

```typescript
const stats = await sdk.chain()
    .profile()
    .humanize()
    .window('微信')
    .find('//Edit')
    .click()
    .type('测试')
    .run();

console.log(`总耗时: ${stats.totalTime}ms`);
console.log(`步骤: ${stats.steps}`);
```

### 断言验证

```typescript
await sdk.chain()
    .window('微信')
    .assertExists('//Edit[@Name="输入"]')
    .assertEnabled('//Button[@Name="发送"]')
    .run();
```

### 空闲移动 + 自动化

```typescript
await sdk.chain()
    .window('微信')
    .idle({ xpath: '//Pane[@ClassName="ChatView"]', speed: 'normal' })
    .find('//Edit[@Name="输入"]')
    .click()
    .type('自动回复')
    .stopIdle()
    .run();
```

### 重试机制

```typescript
await sdk.chain()
    .retry(3, 2000)                      // 失败重试3次，间隔2秒
    .window('Chrome')
    .find('//Button[@Name="提交"]')
    .click()
    .run();
```

## 失败处理

找不到元素时自动截图退出：

```
[FAILED] Element not found: //Button[@Name="发送"]
Window selector: Window[@Name='微信']

Available windows:
  - 微信 (mmui::MainWindow, WeChat.exe)
  - Chrome (Chrome_WidgetWin_1, chrome.exe)

Screenshot saved: ./screenshots/failure-2024-04-28_15-30-45.png
Process exiting for manual intervention...
```

## 窗口选择器

```typescript
// 字符串格式（XPath）
.window('Window[@Name="微信"]')

// 对象格式
.window({ title: '微信' })
.window({ className: 'Chrome_WidgetWin_1' })
.window({ title: '微信', className: 'mmui::MainWindow', processName: 'WeChat' })
```

**最佳实践**: 使用 `title + className + processName` 确保精确匹配。

## 启动服务器

SDK 需要 element-selector-server：

```bash
# 启动服务器（默认端口 8080）
cargo run --bin element-selector-server

# 指定端口
cargo run --bin element-selector-server -- --port 3000
```

## 测试

```bash
npm test
```

## License

MIT