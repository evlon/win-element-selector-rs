# SDK 日志和错误处理使用指南

## 日志系统

SDK 使用 pino 作为日志引擎，支持结构化日志输出和多个日志级别。

### 日志级别

- **trace**: 最详细的调试信息（鼠标轨迹点、API 原始数据）
- **debug**: 调试信息（缓存状态、动作执行详情）
- **info**: 关键操作（窗口激活、元素查找成功、点击完成）
- **warn**: 警告信息（重试、降级操作）
- **error**: 错误信息（异常、失败截图）
- **silent**: 禁用所有日志

### 配置方式

#### 1. 环境变量（推荐）

```bash
# 开发环境 - 美化输出
LOG_LEVEL=debug NODE_ENV=development npm run example:yuanbao

# 生产环境 - JSON 格式
LOG_LEVEL=info NODE_ENV=production node dist/examples/test-yuanbao.js

# 只查看错误
LOG_LEVEL=error npm run example:yuanbao
```

#### 2. 代码中动态设置

```typescript
import { SDK, LogConfig } from 'element-selector-sdk';

// 设置全局日志级别
LogConfig.setLevel('debug');

// 启用生产模式（JSON 输出）
LogConfig.enableProduction();

const sdk = new SDK();
```

### 日志输出示例

**开发环境（美化）**：
```
[2026-05-15 10:30:07.123] INFO  (Chain): Starting chain execution
    actionsCount: 5
    humanizeEnabled: true

[2026-05-15 10:30:07.234] INFO  (HttpClient): POST /api/window/activate completed
    duration: 109ms
    status: 200

[2026-05-15 10:30:07.456] ERROR (Chain): Element not found
    xpath: "//Button[@Name='Submit']"
    windowSelector: "Window[@Name='MyApp']"
```

**生产环境（JSON）**：
```json
{"level":30,"time":1715745007123,"module":"Chain","msg":"Starting chain execution","actionsCount":5,"humanizeEnabled":true}
{"level":30,"time":1715745007234,"module":"HttpClient","msg":"POST /api/window/activate completed","duration":109,"status":200}
```

---

## 错误处理

SDK 提供结构化的异常类，便于精确捕获和处理不同类型的错误。

### 异常类型

#### 1. ElementNotFoundError
元素未找到时抛出。

```typescript
import { SDK, ElementNotFoundError } from 'element-selector-sdk';

try {
    await sdk.flow()
        .window('MyApp')
        .find('//Button[@Name="Submit"]')
        .click()
        .run();
} catch (error) {
    if (error instanceof ElementNotFoundError) {
        console.error('元素未找到');
        console.error('XPath:', error.context?.xpath);
        console.error('窗口:', error.context?.windowSelector);
        console.error('截图:', error.context?.screenshotPath);
        console.error('提示:', error.context?.hint);
    }
}
```

#### 2. WindowNotFoundError
窗口未找到时抛出。

```typescript
import { WindowNotFoundError } from 'element-selector-sdk';

try {
    await sdk.flow()
        .window({ title: 'NonExistent' })
        .find('//Button')
        .run();
} catch (error) {
    if (error instanceof WindowNotFoundError) {
        console.error('窗口未找到:', error.context?.windowSelector);
    }
}
```

#### 3. NetworkError
网络请求失败时抛出。

```typescript
import { NetworkError } from 'element-selector-sdk';

try {
    await sdk.health();
} catch (error) {
    if (error instanceof NetworkError) {
        console.error('网络错误:', error.message);
        console.error('端点:', error.context?.endpoint);
        console.error('原始错误:', error.context?.originalMessage);
    }
}
```

#### 4. TimeoutError
操作超时时抛出。

```typescript
import { TimeoutError } from 'element-selector-sdk';

try {
    await sdk.flow()
        .window('MyApp')
        .waitFor('//Loading', { timeout: 5000 })
        .run();
} catch (error) {
    if (error instanceof TimeoutError) {
        console.error('超时:', error.context?.operation);
        console.error('超时时间:', error.context?.timeout, 'ms');
    }
}
```

#### 5. ActionFailedError
动作执行失败时抛出（点击、打字等）。

```typescript
import { ActionFailedError } from 'element-selector-sdk';

try {
    await sdk.flow()
        .window('MyApp')
        .find('//Button')
        .click()
        .run();
} catch (error) {
    if (error instanceof ActionFailedError) {
        console.error('动作失败:', error.context?.action);
        console.error('原因:', error.context?.reason);
        console.error('截图:', error.context?.screenshotPath);
    }
}
```

### 通用错误处理

```typescript
import { SDK, isSDKError } from 'element-selector-sdk';

try {
    await sdk.flow()
        .window('MyApp')
        .find('//Button')
        .click()
        .run();
} catch (error) {
    if (isSDKError(error)) {
        // 所有 SDK 异常都有这些属性
        console.error('错误代码:', error.code);
        console.error('错误消息:', error.message);
        console.error('上下文:', error.context);
        console.error('时间戳:', new Date(error.timestamp).toISOString());
    } else {
        // 其他未知错误
        console.error('未知错误:', error);
    }
}
```

---

## 最佳实践

### 1. 开发阶段使用 DEBUG 级别

```bash
LOG_LEVEL=debug npm run example:yuanbao
```

可以看到每个操作的详细信息，便于调试。

### 2. 生产环境使用 INFO 或 WARN 级别

```bash
LOG_LEVEL=info NODE_ENV=production node app.js
```

减少日志体积，只保留关键信息。

### 3. 精确捕获异常

```typescript
// ✅ 推荐：精确捕获
try {
    await operation();
} catch (error) {
    if (error instanceof ElementNotFoundError) {
        handleElementNotFound(error);
    } else if (error instanceof NetworkError) {
        handleNetworkError(error);
    } else {
        throw error; // 重新抛出未知错误
    }
}

// ❌ 不推荐：捕获所有错误
try {
    await operation();
} catch (error) {
    console.error('出错了:', error);
}
```

### 4. 记录结构化上下文

```typescript
// ✅ 推荐：结构化日志
logger.info('Operation completed', {
    duration: 1234,
    success: true,
    elementCount: 5
});

// ❌ 不推荐：字符串拼接
logger.info(`Operation completed in 1234ms with 5 elements`);
```

### 5. 错误中包含足够信息

```typescript
// ✅ 推荐：包含上下文
throw new ElementNotFoundError(xpath, windowSelector, screenshotPath);

// ❌ 不推荐：简单消息
throw new Error('Element not found');
```

---

## 故障排查

### 问题：看不到日志输出

**原因**：日志级别设置过高。

**解决**：
```bash
# 检查当前日志级别
echo $LOG_LEVEL  # Linux/Mac
echo %LOG_LEVEL% # Windows

# 设置为 debug
export LOG_LEVEL=debug  # Linux/Mac
set LOG_LEVEL=debug     # Windows
```

### 问题：日志太多影响性能

**解决**：
```typescript
// 生产环境使用 info 或 warn 级别
LogConfig.setLevel('info');
```

### 问题：无法区分不同模块的日志

**解决**：日志已自动包含模块名称：
```
[Chain]: Starting execution
[HttpClient]: Request completed
[Screenshot]: Capture saved
```

---

## API 参考

### Logger 类

```typescript
class Logger {
    trace(msg: string, context?: object): void
    debug(msg: string, context?: object): void
    info(msg: string, context?: object): void
    warn(msg: string, context?: object): void
    error(msg: string, context?: object): void
    errorWithException(error: Error, msg?: string, context?: object): void
    setLevel(level: LogLevel): void
    getLevel(): string
}
```

### LogConfig

```typescript
const LogConfig = {
    setLevel(level: LogLevel): void
    getLevel(): LogLevel
    enableProduction(): void
    enableDevelopment(): void
}
```

### 异常类

```typescript
class SDKError extends Error {
    readonly code: string
    readonly context?: Record<string, any>
    readonly timestamp: number
    toJSON(): Record<string, any>
}

class ElementNotFoundError extends SDKError { ... }
class WindowNotFoundError extends SDKError { ... }
class NetworkError extends SDKError { ... }
class TimeoutError extends SDKError { ... }
class ActionFailedError extends SDKError { ... }
```
