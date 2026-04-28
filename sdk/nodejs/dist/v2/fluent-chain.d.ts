import { HttpClient } from '../client';
import { WindowSelector, ElementInfo } from '../types';
export { ElementInfo } from '../types';
export declare class FluentChain {
    private client;
    private actions;
    private screenshotManager;
    private currentWindowSelector;
    private currentElement;
    private humanizeEnabled;
    private humanizeOptions;
    private debugMode;
    constructor(client: HttpClient);
    /**
     * 开启拟人化模式
     */
    humanize(options?: {
        speed?: 'slow' | 'normal' | 'fast';
    }): this;
    /**
     * 开启调试模式
     */
    debug(): this;
    /**
     * 激活窗口
     */
    window(selector: string | WindowSelector): this;
    /**
     * 查找元素 - 找不到自动截图退出
     */
    find(xpath: string): this;
    /**
     * 点击当前元素
     */
    click(): this;
    /**
     * 双击
     */
    doubleClick(): this;
    /**
     * 右键点击
     */
    rightClick(): this;
    /**
     * 打字
     */
    type(text: string): this;
    /**
     * 等待指定时间
     */
    wait(ms: number, randomMax?: number): this;
    /**
     * 执行快捷键
     */
    shortcut(keys: string): this;
    /**
     * 执行单个按键
     */
    key(keyName: string): this;
    /**
     * 获取元素信息
     */
    inspect(): Promise<ElementInfo | null>;
    /**
     * 执行整条链
     */
    run(): Promise<void>;
    private executePrefixActions;
    private executeWindow;
    private executeFind;
    private executeAction;
    private executeClick;
    private executeType;
    private executeWait;
    private failWithScreenshot;
    private getHumanizedDuration;
    private getHumanizedCharDelay;
    private log;
}
//# sourceMappingURL=fluent-chain.d.ts.map