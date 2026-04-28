# Element Selector SDK

Node.js SDK for element-selector-server, providing humanized UI automation capabilities.

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

## Idle Motion (Requires Server Support)

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
| `health()` | Check server health status |
| `listWindows()` | List all available windows |
| `getElement(params)` | Find element by window selector and XPath |
| `moveMouse(target, options?)` | Move mouse to target position |
| `click(params)` | Click on element |
| `type(text, options?)` | Type text (requires server support) |
| `humanize(callback)` | Execute operations with humanized context |
| `startIdleMotion(params)` | Start idle motion in element area |
| `stopIdleMotion()` | Stop idle motion |
| `getIdleMotionStatus()` | Get current idle motion status |

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
n
# 编译 TypeScript
npm run build

# 运行示例
npx ts-node examples/basic-usage.ts
npx ts-node examples/humanize-demo.ts
npx ts-node examples/idle-motion-demo.ts
```

## License

MIT