// sdk/nodejs/src/index.ts
// Element Selector SDK - 流式 XPath 自动化

import { HttpClient } from './client';
import { Chain, ElementInfo, ProfileStats } from './chain';
import { SDKConfig, DEFAULTS, WindowSelector } from './types';
import { buildWindowSelector } from './utils';

// ═══════════════════════════════════════════════════════════════════════════════
// SDK 入口
// ═══════════════════════════════════════════════════════════════════════════════

/**
 * Element Selector SDK
 * 
 * 流式 XPath 自动化，简单直接，失败自动截图退出。
 * 
 * @example
 * import { SDK } from 'element-selector-sdk';
 * 
 * const sdk = new SDK();
 * 
 * // 基础用法
 * await sdk.flow()
 *     .window("微信")
 *     .find("//Edit[@Name='输入']")
 *     .click()
 *     .type("你好")
 *     .run();
 * 
 * // 拟人化
 * await sdk.flow()
 *     .humanize({ speed: 'slow' })
 *     .window("微信")
 *     .find("//Edit[@Name='输入']")
 *     .click()
 *     .type("你好")
 *     .run();
 * 
 * // 等待元素
 * await sdk.flow()
 *     .window("Chrome")
 *     .waitFor("//Button[@Name='登录']", { timeout: 10000 })
 *     .click()
 *     .run();
 * 
 * // 数据提取
 * const items = await sdk.flow().window("微信").findAll("//ListItem");
 * const texts = await sdk.flow().window("微信").extractList("//ListItem");
 */
export class SDK {
    private client: HttpClient;
    
    constructor(config?: Partial<SDKConfig>) {
        this.client = new HttpClient({
            baseUrl: config?.baseUrl ?? DEFAULTS.baseUrl,
            timeout: config?.timeout ?? DEFAULTS.timeout,
        });
    }
    
    /**
     * 创建流式链式调用
     */
    flow(): Chain {
        return new Chain(this.client);
    }
    
    /**
     * 快捷方式：开启拟人化
     */
    humanize(options?: { speed?: 'slow' | 'normal' | 'fast' }): Chain {
        return this.flow().humanize(options);
    }
    
    /**
     * 快捷方式：指定窗口
     */
    window(selector: string | WindowSelector): Chain {
        return this.flow().window(selector);
    }
    
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
}

// ═══════════════════════════════════════════════════════════════════════════════
// 导出
// ═══════════════════════════════════════════════════════════════════════════════

// 类导出（值）
export { Chain } from './chain';

// 类型导出
export type { ElementInfo, ProfileStats } from './chain';

export { DEFAULTS } from './types';
export type {
    SDKConfig,
    WindowSelector,
    WindowInfo,
    Point,
    Rect,
} from './types';

// 工具导出
export { buildWindowSelector } from './utils';

// 默认导出
export default SDK;