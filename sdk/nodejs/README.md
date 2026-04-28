# Element Selector SDK

Node.js SDK for element-selector-server, providing humanized UI automation capabilities.

## Features

### 已完成功能 ✅

| 功能 | 状态 | 说明 |
|------|------|------|
| **HTTP 服务** | ✅ | Actix-web 服务端，支持 API 调用 |
| **元素查找** | ✅ | XPath 选择器 + 窗口选择器 |
| **拟人化鼠标** | ✅ | Bezier 曲线轨迹、随机延迟 |
| **点击操作** | ✅ | 随机位置偏移、前后暂停 |
| **键盘打字** | ✅ | 随机字符延迟、拟人化输入 |
| **空闲移动** | ✅ | 在元素区域内随机移动鼠标 |
| **人工干预检测** | ✅ | 用户操作自动暂停/恢复 |
| **链式调用** | ✅ | ActionChain 执行序列操作 |
| **humanize 上下文** | ✅ | 自动应用拟人化参数 |

### 核心特性

- **拟人化鼠标移动** - 使用 Bezier 曲线轨迹，模拟真实用户操作
- **随机延迟** - 每次操作都有随机时间间隔
- **空闲移动** - 在指定区域内持续移动鼠标，防止检测
- **人工干预检测** - 检测用户鼠标/键盘操作，自动暂停自动化任务
- **链式调用** - 支持流畅的链式 API 调用

## Installation

```bash
npm install element-selector-sdk
```

## Quick Start

```typescript
import { ElementSelectorSDK } from 'element-selector-sdk';

const sdk = new ElementSelectorSDK({
    baseUrl: 'http://127.0.0.1:8080',
    timeout: 30000
});

// Check server status
const health = await sdk.health();
console.log('Server status:', health.status);

// List all windows
const windows = await sdk.listWindows();
console.log('Windows:', windows);

// Find an element
const element = await sdk.getElement({
    windowSelector: "Window[@Name='微信']",
    xpath: '//Button[@AutomationId="btnSend"]'
});
console.log('Element:', element);

// Click an element
await sdk.click({
    window: { title: '微信' },
    xpath: '//Button[@Name="发送"]'
});
```

## Best Practices ⭐

### 窗口激活的重要性

**关键点**: 鼠标和键盘操作需要目标窗口处于前台并获得焦点。在执行 `click`、`type` 等操作前，应先激活目标窗口。

```typescript
// ❌ 错误方式：直接操作可能失败
await sdk.click({ window: { title: '微信' }, xpath: '//Button' });
await sdk.type('消息内容');  // 可能打字到其他窗口

// ✅ 正确方式：先激活窗口
const windows = await sdk.listWindows();
const wechat = windows.find(w => w.title.includes('微信'));

// 1. 使用精确的窗口选择器
await sdk.activateWindow({
    title: wechat.title,
    className: wechat.className,
    processName: wechat.processName
});

// 2. 再执行操作
await sdk.click({ window: { title: wechat.title }, xpath: '//Edit' });
await sdk.type('消息内容');  // 现在焦点在微信
```

### 使用安全操作方法

SDK 提供了 `safeClick` 和 `safeType` 方法，自动处理窗口激活：

```typescript
// safeClick: 先激活窗口，再点击
await sdk.safeClick({
    window: { title: '微信', className: 'mmui::MainWindow' },
    xpath: '//Edit'
});

// safeType: 先激活窗口并聚焦元素，再打字
await sdk.safeType(
    { title: '微信' },
    '//Edit[@Name="输入"]',
    '这是消息内容'
);
```

### 窗口选择器精确性

使用精确的窗口选择器可以提高操作成功率：

```typescript
// ❌ 不精确：只使用 className
{ className: 'Notepad' }  // 可能匹配多个记事本窗口

// ✅ 精确：使用 title + className + processName
{
    title: 'Untitled - Notepad',
    className: 'Notepad',
    processName: 'Notepad'
}
```

## Humanized Operations

```typescript
// Using humanize context
await sdk.humanize(async (ctx) => {
    // Click element (auto-applies humanized parameters)
    await ctx.click({
        window: { title: '微信' },
        xpath: '//Button[@Name="发送"]'
    });
    
    // Type text with random delay per character
    await ctx.type('Hello World!', {
        charDelay: { min: 50, max: 150 }
    });
});

// Chained operations
await sdk.humanize(async (ctx) => {
    await ctx.chain()
        .click({ window: { title: '微信' }, xpath: '//Edit' })
        .type('消息内容')
        .wait(500)
        .click({ window: { title: '微信' }, xpath: '//Button[@Name="发送"]' })
        .execute();
});
```

## Idle Motion

空闲移动功能可在指定元素区域内自动移动鼠标，适用于长时间等待场景。

### 特性

- 在元素矩形区域内随机移动
- 支持 slow/normal/fast 三种速度
- 人工干预时自动暂停，静止后恢复
- API 调用时自动暂停 → 执行 → 恢复

### 示例

```typescript
// Start idle motion
await sdk.startIdleMotion({
    window: { title: '微信' },
    xpath: '//Pane[@ClassName="ChatView"]',
    speed: 'normal',
    moveInterval: 800,
    idleTimeout: 60000,
    humanIntervention: {
        enabled: true,
        pauseOnMouse: true,
        pauseOnKeyboard: true,
        resumeDelay: 3000
    }
});

// All operations auto-pause → execute → resume
await sdk.click({ window: { title: '微信' }, xpath: '//Edit' });
await sdk.type('自动回复消息');
await sdk.click({ window: { title: '微信' }, xpath: '//Button[@Name="发送"]' });

// Check status
const status = await sdk.getIdleMotionStatus();
console.log('Idle motion status:', status);

// Stop idle motion
await sdk.stopIdleMotion();
```

## API Reference

### ElementSelectorSDK

| Method | Description |
|--------|-------------|
| `health()` | 健康检查，返回服务器状态 |
| `listWindows()` | 获取所有可见窗口列表 |
| `getElement(params)` | 查找元素，返回位置和矩形 |
| `moveMouse(target, options?)` | 移动鼠标到指定位置 |
| `click(params)` | 点击元素 |
| `type(text, options?)` | 打字输入 |
| `activateWindow(selector)` | **激活窗口**（使其成为前台窗口） |
| `focusElement(selector, xpath)` | **激活窗口并聚焦元素** |
| `safeClick(params)` | **安全点击**（先激活窗口再点击） |
| `safeType(selector, xpath, text)` | **安全打字**（先聚焦再打字） |
| `humanize(callback)` | 拟人化上下文执行操作 |
| `startIdleMotion(params)` | 启动空闲移动 |
| `stopIdleMotion()` | 停止空闲移动 |
| `getIdleMotionStatus()` | 获取空闲移动状态 |

### WindowSelector

```typescript
interface WindowSelector {
    title?: string;        // 窗口标题
    className?: string;    // 窗口类名
    processName?: string;  // 进程名
}
```

### IdleMotionParams

```typescript
interface IdleMotionParams {
    window: WindowSelector;    // 目标窗口
    xpath: string;             // 元素 XPath
    speed?: 'slow' | 'normal' | 'fast';
    moveInterval?: number;     // 移动间隔 (ms)
    idleTimeout?: number;      // 空闲超时 (ms)
    humanIntervention?: {
        enabled: boolean;
        pauseOnMouse?: boolean;
        pauseOnKeyboard?: boolean;
        resumeDelay?: number;  // 恢复延迟 (ms)
    }; 
}
```

## Configuration

```typescript
const sdk = new ElementSelectorSDK({
    baseUrl: 'http://127.0.0.1:8080',  // Server URL
    timeout: 30000                      // Request timeout (ms)
});
```

## Default Values

| Parameter | Default | Description |
|-----------|---------|-------------|
| `moveOptions.humanize` | `true` | Enable humanized movement |
| `moveOptions.trajectory` | `'bezier'` | Movement trajectory type |
| `moveOptions.duration` | `600` | Movement duration (ms) |
| `clickOptions.randomRange` | `0.55` | Random click range percentage |
| `idleMotion.moveInterval` | `800` | Movement interval (ms) |
| `idleMotion.idleTimeout` | `60000` | Inactivity timeout (ms) |
| `typeOptions.charDelay.min` | `50` | Min char delay (ms) |
| `typeOptions.charDelay.max` | `150` | Max char delay (ms) |

## Testing

### Run Tests

```bash
# 安装依赖
npm install

# 运行所有测试
npm test

# 运行特定测试
npm test --testPathPattern=types       # 类型测试
npm test --testPathPattern=utils       # 工具函数测试
npm test --testPathPattern=integration  # 集成测试（需要服务器运行）
```

### Integration Tests

集成测试需要 `element-selector-server` 服务运行:

```bash
# 1. 启动服务器（在项目根目录）
cd ../..
cargo run --bin element-selector-server

# 2. 等待服务器启动（约 3 秒）
# 3. 运行集成测试
npm test --testPathPattern=integration
```

## Examples

示例代码位于 `examples/` 目录:

| 文件 | 说明 |
|------|------|
| `basic-usage.ts` | 基本使用：健康检查、窗口列表、元素查找、点击、打字 |
| `humanize-demo.ts` | 拟人化操作：humanize 上下文、链式调用 |
| `idle-motion-demo.ts` | 空闲移动：启动、监控状态、执行操作、停止 |

### Run Examples

```bash
# 1. 启动服务器
cd ../..
cargo run --bin element-selector-server

# 2. 运行示例（在 SDK 目录）
cd sdk/nodejs

# 编译 TypeScript
npm run build

# 运行示例
npx ts-node examples/basic-usage.ts
npx ts-node examples/humanize-demo.ts
npx ts-node examples/idle-motion-demo.ts
```

## Server

SDK 需要配合 `element-selector-server` 使用:

```bash
# 启动服务器（默认端口 8080）
cargo run --bin element-selector-server

# 指定端口
cargo run --bin element-selector-server -- --port 3000

# Release 构建
cargo build --release --bin element-selector-server
```

## Test Coverage

| 模块 | 测试数 | 说明 |
|------|--------|------|
| types.test.ts | 7 | 类型定义测试 |
| utils.test.ts | 7 | 工具函数测试 |
| integration.test.ts | 4 | API 集成测试 |
| Rust idle_motion | 6 | 服务端单元测试 |
| Rust keyboard | 6 | 服务端单元测试 |

## License

MIT