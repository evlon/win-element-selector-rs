# 编译问题修复记录

## 2026-05-29: Bounds 偏移计算功能编译修复

### 问题列表

#### 1. ValidationResult::Found 缺少 element_info 字段

**错误信息**:
```
error[E0026]: variant `core::model::ValidationResult::Found` does not have a field named `element_info`
```

**原因**: `ValidationResult::Found` 只有 `count`, `first_rect`, `rects` 字段，没有 `element_info`。

**解决方案**: 
- 从 `DetailedValidationResult` 中无法直接获取 element_info
- 改为通过窗口选择器获取窗口矩形，然后在 click_mouse 函数中手动计算 visibleRect
- 使用 `get_window_rect_by_selector` 获取窗口矩形，然后与元素矩形求交集

#### 2. async 函数中使用 ? 运算符

**错误信息**:
```
error[E0277]: the `?` operator can only be used in an async function that returns `Result` or `Option`
```

**原因**: `click_mouse` 函数返回 `HttpResponse`，不是 `Result` 类型，不能直接使用 `?` 运算符。

**解决方案**: 
- 将 `calculate_offset_click_point` 的调用包裹在 `match` 语句中
- 添加 Err 分支处理计算失败的情况
- 返回适当的错误响应

#### 3. ElementRect 字段名称错误

**错误信息**:
```
error[E0609]: no field `left` on type `core::model::ElementRect`
error[E0609]: no field `top` on type `core::model::ElementRect`
...
```

**原因**: `ElementRect` 使用的是 `x, y, width, height` 字段，而不是 `left, top, right, bottom`。

**解决方案**: 
- 修正字段访问：`win_rect.x`, `win_rect.y`, `win_rect.width`, `win_rect.height`

#### 4. offset_parser 测试失败

**错误信息**:
```
assertion `left == right` failed
  left: 210.0
 right: 190.0
```

**原因**: 偏移表达式的语义理解错误。最初的实现假设 `+/-` 统一表示向内/向外，但实际语义是：
- `left+X`: x = X (距离左边 X)
- `right-X`: x = width - X (距离右边 X，向内)
- `top-X`: y = -X (在顶部外侧)
- `bottom+X`: y = height - X (距离底部 X，向内)

**解决方案**: 
- 重新设计表达式解析逻辑，使 `+/-` 对不同参考边有不同的含义
- left/top: `+` 增加坐标值（向内），`-` 减少坐标值（向外）
- right/bottom: `-` 减少距离（向内），`+` 增加距离（向外）

#### 5. 未使用的导入警告

**警告信息**:
```
warning: unused import: `Point`
```

**解决方案**: 从 `offset_parser.rs` 中移除未使用的 `Point` 导入。

### 修复后的测试结果

✅ Rust 后端编译成功（debug 和 release）
✅ 所有单元测试通过（6/6）
✅ TypeScript SDK 编译成功
✅ 无警告信息

### 关键修改文件

1. **src/api/mouse.rs**
   - 修改 `click_mouse` 函数，手动计算 visibleRect
   - 添加 match 语句处理计算结果
   - 添加 Err 分支处理错误情况

2. **src/api/offset_parser.rs**
   - 移除未使用的 `Point` 导入
   - 修正偏移表达式解析逻辑
   - 更新注释说明正确的语义

3. **src/core/uia.rs**
   - 无需修改（已存在 `get_window_rect_by_selector` 函数）

### 最终实现的语义

| 表达式 | 含义 | 计算公式 |
|--------|------|----------|
| `left+X%` | 距离左边 X% | x = width × X% |
| `left-X%` | 在左边外侧 X% | x = -(width × X%) |
| `right-X%` | 距离右边 X%（向内） | x = width - (width × X%) |
| `right+X%` | 在右边外侧 X% | x = width + (width × X%) |
| `top+Xpx` | 距离顶部 Xpx（向内） | y = X |
| `top-Xpx` | 在顶部外侧 Xpx | y = -X |
| `bottom+Xpx` | 距离底部 Xpx（向内） | y = height - X |
| `bottom-Xpx` | 在底部外侧 Xpx | y = height + X |

**记忆技巧**: 
- `+` 对于 left/top 表示向正方向移动（向右/向下，即向内）
- `-` 对于 right/bottom 表示向负方向移动（向左/向上，即向内）
- 相反的操作符会将坐标移到元素外部
