import { Chain } from './chain';
import { SDKConfig, WindowSelector } from './types';
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
export declare class SDK {
    private client;
    constructor(config?: Partial<SDKConfig>);
    /**
     * 创建流式链式调用
     */
    flow(): Chain;
    /**
     * 快捷方式：开启拟人化
     */
    humanize(options?: {
        speed?: 'slow' | 'normal' | 'fast';
    }): Chain;
    /**
     * 快捷方式：指定窗口
     */
    window(selector: string | WindowSelector): Chain;
    /**
     * 健康检查
     */
    health(): Promise<import("./types").HealthStatus>;
    /**
     * 获取窗口列表
     */
    listWindows(): Promise<import("./types").WindowInfo[]>;
}
export { Chain } from './chain';
export type { ElementInfo, ProfileStats } from './chain';
export { createLogger, Logger, LogConfig } from './logger';
export type { LogLevel } from './logger';
export { SDKError, ElementNotFoundError, WindowNotFoundError, NetworkError, TimeoutError, ActionFailedError, InvalidArgumentError, StateError, isSDKError, isElementNotFoundError, isWindowNotFoundError } from './errors';
export { DEFAULTS } from './types';
export type { SDKConfig, WindowSelector, WindowInfo, Point, Rect, } from './types';
export { buildWindowSelector } from './utils';
export default SDK;
//# sourceMappingURL=index.d.ts.map