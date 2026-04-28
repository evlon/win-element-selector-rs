"use strict";
// sdk/nodejs/src/v2/screenshot.ts
// 截图管理模块
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.globalScreenshotManager = exports.ScreenshotManager = void 0;
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
// ═══════════════════════════════════════════════════════════════════════════════
// 截图管理类
// ═══════════════════════════════════════════════════════════════════════════════
class ScreenshotManager {
    constructor(baseDir = 'screenshots') {
        // 确保截图目录存在
        this.screenshotDir = path.resolve(baseDir);
        this.ensureDirectoryExists();
    }
    /**
     * 捕获失败截图
     * @param step 失败的步骤名称
     * @returns 截图文件路径
     */
    async captureFailure(step) {
        const filename = this.generateFilename('failure', step);
        const filepath = path.join(this.screenshotDir, filename);
        // TODO: 调用服务端截图 API
        // 目前先创建一个占位文件
        await this.createPlaceholder(filepath, step);
        return filepath;
    }
    /**
     * 捕获全屏截图
     */
    async captureScreen(name) {
        const filename = this.generateFilename('screen', name);
        const filepath = path.join(this.screenshotDir, filename);
        // TODO: 调用服务端截图 API
        await this.createPlaceholder(filepath, name);
        return filepath;
    }
    /**
     * 捕获窗口截图
     */
    async captureWindow(windowTitle) {
        const filename = this.generateFilename('window', windowTitle);
        const filepath = path.join(this.screenshotDir, filename);
        // TODO: 调用服务端截图 API
        await this.createPlaceholder(filepath, windowTitle);
        return filepath;
    }
    /**
     * 自动命名截图
     */
    async captureAuto() {
        const filename = this.generateFilename('auto');
        const filepath = path.join(this.screenshotDir, filename);
        // TODO: 调用服务端截图 API
        await this.createPlaceholder(filepath, 'auto');
        return filepath;
    }
    // ═══════════════════════════════════════════════════════════════════════════════
    // 内部方法
    // ═══════════════════════════════════════════════════════════════════════════════
    /**
     * 生成文件名（带时间戳）
     */
    generateFilename(type, name) {
        const timestamp = new Date().toISOString()
            .replace(/[:.]/g, '-')
            .slice(0, 19);
        const safeName = name ? name.replace(/[^\w\-]/g, '_') : '';
        const parts = [timestamp, type];
        if (safeName) {
            parts.push(safeName);
        }
        return `${parts.join('-')}.png`;
    }
    /**
     * 确保目录存在
     */
    ensureDirectoryExists() {
        if (!fs.existsSync(this.screenshotDir)) {
            fs.mkdirSync(this.screenshotDir, { recursive: true });
        }
    }
    /**
     * 创建占位文件（待实现真实截图）
     */
    async createPlaceholder(filepath, context) {
        // 创建一个简单的 JSON 文件记录失败信息
        const info = {
            timestamp: new Date().toISOString(),
            context,
            filepath,
            note: 'Screenshot placeholder - actual screenshot API not yet implemented'
        };
        fs.writeFileSync(filepath + '.json', JSON.stringify(info, null, 2));
        // 创建一个空的 PNG 占位文件
        // 实际实现需要调用服务端截图 API
        const placeholderContent = `Screenshot placeholder for: ${context}\nTimestamp: ${info.timestamp}`;
        fs.writeFileSync(filepath + '.txt', placeholderContent);
    }
}
exports.ScreenshotManager = ScreenshotManager;
// ═══════════════════════════════════════════════════════════════════════════════
// 全局单例
// ═══════════════════════════════════════════════════════════════════════════════
exports.globalScreenshotManager = new ScreenshotManager();
//# sourceMappingURL=screenshot.js.map