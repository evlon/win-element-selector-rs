# Windows 终端乱码解决方案

## 问题描述

在 Windows PowerShell 或 CMD 中运行 SDK 时，日志中的中文显示为乱码：

```
[2026-05-15 11:19:24.832 +0800] INFO: Chain: 寮€濮嬫墽琛岃嚜鍔ㄥ寲娴佺▼
```

## 原因

Windows 控制台默认使用 GBK 编码（代码页 936），而 pino-pretty 输出的是 UTF-8 编码的文本。

## 解决方案

### 方案 1：临时设置编码（推荐用于测试）

在运行命令前，先设置控制台编码为 UTF-8：

**PowerShell:**
```powershell
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
npm run example:yuanbao
```

**CMD:**
```cmd
chcp 65001
npm run example:yuanbao
```

### 方案 2：永久设置编码（推荐用于开发）

#### PowerShell
在 PowerShell 配置文件 `~\Documents\PowerShell\Microsoft.PowerShell_profile.ps1` 中添加：

```powershell
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
```

然后重启 PowerShell。

#### CMD
在系统环境变量中添加：
- 变量名：`PYTHONIOENCODING`
- 变量值：`utf-8`

或在注册表中设置默认代码页（需要管理员权限）。

### 方案 3：使用 Windows Terminal（最佳体验）

Windows Terminal 默认支持 UTF-8，无需额外配置。

1. 从 Microsoft Store 安装 Windows Terminal
2. 使用 Windows Terminal 运行命令：
   ```bash
   npm run example:yuanbao
   ```

### 方案 4：禁用美化输出（纯 JSON 格式）

如果不需要彩色日志，可以切换到生产模式，输出纯 JSON：

```bash
NODE_ENV=production npm run example:yuanbao
```

输出示例：
```json
{"level":30,"time":1715745564832,"module":"Chain","msg":"开始执行自动化流程"}
```

这种方式不会出现乱码，但可读性较差。

### 方案 5：重定向到文件查看

将输出重定向到文件，然后用支持 UTF-8 的编辑器打开：

```bash
npm run example:yuanbao > output.log 2>&1
code output.log  # 用 VS Code 打开
```

## 快速验证

运行以下命令测试编码是否正常：

```powershell
# PowerShell
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
node -e "console.log('中文测试：✓ 窗口已激活')"
```

应该看到正常的中文输出。

## 推荐的开发环境配置

### Visual Studio Code 集成终端

VS Code 的集成终端默认使用 UTF-8，无需额外配置。

1. 打开 VS Code
2. 使用内置终端运行：
   ```bash
   npm run example:yuanbao
   ```

### 设置 VS Code 终端编码

如果仍有问题，在 `.vscode/settings.json` 中添加：

```json
{
    "terminal.integrated.env.windows": {
        "PYTHONIOENCODING": "utf-8"
    }
}
```

## 总结

| 方案 | 适用场景 | 难度 | 效果 |
|------|---------|------|------|
| Windows Terminal | 日常开发 | ⭐ | ✅✅✅ 最佳 |
| VS Code 终端 | IDE 开发 | ⭐ | ✅✅✅ 最佳 |
| 临时设置编码 | 快速测试 | ⭐⭐ | ✅✅ 良好 |
| 永久设置编码 | 长期使用 | ⭐⭐⭐ | ✅✅ 良好 |
| 纯 JSON 输出 | 日志分析 | ⭐ | ✅ 可用 |
| 重定向到文件 | 问题排查 | ⭐⭐ | ✅✅ 良好 |

**推荐**：使用 **Windows Terminal** 或 **VS Code 集成终端**，它们默认支持 UTF-8，无需额外配置。
