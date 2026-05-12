---
name: windows-only-simplify
overview: 精简项目代码，去除所有跨平台条件编译和非 Windows stub/mock 代码，将项目改为纯 Windows 实现
todos:
  - id: cargo-toml
    content: 将 windows 依赖从条件依赖移至普通 [dependencies]
    status: completed
  - id: core-layer
    content: 清理 core 层：enum_windows.rs、mod.rs、uia.rs 的 cfg 守卫和 mock_impl
    status: completed
    dependencies:
      - cargo-toml
  - id: gui-layer
    content: 清理 gui 层：mouse_hook.rs 和 highlight.rs 的 cfg 守卫及 stub 模块
    status: completed
    dependencies:
      - core-layer
  - id: api-layer
    content: 清理 api 层：keyboard.rs、window.rs、mouse.rs、idle_motion.rs 的 cfg 守卫及 stub
    status: completed
    dependencies:
      - core-layer
  - id: top-layer
    content: 清理顶层：mouse_control.rs、main.rs、bin/server.rs 的 cfg 守卫及 stub
    status: completed
    dependencies:
      - gui-layer
      - api-layer
  - id: build-verify
    content: 编译验证：cargo build 确认无编译错误
    status: completed
    dependencies:
      - top-layer
---

## 用户需求

项目仅适配 Windows 平台，需要移除所有跨平台条件编译代码（`#[cfg(target_os = "windows")]` / `#[cfg(not(target_os = "windows"))]`）及对应的非 Windows stub/mock 实现，精简逻辑。

## 核心变更

- 删除所有 `#[cfg(target_os = "windows")]` 条件编译守卫，Windows 实现代码直接生效
- 删除所有 `#[cfg(not(target_os = "windows"))]` 块，包括 stub 函数、mock 模块、空实现
- 将 `Cargo.toml` 中的 `windows` 依赖从条件依赖移至普通依赖
- 涉及 13 个文件的精简重构，无功能变更

## 技术栈

- 项目本身：Rust + Windows UI Automation (windows-rs crate) + eframe/egui + actix-web
- 本次变更：纯代码精简重构，不引入新技术

## 实现方案

采用逐文件清理策略，按依赖层级从底层到上层处理：

1. **Cargo.toml**：将 `windows` 从 `[target.'cfg(windows)'.dependencies]` 移至 `[dependencies]`
2. **core 层**：清理 `enum_windows.rs`、`mod.rs`、`uia.rs`（含删除 mock_impl 模块约 70 行）
3. **gui 层**：清理 `mouse_hook.rs`（删除 stub 模块）、`highlight.rs`（删除 stub 分支）
4. **api 层**：清理 `keyboard.rs`（删除 stub）、`window.rs`、`mouse.rs`、`idle_motion.rs`
5. **顶层**：清理 `mouse_control.rs`（删除 stub）、`main.rs`、`bin/server.rs`

## 实现要点

- 每个 `#[cfg(target_os = "windows")]` 块：保留内部代码，删除 cfg 属性
- 每个 `#[cfg(not(target_os = "windows"))]` 块：整块删除
- 条件导入 `use` 语句：去掉 cfg 属性，直接导入
- 条件模块 `mod`：去掉 cfg 属性，删除 stub/mock 替代模块
- 条件 `pub use` 重导出：去掉 cfg 属性

## 目录结构

```
d:\repos\element-selector\
├── Cargo.toml                                    # [MODIFY] windows 依赖从条件依赖移至普通依赖
├── src/
│   ├── main.rs                                   # [MODIFY] 去掉 COM 初始化和字体加载的 cfg 守卫
│   ├── mouse_control.rs                          # [MODIFY] 去掉 cfg 导入和函数内分支，删除 stub execute_trajectory
│   ├── core/
│   │   ├── mod.rs                                # [MODIFY] 去掉 enumerate_windows_fast 的 cfg 守卫
│   │   ├── enum_windows.rs                       # [MODIFY] 去掉所有 cfg(target_os = "windows") 守卫
│   │   └── uia.rs                                # [MODIFY] 去掉 windows_impl 的 cfg 守卫，删除 mock_impl 模块和条件 pub use
│   ├── gui/
│   │   ├── highlight.rs                          # [MODIFY] 去掉公共 API 的 cfg 分支，去掉 windows_impl 的 cfg 守卫，删除 stub 分支
│   │   └── mouse_hook.rs                         # [MODIFY] 去掉 cfg 导入和 HOOK_HANDLE 的 cfg 守卫，删除 win_hook stub 模块
│   ├── api/
│   │   ├── keyboard.rs                           # [MODIFY] 去掉 cfg 导入，删除 parse_key_name/send_unicode_char/execute_key 的 stub 分支
│   │   ├── window.rs                             # [MODIFY] 去掉 ensure_com_sta() 调用的 cfg 守卫
│   │   ├── mouse.rs                              # [MODIFY] 去掉 ensure_com_sta() 调用的 cfg 守卫
│   │   └── idle_motion.rs                        # [MODIFY] 去掉 ensure_com_sta() 调用的 cfg 守卫
│   └── bin/
│       └── server.rs                             # [MODIFY] 去掉 COM 初始化的 cfg 守卫
```