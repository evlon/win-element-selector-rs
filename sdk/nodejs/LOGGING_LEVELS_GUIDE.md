# 日志级别优化指南

## 设计理念

> **INFO 级别**：给用户看的，人能看懂的简洁操作进度  
> **DEBUG 级别**：给程序员看的，方便调试找问题的技术细节

## 日志级别对比

### INFO 级别（默认）

适合普通用户和运维人员，显示关键的操作进度和结果。

```
[2026-05-15 11:11:25.444 +0800] INFO: Chain: 开始执行自动化流程
    module: "Chain"

[2026-05-15 11:11:25.488 +0800] INFO: Chain: ✓ 窗口已激活
    module: "Chain"

[2026-05-15 11:11:25.488 +0800] INFO: Chain: ✓ 找到元素: 新建对话
    module: "Chain"
    controlType: "Button"
    rect: "(100, 200, 150x40)"

[2026-05-15 11:11:25.602 +0800] INFO: Chain: ✓ 点击成功 (click)
    module: "Chain"

[2026-05-15 11:11:25.830 +0800] INFO: Chain: ✓ 输入完成: 12 个字符
    module: "Chain"

[2026-05-15 11:11:25.830 +0800] INFO: Chain: ✅ 流程执行完成
    module: "Chain"
    totalTime: "386ms"
```

**特点**：
- ✅ 使用中文，易于理解
- ✅ 使用符号（✓、✅）增强可读性
- ✅ 只显示关键信息（元素名称、位置、耗时）
- ✅ 不包含技术细节（API 路径、参数等）

### DEBUG 级别

适合开发者和测试人员，显示完整的技术细节用于调试。

```bash
LOG_LEVEL=debug npm run example:yuanbao
```

输出示例：

```
[2026-05-15 11:11:25.444 +0800] INFO: Chain: 开始执行自动化流程
    module: "Chain"
    actionsCount: 8
    humanizeEnabled: true
    debugMode: false

[2026-05-15 11:11:25.445 +0800] DEBUG: Chain: Activating window
    module: "Chain"
    windowSelector: "Window[@Name='元宝' and @ClassName='Tauri Window'...]"

[2026-05-15 11:11:25.488 +0800] DEBUG: HttpClient: GET /api/element
    module: "HttpClient"
    windowSelector: "Window[@Name='元宝' and @ClassName='Tauri Window'...]..."
    xpath: "//Button[@Name=\"新建对话\"]..."

[2026-05-15 11:11:25.488 +0800] DEBUG: HttpClient: Element query completed
    module: "HttpClient"
    duration: 43
    found: true

[2026-05-15 11:11:25.488 +0800] DEBUG: Chain: Finding element
    module: "Chain"
    xpath: "//Button[@Name=\"新建对话\"]"

[2026-05-15 11:11:25.602 +0800] DEBUG: Chain: Executing action
    module: "Chain"
    type: "click"

[2026-05-15 11:11:25.779 +0800] DEBUG: HttpClient: POST /api/mouse/click
    module: "HttpClient"
    window: {...}
    xpath: "//Button[@Name=\"新建对话\"]..."

[2026-05-15 11:11:25.779 +0800] DEBUG: HttpClient: Click completed
    module: "HttpClient"
    duration: 177
    success: true
    clickPoint: { x: 175, y: 220 }

[2026-05-15 11:11:25.779 +0800] DEBUG: Chain: Action completed
    module: "Chain"
    type: "click"
    duration: 177
```

**特点**：
- 🔧 包含 API 调用详情（HTTP 方法、路径）
- 🔧 显示请求参数和响应数据
- 🔧 记录每个操作的耗时
- 🔧 保留英文技术术语（便于搜索和问题定位）

## 错误日志

错误日志始终显示，不受日志级别控制。

### 窗口未找到

```
[2026-05-15 11:11:26.488 +0800] ERROR: Chain: 窗口激活失败: 未找到匹配的窗口
    module: "Chain"
    windowSelector: "Window[@Name='不存在的窗口']"
```

### 元素未找到

```
[2026-05-15 11:11:26.488 +0800] ERROR: Chain: 元素未找到
    module: "Chain"
    xpath: "//Button[@Name=\"新建对话\"]"
    windowSelector: "Window[@Name='元宝' and @ClassName='Tauri Window'...]"

============================================================
[FAILED] 未找到元素: //Button[@Name="新建对话"]
============================================================
Window selector: Window[@Name='元宝' and @ClassName='Tauri Window' and @ProcessName='yuanbao']
XPath: //Button[@Name="新建对话"]

Available windows:
  - 元宝 (Tauri Window, yuanbao)
  - test-logging-levels.ts - win-element-selector-rs (工作区) - Lingma (Chrome_WidgetWin_1, Lingma)

Screenshot saved: D:\repos\uia-project\win-element-selector-rs\sdk\nodejs\screenshots\2026-05-15T03-11-26-failure-find.png
Process exiting for manual intervention...
```

### 网络错误

```
[2026-05-15 11:11:26.488 +0800] ERROR: HttpClient: Element query failed
    module: "HttpClient"
    params: {...}
    error: "connect ECONNREFUSED 127.0.0.1:8080"
```

## 如何使用

### 1. 默认模式（INFO 级别）

适合日常使用和监控：

```typescript
import { SDK } from '@element-selector/sdk';

const sdk = new SDK({ baseUrl: 'http://localhost:8080' });

await sdk.flow()
    .window({ title: '元宝' })
    .find('//Button[@Name="新建对话"]')
    .click()
    .run();
```

输出：
```
[INFO] Chain: 开始执行自动化流程
[INFO] Chain: ✓ 窗口已激活
[INFO] Chain: ✓ 找到元素: 新建对话
[INFO] Chain: ✓ 点击成功 (click)
[INFO] Chain: ✅ 流程执行完成
```

### 2. 调试模式（DEBUG 级别）

适合开发和故障排查：

```bash
# 方式 1：环境变量
LOG_LEVEL=debug npm run example:yuanbao

# 方式 2：代码中设置
process.env.LOG_LEVEL = 'debug';
```

输出：完整的 DEBUG + INFO 日志

### 3. 静默模式（SILENT 级别）

适合生产环境或脚本批量执行：

```bash
LOG_LEVEL=silent npm run example:yuanbao
```

输出：无日志（仅错误）

## 日志级别优先级

从高到低：

1. **silent** - 不输出任何日志（包括错误）
2. **error** - 仅输出错误
3. **warn** - 输出警告和错误
4. **info** - 输出信息、警告和错误（默认）
5. **debug** - 输出调试信息和所有以上级别
6. **trace** - 输出最详细的跟踪信息

## 最佳实践

### 开发阶段

```bash
# 使用 DEBUG 级别查看详细信息
LOG_LEVEL=debug npm run dev
```

### 测试阶段

```bash
# 使用 INFO 级别验证功能
npm test
```

### 生产环境

```bash
# 使用 INFO 或 WARN 级别
LOG_LEVEL=info node app.js
```

### CI/CD 流水线

```bash
# 使用 SILENT 级别减少噪音
LOG_LEVEL=silent npm run e2e-test
```

## 常见问题

### Q: 为什么有些日志是英文，有些是中文？

A: 
- **INFO 级别**：面向用户，使用中文，简洁易懂
- **DEBUG 级别**：面向开发者，使用英文技术术语，便于搜索和国际化

### Q: 如何只看到错误信息？

A: 设置 `LOG_LEVEL=error`

### Q: 日志文件保存在哪里？

A: 默认输出到控制台。如需保存到文件，可以：

```bash
# 重定向到文件
npm run example:yuanbao > logs/output.log 2>&1

# 或使用 pino 的文件传输
LOG_TRANSPORT_FILE=true npm run example:yuanbao
```

### Q: 如何在代码中动态切换日志级别？

A: 

```typescript
import { setLogLevel } from '@element-selector/sdk';

// 切换到调试模式
setLogLevel('debug');

// 切换到静默模式
setLogLevel('silent');
```

## 总结

| 场景 | 推荐级别 | 命令 |
|------|---------|------|
| 日常使用 | INFO | `npm run example` |
| 开发调试 | DEBUG | `LOG_LEVEL=debug npm run example` |
| 生产部署 | INFO/WARN | `LOG_LEVEL=info node app.js` |
| 批量执行 | SILENT | `LOG_LEVEL=silent npm run batch` |
| 问题排查 | DEBUG | `LOG_LEVEL=debug npm run example` |

---

**核心理念**：
- 📖 INFO = 人能看懂的操作进度
- 🔧 DEBUG = 程序员调试的技术细节
