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

## License

MIT