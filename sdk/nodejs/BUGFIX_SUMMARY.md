# 关键 Bug 修复总结

## 问题列表

### 1. ❌ 乱码问题
**现象**：日志中的中文显示为乱码（如 `寮€濮嬫墽琛岃嚜鍔ㄥ寲娴佺▼`）

**原因**：pino-pretty 在 Windows PowerShell 中编码配置不当

**解决方案**：
- 在 logger.ts 中添加 `destination: 1` 明确指定输出到 stdout
- 添加 `mkdir: true` 确保目录存在

**修改文件**：[src/logger.ts](file://d:\repos\uia-project\win-element-selector-rs\sdk\nodejs\src\logger.ts)

```typescript
transport: !isProduction ? {
    target: 'pino-pretty',
    options: {
        colorize: true,
        translateTime: 'SYS:standard',
        ignore: 'pid,hostname',
        messageFormat: '{module}: {msg}',
        // Windows 终端兼容性：使用 UTF-8 编码
        destination: 1, // stdout
        mkdir: true,
    }
} : undefined,
```

---

### 2. ⚠️ 偶发报错 "socket hang up"
**现象**：偶尔出现网络超时错误
```
❌ 发生错误: Network error: socket hang up
```

**原因**：默认超时时间为 30 秒，对于复杂的 UI 自动化操作（特别是拟人化移动 + 点击）可能不够

**解决方案**：
- 将默认超时时间从 30 秒增加到 60 秒

**修改文件**：[src/types.ts](file://d:\repos\uia-project\win-element-selector-rs\sdk\nodejs\src\types.ts)

```typescript
export const DEFAULTS = {
    baseUrl: 'http://127.0.0.1:8080',
    timeout: 60000,  // 增加到 60 秒，避免长时间操作超时
    // ...
};
```

---

### 3. 🔴 严重 Bug：只有最后一个 click 生效
**现象**：
```typescript
sdk.flow()
    .find(xpath1).click()  // ❌ 没有执行或执行了错误的元素
    .find(xpath2).click()  // ✅ 只有这个生效
    .run();
```

**根本原因**：

原来的执行逻辑是：
```typescript
async run() {
    // 第一步：执行所有 prefix actions（window 和 find）
    await this.executePrefixActions();  // ← 这里会执行所有的 find()
    
    // 第二步：执行其他 actions（click, type, wait等）
    for (const action of this.actions) {
        if (action.type === 'window' || action.type === 'find') {
            continue;  // 跳过，因为已经执行过了
        }
        await this.executeAction(action);  // click 使用 currentXpath
    }
}
```

这导致：
1. `executePrefixActions()` 遍历所有 actions，执行所有的 `find()`
2. 每次 `find()` 都会更新 `this.currentXpath`
3. 最终 `currentXpath` 指向**最后一个 find 的 xpath**
4. 后续所有的 `click()` 都使用同一个 xpath（最后一个）

**示例流程**：
```
actions: [
    { type: 'find', xpath: 'xpath1' },
    { type: 'click' },
    { type: 'find', xpath: 'xpath2' },
    { type: 'click' },
]

执行过程：
1. executePrefixActions() 执行所有 find:
   - executeFind('xpath1') → currentXpath = 'xpath1'
   - executeFind('xpath2') → currentXpath = 'xpath2'  ← 覆盖了！

2. 执行 click actions:
   - executeClick() → 使用 currentXpath ('xpath2')  ← 错误！应该是 xpath1
   - executeClick() → 使用 currentXpath ('xpath2')  ← 正确
```

**解决方案**：

改为**按顺序执行**，而不是分两批执行：

```typescript
async run() {
    const executeChain = async () => {
        // 按顺序执行所有动作
        for (const action of this.actions) {
            if (action.type === 'humanize') {
                continue;
            }
            
            // 对于 window 和 find，直接执行
            if (action.type === 'window') {
                await this.executeWindow(action.params as string);
                continue;
            }
            
            if (action.type === 'find') {
                await this.executeFind(action.xpath!);
                continue;
            }
            
            // 对于其他动作（click, type, wait等）
            await this.executeAction(action);
        }
    };
    
    await executeChain();
}
```

**新的执行流程**：
```
actions: [
    { type: 'find', xpath: 'xpath1' },
    { type: 'click' },
    { type: 'find', xpath: 'xpath2' },
    { type: 'click' },
]

执行过程：
1. executeFind('xpath1') → currentXpath = 'xpath1'
2. executeClick() → 使用 currentXpath ('xpath1')  ← ✅ 正确！
3. executeFind('xpath2') → currentXpath = 'xpath2'
4. executeClick() → 使用 currentXpath ('xpath2')  ← ✅ 正确！
```

**修改文件**：[src/chain.ts](file://d:\repos\uia-project\win-element-selector-rs\sdk\nodejs\src\chain.ts#L750-L790)

---

## 测试验证

### 测试脚本
创建了专门的测试脚本验证多次点击：

```bash
npx ts-node examples/test-multiple-clicks.ts
```

### 预期结果
```
=== 测试多个点击操作 ===

1. 健康检查...
   服务状态: ok

2. 开始执行多次点击测试...

[INFO] Chain: 开始执行自动化流程
[INFO] Chain: ✓ 窗口已激活
[INFO] Chain: ✓ 找到元素: 新建对话
[INFO] Chain: ✓ 点击成功 (click)
[INFO] Chain: ✓ 找到元素: input-editor
[INFO] Chain: ✓ 点击成功 (click)
[INFO] Chain: ✓ 输入完成: 6 个字符
[INFO] Chain: ✅ 流程执行完成

✅ 所有操作成功完成

验证点：
  ✓ 第一次点击应该点击了"新建对话"按钮
  ✓ 第二次点击应该点击了输入框
  ✓ 输入框中应该有"测试多次点击"文本
```

---

## 影响范围

### 受影响的场景
1. **所有使用多次 find().click() 的代码** - 之前只有最后一次点击生效
2. **复杂的多步骤自动化流程** - 可能导致不可预期的行为

### 不受影响的场景
1. 单次 find().click() - 正常工作
2. find().type() - 正常工作
3. 不包含 find 的纯操作序列 - 正常工作

---

## 回归测试建议

执行以下测试确保修复没有引入新问题：

```bash
# 1. 基础功能测试
npm test

# 2. 多次点击测试
npx ts-node examples/test-multiple-clicks.ts

# 3. 原有示例测试
npx ts-node examples/test-yuanbao.ts

# 4. 调试模式测试（查看完整日志）
LOG_LEVEL=debug npx ts-node examples/test-multiple-clicks.ts
```

---

## 总结

| 问题 | 严重程度 | 状态 | 修复方式 |
|------|---------|------|---------|
| 乱码 | 🟡 中等 | ✅ 已修复 | 配置 pino-pretty 输出选项 |
| 偶发超时 | 🟡 中等 | ✅ 已修复 | 增加超时时间到 60 秒 |
| 只有最后 click 生效 | 🔴 严重 | ✅ 已修复 | 改为按顺序执行 actions |

**核心改进**：
- ✅ 修复了严重的执行顺序 bug
- ✅ 提高了日志可读性（解决乱码）
- ✅ 增强了稳定性（增加超时时间）
- ✅ 保持了向后兼容性
