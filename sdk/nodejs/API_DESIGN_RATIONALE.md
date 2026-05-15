# SDK API 设计说明

## 设计理念

Element Selector SDK 采用**链式调用 + 隐式状态**的设计模式，在简洁性和功能性之间取得平衡。

## API 风格对比

### 方案 A：当前设计（链式调用 + 隐式状态）✅ 已采用

```typescript
sdk.flow()
    .window({ title: '元宝' })
    .find('//Button[@Name="新建对话"]')
    .click()              // 自动使用最近一次 find() 的结果
    .wait(2000)
    .find('//Edit[@AutomationId="input-editor"]')
    .click()              // 自动使用最近一次 find() 的结果
    .type('测试')         // 自动使用最近一次 find() 的结果
    .run();
```

**优点**：
- ✅ 代码简洁，可读性强
- ✅ 符合用户直觉（类似 jQuery、Playwright）
- ✅ 减少样板代码
- ✅ 适合快速编写自动化脚本

**缺点**：
- ⚠️ 依赖内部状态管理
- ⚠️ 需要理解"最近一次 find()"的语义
- ⚠️ 如果忘记 find() 直接 click() 会报错

**适用场景**：
- 线性自动化流程
- 快速原型开发
- E2E 测试脚本

---

### 方案 B：显式元素引用

```typescript
const flow = sdk.flow().window({ title: '元宝' });

const btn = await flow.find('//Button[@Name="新建对话"]');
await btn.click();

await flow.wait(2000);

const input = await flow.find('//Edit[@AutomationId="input-editor"]');
await input.click();
await input.type('测试');
```

**优点**：
- ✅ 状态明确，不易出错
- ✅ 可以复用元素引用
- ✅ 支持并行操作

**缺点**：
- ❌ 代码冗长
- ❌ 破坏链式调用的流畅性
- ❌ 需要 async/await，增加复杂度

**适用场景**：
- 复杂的条件分支逻辑
- 需要复用元素的场景
- 并行执行多个操作

---

### 方案 C：命名元素引用

```typescript
sdk.flow()
    .window({ title: '元宝' })
    .find('//Button[@Name="新建对话"]', 'newChatBtn')
    .click('newChatBtn')
    .wait(2000)
    .find('//Edit[@AutomationId="input-editor"]', 'inputBox')
    .click('inputBox')
    .type('inputBox', '测试')
    .run();
```

**优点**：
- ✅ 状态明确
- ✅ 可以跨步骤引用元素
- ✅ 保持链式调用

**缺点**：
- ❌ 需要为每个元素命名（增加认知负担）
- ❌ 名称可能冲突
- ❌ 简单场景下显得冗余

**适用场景**：
- 需要多次引用同一元素
- 复杂的页面交互流程

---

## 为什么选择方案 A？

### 1. 符合用户心智模型

大多数 UI 自动化工具（Selenium、Playwright、Puppeteer）都采用类似的设计：

```javascript
// Playwright
await page.click('#button');
await page.fill('#input', 'text');

// Selenium
driver.findElement(By.id('button')).click();
driver.findElement(By.id('input')).sendKeys('text');
```

用户已经习惯了"查找 → 操作"的模式。

### 2. 简洁性优先

对于 80% 的自动化场景，线性流程就足够了：

```typescript
// 打开应用 → 点击按钮 → 输入文本 → 提交
sdk.flow()
    .window({ title: 'App' })
    .find('//Button[@Name="开始"]')
    .click()
    .find('//Input')
    .type('数据')
    .find('//Button[@Name="提交"]')
    .click()
    .run();
```

如果用方案 B，代码量会增加 50% 以上。

### 3. 内部实现保证正确性

通过**按顺序串行执行**，确保每个操作都能访问到正确的状态：

```typescript
// 执行顺序（修复后）
actions: [
    { type: 'find', xpath: 'xpath1' },   // ← 执行，currentXpath = xpath1
    { type: 'click' },                    // ← 执行，使用 currentXpath (xpath1) ✅
    { type: 'find', xpath: 'xpath2' },   // ← 执行，currentXpath = xpath2
    { type: 'click' },                    // ← 执行，使用 currentXpath (xpath2) ✅
]
```

### 4. 提供足够的错误提示

当用户误用时，给出清晰的错误信息：

```typescript
sdk.flow()
    .window({ title: 'App' })
    .click()  // ❌ 错误：必须先调用 find() 找到元素
    .run();
```

错误信息：
```
Error: 必须先调用 find() 找到元素
```

---

## 最佳实践

### ✅ 推荐用法

```typescript
// 1. 线性流程（最常见）
sdk.flow()
    .window({ title: 'App' })
    .find('//Button')
    .click()
    .find('//Input')
    .type('text')
    .run();

// 2. 带等待的流程
sdk.flow()
    .window({ title: 'App' })
    .find('//Loading')
    .waitUntilGone('//Loading', { timeout: 10000 })
    .find('//Content')
    .click()
    .run();

// 3. 带重试的流程
sdk.flow()
    .retry(3, 1000)  // 最多重试 3 次，每次间隔 1 秒
    .window({ title: 'App' })
    .find('//Button')
    .click()
    .run();
```

### ⚠️ 注意事项

```typescript
// ❌ 错误：忘记 find() 直接 click()
sdk.flow()
    .window({ title: 'App' })
    .click()  // Error: 必须先调用 find() 找到元素
    .run();

// ❌ 错误：期望 click 作用于特定的 find，但中间有其他 find
sdk.flow()
    .find('//Button1')
    .find('//Button2')  // ← 覆盖了 currentXpath
    .click()            // ← 点击的是 Button2，不是 Button1
    .run();

// ✅ 正确：每个 click 前都有对应的 find
sdk.flow()
    .find('//Button1')
    .click()            // 点击 Button1
    .find('//Button2')
    .click()            // 点击 Button2
    .run();
```

### 💡 高级技巧

```typescript
// 1. 使用 waitFor 等待动态元素
sdk.flow()
    .window({ title: 'App' })
    .waitFor('//LoadingSpinner', { timeout: 5000 })
    .waitUntilGone('//LoadingSpinner')
    .find('//Content')
    .click()
    .run();

// 2. 使用 assert 验证状态
sdk.flow()
    .window({ title: 'App' })
    .find('//Button')
    .assertExists()
    .assertEnabled()
    .click()
    .find('//SuccessMessage')
    .assertText('操作成功')
    .run();

// 3. 组合使用 inspect 获取元素信息
const flow = sdk.flow()
    .window({ title: 'App' })
    .find('//Button');

const info = await flow.inspect();
console.log('按钮位置:', info.rect);

await flow.click().run();
```

---

## 未来扩展方向

如果用户需求变化，可以考虑以下扩展：

### 1. 添加显式元素引用（可选）

```typescript
// 向后兼容：仍然支持隐式状态
sdk.flow()
    .find('//Button')
    .click()
    .run();

// 新增：支持显式引用（用于复杂场景）
const btn = await sdk.element('//Button');
await btn.click();
await btn.doubleClick();
```

### 2. 添加作用域隔离

```typescript
// 在当前窗口上下文中操作
sdk.flow()
    .window({ title: 'App' })
    .scope(async (flow) => {
        // 在这个作用域内，所有操作都在 App 窗口中
        await flow.find('//Button').click();
        await flow.find('//Input').type('text');
    })
    .run();
```

### 3. 添加并行执行

```typescript
// 并行查找多个元素
const [btn, input] = await Promise.all([
    sdk.flow().find('//Button').inspect(),
    sdk.flow().find('//Input').inspect(),
]);

// 然后依次操作
sdk.flow()
    .click(btn)
    .type(input, 'text')
    .run();
```

---

## 总结

| 维度 | 方案 A（当前） | 方案 B（显式） | 方案 C（命名） |
|------|--------------|--------------|--------------|
| 简洁性 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ |
| 安全性 | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ |
| 学习成本 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ |
| 灵活性 | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ |
| 适用场景 | 80% 常规场景 | 复杂场景 | 中等复杂场景 |

**结论**：当前设计（方案 A）在简洁性和功能性之间取得了良好的平衡，适合大多数自动化场景。对于特殊需求，可以通过扩展 API 来支持。
