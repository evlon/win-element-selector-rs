# 快速开始 - 日志和错误处理

## 5 分钟上手

### 1. 查看日志输出

```bash
# 使用 debug 级别运行示例
LOG_LEVEL=debug npm run example:yuanbao
```

你会看到类似这样的输出：
```
[2026-05-15 10:30:07.123] INFO  (Chain): Starting chain execution
    actionsCount: 5
    humanizeEnabled: true

[2026-05-15 10:30:07.234] INFO  (HttpClient): POST /api/window/activate completed
    duration: 109ms
    status: 200

[2026-05-15 10:30:07.456] DEBUG (Chain): Finding element
    xpath: "//Button[@Name='Submit']"
```

### 2. 捕获异常

```typescript
import { SDK, ElementNotFoundError } from 'element-selector-sdk';

const sdk = new SDK();

try {
    await sdk.flow()
        .window('MyApp')
        .find('//Button[@Name="Submit"]')
        .click()
        .run();
} catch (error) {
    if (error instanceof ElementNotFoundError) {
        // 精确处理元素未找到错误
        console.error('元素不存在');
        console.error('XPath:', error.context?.xpath);
        console.error('截图路径:', error.context?.screenshotPath);
        
        // 可以在此处重试或采取其他措施
    } else {
        // 其他错误
        throw error;
    }
}
```

### 3. 调整日志级别

```typescript
import { LogConfig } from 'element-selector-sdk';

// 只显示错误
LogConfig.setLevel('error');

// 显示所有信息
LogConfig.setLevel('debug');

// 完全禁用日志
LogConfig.setLevel('silent');
```

### 4. 生产环境配置

```bash
# 使用 info 级别，JSON 格式输出
LOG_LEVEL=info NODE_ENV=production node app.js
```

输出示例（JSON）：
```json
{"level":30,"time":1715745007123,"module":"Chain","msg":"Starting chain execution","actionsCount":5}
{"level":30,"time":1715745007234,"module":"HttpClient","msg":"POST /api/window/activate completed","duration":109}
```

---

## 常见场景

### 场景 1：调试失败的操作

```bash
# 使用 trace 级别查看最详细的信息
LOG_LEVEL=trace npm run example:yuanbao
```

### 场景 2：只关注错误

```typescript
import { LogConfig } from 'element-selector-sdk';

LogConfig.setLevel('error');

// 现在只会看到错误日志
await sdk.flow().window('App').find('//Button').run();
```

### 场景 3：记录自定义日志

```typescript
import { createLogger } from 'element-selector-sdk';

const logger = createLogger('MyModule');

logger.info('Operation started', { userId: 123 });
logger.debug('Processing data', { count: 100 });
logger.error('Operation failed', { reason: 'timeout' });
```

### 场景 4：区分不同类型的错误

```typescript
import { 
    ElementNotFoundError,
    WindowNotFoundError,
    NetworkError,
    TimeoutError
} from 'element-selector-sdk';

try {
    await operation();
} catch (error) {
    if (error instanceof ElementNotFoundError) {
        handleElementNotFound(error);
    } else if (error instanceof WindowNotFoundError) {
        handleWindowNotFound(error);
    } else if (error instanceof NetworkError) {
        handleNetworkError(error);
    } else if (error instanceof TimeoutError) {
        handleTimeout(error);
    } else {
        throw error; // 未知错误
    }
}
```

---

## 下一步

- 📖 阅读 [LOGGING_GUIDE.md](./LOGGING_GUIDE.md) 了解完整功能
- 🔍 查看 [examples/test-logging.ts](./examples/test-logging.ts) 学习更多用法
- 📝 查看 [IMPLEMENTATION_SUMMARY.md](./IMPLEMENTATION_SUMMARY.md) 了解设计细节
