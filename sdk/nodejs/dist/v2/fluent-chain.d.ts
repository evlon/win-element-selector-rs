import { HttpClient } from '../client';
import { WindowSelector, ElementInfo } from '../types';
export interface ProfileStats {
    totalTime: number;
    steps: {
        step: string;
        time: number;
        xpath?: string;
    }[];
}
export type { ElementInfo } from '../types';
export declare class FluentChain {
    private client;
    private actions;
    private screenshotManager;
    private currentWindowSelector;
    private currentElement;
    private humanizeEnabled;
    private humanizeOptions;
    private debugMode;
    private profileEnabled;
    private profileSteps;
    private profileStartTime;
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
     * 开启性能监控
     */
    profile(): this;
    /**
     * 激活窗口
     */
    window(selector: string | WindowSelector): this;
    /**
     * 查找元素 - 找不到自动截图退出
     */
    find(xpath: string): this;
    /**
     * 查找所有匹配元素（返回数组，不执行后续操作）
     */
    findAll(xpath: string): Promise<ElementInfo[]>;
    /**
     * 提取元素属性数组
     * @param attrs 要提取的属性列表 ['name', 'controlType', 'rect']
     */
    extract(xpath: string, attrs: string[]): Promise<Record<string, unknown>[]>;
    /**
     * 提取元素文本列表
     */
    extractList(xpath: string): Promise<string[]>;
    /**
     * 提取表格数据
     * TODO: 需要更复杂的逻辑处理表格结构
     */
    extractTable(xpath: string): Promise<string[][]>;
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
     * 等待元素出现（轮询检查）
     * @param xpath 元素 XPath
     * @param options timeout: 最大等待时间 (ms), interval: 检查间隔 (ms)
     */
    waitFor(xpath: string, options?: {
        timeout?: number;
        interval?: number;
    }): Promise<ElementInfo>;
    /**
     * 等待元素消失（轮询检查）
     */
    waitUntilGone(xpath: string, options?: {
        timeout?: number;
        interval?: number;
    }): Promise<void>;
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
     * 断言元素存在
     */
    assertExists(xpath: string): Promise<this>;
    /**
     * 断言元素不存在
     */
    assertNotExists(xpath: string): Promise<this>;
    /**
     * 断言元素文本内容
     */
    assertText(xpath: string, expectedText: string): Promise<this>;
    /**
     * 断言元素可见
     */
    assertVisible(xpath: string): Promise<this>;
    /**
     * 断言元素可用
     */
    assertEnabled(xpath: string): Promise<this>;
    /**
     * 检查元素是否存在
     * @returns true 如果元素存在，否则 false
     */
    exists(xpath: string): Promise<boolean>;
    /**
     * 尝试查找元素（不失败）
     * @returns 元素信息如果找到，否则 null
     */
    tryFind(xpath: string): Promise<ElementInfo | null>;
    /**
     * 截取全屏
     * @param outputPath 输出路径
     */
    screenshot(outputPath?: string): Promise<string>;
    /**
     * 截取当前元素
     * @param outputPath 输出路径
     */
    screenshotElement(outputPath?: string): Promise<string>;
    /**
     * 自动命名截图
     */
    screenshotAuto(): Promise<string>;
    private idleRunning;
    /**
     * 启动空闲移动
     * @param options 空闲移动参数
     */
    idle(options: {
        xpath: string;
        speed?: 'slow' | 'normal' | 'fast';
    }): Promise<this>;
    /**
     * 停止空闲移动
     */
    stopIdle(): Promise<this>;
    /**
     * 解析 windowSelector 字符串为 WindowSelector 对象
     */
    private parseWindowSelector;
    /**
     * 获取元素信息
     */
    inspect(): Promise<ElementInfo | null>;
    private retryCount;
    private retryDelay;
    /**
     * 设置重试机制
     * @param count 重试次数
     * @param delayMs 重试间隔 (ms)
     */
    retry(count: number, delayMs?: number): this;
    /**
     * 带重试的执行
     */
    private executeWithRetry;
    /**
     * 执行整条链
     * @returns 如果开启 profile，返回性能统计；否则返回 void
     */
    run(): Promise<ProfileStats | void>;
    private executePrefixActions;
    private executeWindow;
    private executeFind;
    private executeAction;
    private executeClick;
    private executeType;
    private executeWait;
    private executeShortcut;
    private executeKey;
    private failWithScreenshot;
    private getHumanizedDuration;
    private getHumanizedCharDelay;
    private log;
}
//# sourceMappingURL=fluent-chain.d.ts.map