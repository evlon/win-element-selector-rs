// sdk/nodejs/src/v2/index.ts
// SDK V2 - 流式 XPath 自动化 API

import { HttpClient } from '../client';
import { FluentChain } from './fluent-chain';
import { ElementInfo } from './fluent-chain';
import { ScreenshotManager, globalScreenshotManager } from './screenshot';
import { SDKConfig, DEFAULTS, WindowSelector } from '../types';
import { buildWindowSelector } from '../utils';

// ═══════════════════════════════════════════════════════════════════════════════
// SDK V2 入口类
// ═══════════════════════════════════════════════════════════════════════════════

export class ElementSelectorSDKv2 {
    private client: HttpClient;
    private screenshotManager: ScreenshotManager;
    
    constructor(config?: Partial<SDKConfig>) {
        this.client = new HttpClient({
            baseUrl: config?.baseUrl ?? DEFAULTS.baseUrl,
            timeout: config?.timeout ?? DEFAULTS.timeout,
        });
        this.screenshotManager = globalScreenshotManager;
    }
    
    /**
     * 创建流式链式调用
     * 
     * @example
     * await sdk.chain()
     *     .humanize()
     *     .window("微信")
     *     .find("//Edit[@Name='输入']")
     *     .click()
     *     .type("你好")
     *     .run();
     */
    chain(): FluentChain {
        return new FluentChain(this.client);
    }
    
    /**
     * 快捷方式：直接开始拟人化链式调用
     * 
     * @example
     * await sdk
     *     .humanize()
     *     .window("微信")
     *     .find("//Edit")
     *     .click()
     *     .run();
     */
    humanize(options?: { speed?: 'slow' | 'normal' | 'fast' }): FluentChain {
        return this.chain().humanize(options);
    }
    
    /**
     * 快捷方式：直接指定窗口开始链式调用
     */
    window(selector: string | WindowSelector): FluentChain {
        return this.chain().window(selector);
    }
    
    // ═══════════════════════════════════════════════════════════════════════════════
    // 原有 API（兼容性保留）
    // ═══════════════════════════════════════════════════════════════════════════════
    
    /**
     * 健康检查
     */
    async health() {
        return this.client.health();
    }
    
    /**
     * 获取窗口列表
     */
    async listWindows() {
        return this.client.listWindows();
    }
    
    /**
     * 截图
     */
    async screenshot(name?: string): Promise<string> {
        return this.screenshotManager.captureAuto();
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 导出
// ═══════════════════════════════════════════════════════════════════════════════

export { FluentChain, ElementInfo } from './fluent-chain';
export { ScreenshotManager } from './screenshot';

// 默认导出
export default ElementSelectorSDKv2;