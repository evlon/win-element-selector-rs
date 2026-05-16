# 智能极简优化功能测试指南

## 测试步骤

### 1. 启动程序
```powershell
cd d:\repos\uia-project\win-element-selector-rs
.\target\debug\element-selector.exe
```

### 2. 捕获元素
1. 按 **F4** 键开始捕获
2. 将鼠标移动到 Chrome 浏览器中的目标元素上
3. 点击鼠标左键确认捕获

### 3. 测试智能优化
1. 在左侧面板中，找到 **"元素 XPath"** 区域
2. 点击 **"智能优化"** 按钮
3. 观察优化后的 XPath（应该会移除一些动态属性）

### 4. 测试极简优化 ⭐（新功能）
1. 点击 **"🎯 极简优化"** 按钮
2. 观察日志面板自动弹出
3. 查看实时优化过程：
   - 每个属性的尝试移除
   - 验证结果（成功/失败）
   - 最终统计信息
4. 如果需要，可以点击 **"❌ 取消"** 按钮中断优化
5. 优化完成后，可以：
   - 点击 **"🗑️ 清空"** 清除日志
   - 点击 **"📋 复制全部"** 复制所有日志
   - 关闭窗口或点击按钮隐藏面板

### 5. 预期结果对比

#### 原始 XPath（捕获后自动生成）
```xpath
//Document[@AutomationId='RootWebArea' and @FrameworkId='Chrome' and @LocalizedControlType='文档']/Group[@FrameworkId='Chrome' and @LocalizedControlType='组']/Group[starts-with(@ClassName, 'chat_mainPage__wilLn') and @FrameworkId='Chrome' and @LocalizedControlType='组']/Group[starts-with(@ClassName, 'temp-dialogue-btn_temp-dialogue') and @FrameworkId='Chrome' and @LocalizedControlType='组']
```

#### 智能优化后（可能结果）
```xpath
//Document[@AutomationId='RootWebArea' and @FrameworkId='Chrome']/Group[@FrameworkId='Chrome']/Group[starts-with(@ClassName, 'chat_mainPage__wilLn') and @FrameworkId='Chrome']/Group[starts-with(@ClassName, 'temp-dialogue-btn_temp-dialogue') and @FrameworkId='Chrome']
```
- 移除了 `@LocalizedControlType`（冗余属性）

#### 极简优化后（理想结果）
```xpath
//Document[@AutomationId='RootWebArea']/Group/Group[starts-with(@ClassName, 'chat_mainPage__wilLn')]/Group[starts-with(@ClassName, 'temp-dialogue-btn_temp-dialogue')]
```
- 移除了所有不必要的 `@FrameworkId` 和 `@LocalizedControlType`
- 只保留必要的定位属性（AutomationId、ClassName）
- 最简化的 XPath

### 6. 验证功能点

✅ **按钮状态切换**
- 点击"🎯 极简优化"后，按钮立即变为"❌ 取消"
- 优化完成或取消后，按钮恢复为"🎯 极简优化"

✅ **日志面板按需显示**
- 只在极简优化时自动弹出
- 可以通过窗口关闭按钮关闭
- 再次点击按钮可重新打开

✅ **实时日志输出**
- 显示每个节点的属性尝试过程
- 显示移除成功/失败的详细信息
- 显示最终统计（移除多少个属性、耗时等）

✅ **取消功能**
- 优化过程中可以随时点击"❌ 取消"
- 取消后立即停止优化
- 状态消息显示"极简优化已取消或失败"

✅ **日志管理**
- "🗑️ 清空"按钮清除所有日志
- "📋 复制全部"按钮复制格式化日志到剪贴板
- 自动滚动到最新日志
- 彩色显示不同级别的日志（INFO/WARN/ERROR）

### 7. 性能观察

对于您提供的 XPath（4层嵌套，每层约3个属性）：
- **预计尝试次数**: 约 10-12 次属性移除尝试
- **预计耗时**: 2-5 秒（取决于 UIA 响应速度）
- **日志条数**: 约 20-30 条（包括进度信息和验证结果）

### 8. 常见问题

**Q: 为什么极简优化比智能优化慢？**
A: 因为极简优化需要逐个尝试移除属性并实时验证，而智能优化基于规则快速分析。

**Q: 可以中途停止吗？**
A: 可以！点击"❌ 取消"按钮即可立即停止。

**Q: 日志面板怎么关闭？**
A: 点击窗口右上角的 X 按钮，或者等待下次优化时会自动清空。

**Q: 如果优化后 XPath 无法定位元素怎么办？**
A: 极简优化会确保每次移除属性后都能定位到相同元素，所以不会出现这个问题。如果验证失败，该属性会被保留。

---

## 技术实现细节

### 核心算法
1. **标准优化作为基础**: 先执行智能优化得到基准 XPath
2. **逐属性尝试**: 对每个节点的每个属性按优先级尝试移除
   - 优先级: AutomationId > ClassName > Name > FrameworkId > LocalizedControlType
3. **实时验证**: 每次尝试后调用 UIA API 验证是否仍能定位元素
4. **支持取消**: 使用 `Arc<AtomicBool>` 实现线程安全的取消标志

### 日志收集
- 使用自定义 `GuiLogger` 实现 `log::Log` trait
- 线程安全：`Arc<Mutex<Vec<LogEntry>>>`
- 最大行数限制：默认 1000 条，自动清理旧日志

### UI 集成
- 按钮动态文本：根据优化状态显示不同文本
- 日志窗口：egui Window，支持拖拽、调整大小
- 彩色日志：根据级别显示不同颜色
- 时间戳：精确到毫秒
