// sdk/nodejs/src/v2/screenshot.ts
// 截图管理模块

import * as fs from 'fs';
import * as path from 'path';

// ═══════════════════════════════════════════════════════════════════════════════
// 截图管理类
// ═══════════════════════════════════════════════════════════════════════════════

export class ScreenshotManager {
    private screenshotDir: string;
    
    constructor(baseDir: string = 'screenshots') {
        // 确保截图目录存在
        this.screenshotDir = path.resolve(baseDir);
        this.ensureDirectoryExists();
    }
    
    /**
     * 捕获失败截图
     * @param step 失败的步骤名称
     * @returns 截图文件路径
     */
    async captureFailure(step: string): Promise<string> {
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
    async captureScreen(name: string): Promise<string> {
        const filename = this.generateFilename('screen', name);
        const filepath = path.join(this.screenshotDir, filename);
        
        // TODO: 调用服务端截图 API
        await this.createPlaceholder(filepath, name);
        
        return filepath;
    }
    
    /**
     * 捕获窗口截图
     */
    async captureWindow(windowTitle: string): Promise<string> {
        const filename = this.generateFilename('window', windowTitle);
        const filepath = path.join(this.screenshotDir, filename);
        
        // TODO: 调用服务端截图 API
        await this.createPlaceholder(filepath, windowTitle);
        
        return filepath;
    }
    
    /**
     * 自动命名截图
     */
    async captureAuto(): Promise<string> {
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
    private generateFilename(type: string, name?: string): string {
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
    private ensureDirectoryExists(): void {
        if (!fs.existsSync(this.screenshotDir)) {
            fs.mkdirSync(this.screenshotDir, { recursive: true });
        }
    }
    
    /**
     * 创建占位文件（待实现真实截图）
     */
    private async createPlaceholder(filepath: string, context: string): Promise<void> {
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

// ═══════════════════════════════════════════════════════════════════════════════
// 全局单例
// ═══════════════════════════════════════════════════════════════════════════════

export const globalScreenshotManager = new ScreenshotManager();