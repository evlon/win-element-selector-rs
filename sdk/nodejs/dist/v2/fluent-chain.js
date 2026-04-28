"use strict";
// sdk/nodejs/src/v2/fluent-chain.ts
// SDK V2 流式链式调用核心实现
Object.defineProperty(exports, "__esModule", { value: true });
exports.FluentChain = void 0;
const types_1 = require("../types");
const utils_1 = require("../utils");
const screenshot_1 = require("./screenshot");
// ═══════════════════════════════════════════════════════════════════════════════
// FluentChain 类 - 流式链式调用
// ═══════════════════════════════════════════════════════════════════════════════
class FluentChain {
    constructor(client) {
        this.actions = [];
        // 当前状态
        this.currentWindowSelector = null;
        this.currentElement = null;
        this.humanizeEnabled = false;
        this.humanizeOptions = {};
        this.debugMode = false;
        this.client = client;
        this.screenshotManager = new screenshot_1.ScreenshotManager();
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 初始化方法
    // ═══════════════════════════════════════════════════════════════════════════
    /**
     * 开启拟人化模式
     */
    humanize(options) {
        this.humanizeEnabled = true;
        this.humanizeOptions = options ?? {};
        this.actions.push({ type: 'humanize', options });
        return this;
    }
    /**
     * 开启调试模式
     */
    debug() {
        this.debugMode = true;
        return this;
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 窗口操作
    // ═══════════════════════════════════════════════════════════════════════════
    /**
     * 激活窗口
     */
    window(selector) {
        const windowSelector = typeof selector === 'string'
            ? selector
            : (0, utils_1.buildWindowSelector)(selector);
        this.currentWindowSelector = windowSelector;
        this.actions.push({ type: 'window', params: windowSelector });
        return this;
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 元素查找
    // ═══════════════════════════════════════════════════════════════════════════
    /**
     * 查找元素 - 找不到自动截图退出
     */
    find(xpath) {
        this.actions.push({ type: 'find', xpath });
        return this;
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 多元素查找和提取
    // ═══════════════════════════════════════════════════════════════════════════
    /**
     * 查找所有匹配元素（返回数组，不执行后续操作）
     */
    async findAll(xpath) {
        if (!this.currentWindowSelector) {
            throw new Error('必须先调用 window() 激活窗口');
        }
        this.log(`findAll("${xpath}")`);
        const result = await this.client.getAllElements({
            windowSelector: this.currentWindowSelector,
            xpath,
            randomRange: types_1.DEFAULTS.click.randomRange,
        });
        if (!result.found) {
            this.log(`  → not found`);
            return [];
        }
        this.log(`  → found ${result.total} elements`);
        return result.elements;
    }
    /**
     * 提取元素属性数组
     * @param attrs 要提取的属性列表 ['name', 'controlType', 'rect']
     */
    async extract(xpath, attrs) {
        const elements = await this.findAll(xpath);
        return elements.map(elem => {
            const result = {};
            for (const attr of attrs) {
                if (attr in elem) {
                    result[attr] = elem[attr];
                }
            }
            return result;
        });
    }
    /**
     * 提取元素文本列表
     */
    async extractList(xpath) {
        const elements = await this.findAll(xpath);
        return elements.map(elem => elem.name);
    }
    /**
     * 提取表格数据
     * TODO: 需要更复杂的逻辑处理表格结构
     */
    async extractTable(xpath) {
        // 简化实现：假设每行是一个元素
        const elements = await this.findAll(xpath);
        return elements.map(elem => [elem.name]);
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 点击操作
    // ═══════════════════════════════════════════════════════════════════════════
    /**
     * 点击当前元素
     */
    click() {
        this.actions.push({ type: 'click' });
        return this;
    }
    /**
     * 双击
     */
    doubleClick() {
        this.actions.push({ type: 'doubleClick' });
        return this;
    }
    /**
     * 右键点击
     */
    rightClick() {
        this.actions.push({ type: 'rightClick' });
        return this;
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 打字操作
    // ═══════════════════════════════════════════════════════════════════════════
    /**
     * 打字
     */
    type(text) {
        this.actions.push({ type: 'type', text });
        return this;
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 等待元素操作
    // ═══════════════════════════════════════════════════════════════════════════
    /**
     * 等待元素出现（轮询检查）
     * @param xpath 元素 XPath
     * @param options timeout: 最大等待时间 (ms), interval: 检查间隔 (ms)
     */
    async waitFor(xpath, options) {
        if (!this.currentWindowSelector) {
            throw new Error('必须先调用 window() 激活窗口');
        }
        const timeout = options?.timeout ?? 10000;
        const interval = options?.interval ?? 500;
        this.log(`waitFor("${xpath}", timeout=${timeout}ms)`);
        const startTime = Date.now();
        while (Date.now() - startTime < timeout) {
            const result = await this.client.getElement({
                windowSelector: this.currentWindowSelector,
                xpath,
                randomRange: types_1.DEFAULTS.click.randomRange,
            });
            if (result.found && result.element) {
                this.log(`  → element appeared after ${Date.now() - startTime}ms`);
                this.currentElement = result.element;
                return result.element;
            }
            await new Promise(r => setTimeout(r, interval));
        }
        await this.failWithScreenshot(`Element did not appear within ${timeout}ms: ${xpath}`, 'waitFor', { windowSelector: this.currentWindowSelector, xpath });
        return null; // unreachable
    }
    /**
     * 等待元素消失（轮询检查）
     */
    async waitUntilGone(xpath, options) {
        if (!this.currentWindowSelector) {
            throw new Error('必须先调用 window() 激活窗口');
        }
        const timeout = options?.timeout ?? 10000;
        const interval = options?.interval ?? 500;
        this.log(`waitUntilGone("${xpath}", timeout=${timeout}ms)`);
        const startTime = Date.now();
        while (Date.now() - startTime < timeout) {
            const result = await this.client.getElement({
                windowSelector: this.currentWindowSelector,
                xpath,
                randomRange: types_1.DEFAULTS.click.randomRange,
            });
            if (!result.found) {
                this.log(`  → element gone after ${Date.now() - startTime}ms`);
                return;
            }
            await new Promise(r => setTimeout(r, interval));
        }
        await this.failWithScreenshot(`Element did not disappear within ${timeout}ms: ${xpath}`, 'waitUntilGone', { windowSelector: this.currentWindowSelector, xpath });
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 等待时间操作
    // ═══════════════════════════════════════════════════════════════════════════
    /**
     * 等待指定时间
     */
    wait(ms, randomMax) {
        const duration = randomMax
            ? ms + Math.random() * (randomMax - ms)
            : ms;
        this.actions.push({ type: 'wait', duration });
        return this;
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 快捷键操作
    // ═══════════════════════════════════════════════════════════════════════════
    /**
     * 执行快捷键
     */
    shortcut(keys) {
        this.actions.push({ type: 'shortcut', text: keys });
        return this;
    }
    /**
     * 执行单个按键
     */
    key(keyName) {
        this.actions.push({ type: 'key', text: keyName });
        return this;
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 查询操作
    // ═══════════════════════════════════════════════════════════════════════════
    /**
     * 获取元素信息
     */
    async inspect() {
        await this.executePrefixActions();
        return this.currentElement;
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 执行
    // ═══════════════════════════════════════════════════════════════════════════
    /**
     * 执行整条链
     */
    async run() {
        await this.executePrefixActions();
        for (const action of this.actions) {
            if (action.type === 'window' || action.type === 'find' || action.type === 'humanize') {
                continue;
            }
            await this.executeAction(action);
        }
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 内部实现
    // ═══════════════════════════════════════════════════════════════════════════
    async executePrefixActions() {
        for (const action of this.actions) {
            if (action.type === 'humanize')
                continue;
            if (action.type === 'window')
                await this.executeWindow(action.params);
            if (action.type === 'find')
                await this.executeFind(action.xpath);
        }
    }
    async executeWindow(windowSelector) {
        this.log(`window("${windowSelector}")`);
        const result = await this.client.activateWindow(windowSelector);
        if (!result.success) {
            await this.failWithScreenshot(`Window not found: ${windowSelector}`, 'window');
        }
        this.log(`  → window activated`);
    }
    async executeFind(xpath) {
        if (!this.currentWindowSelector) {
            throw new Error('必须先调用 window() 激活窗口');
        }
        this.log(`find("${xpath}")`);
        const result = await this.client.getElement({
            windowSelector: this.currentWindowSelector,
            xpath,
            randomRange: types_1.DEFAULTS.click.randomRange,
        });
        if (!result.found || !result.element) {
            await this.failWithScreenshot(`Element not found: ${xpath}`, 'find', { windowSelector: this.currentWindowSelector, xpath });
            return; // process.exit 已调用，但 TypeScript 需要明确的 return
        }
        // result.element 已检查不为 null
        const element = result.element;
        this.currentElement = element;
        this.log(`  → found: rect(${element.rect.x}, ${element.rect.y}, ${element.rect.width}x${element.rect.height})`);
    }
    async executeAction(action) {
        switch (action.type) {
            case 'click':
                await this.executeClick();
                break;
            case 'doubleClick':
                await this.executeClick('double');
                break;
            case 'rightClick':
                await this.executeClick('right');
                break;
            case 'type':
                await this.executeType(action.text);
                break;
            case 'wait':
                await this.executeWait(action.duration);
                break;
            case 'shortcut':
                await this.executeShortcut(action.text);
                break;
            case 'key':
                await this.executeKey(action.text);
                break;
        }
    }
    async executeClick(mode = 'single') {
        if (!this.currentElement)
            throw new Error('必须先调用 find() 找到元素');
        const modeText = mode === 'single' ? 'click' : mode === 'double' ? 'doubleClick' : 'rightClick';
        this.log(`${modeText}()`);
        const target = this.currentElement.centerRandom;
        const duration = this.getHumanizedDuration();
        const result = await this.client.moveMouse(target, {
            humanize: this.humanizeEnabled,
            trajectory: types_1.DEFAULTS.move.trajectory,
            duration,
        });
        if (!result.success) {
            await this.failWithScreenshot(`${modeText} failed`, modeText);
            return;
        }
        this.log(`  → clicked at (${target.x}, ${target.y}), ${result.durationMs}ms`);
    }
    async executeType(text) {
        this.log(`type("${text}")`);
        const charDelay = this.getHumanizedCharDelay();
        const result = await this.client.typeText(text, { charDelay });
        if (!result.success)
            await this.failWithScreenshot(`Type failed`, 'type');
        this.log(`  → typed ${result.charsTyped} chars, ${result.durationMs}ms`);
    }
    async executeWait(duration) {
        this.log(`wait(${Math.round(duration)}ms)`);
        await new Promise(r => setTimeout(r, duration));
        this.log(`  → waited`);
    }
    async executeShortcut(keys) {
        this.log(`shortcut("${keys}")`);
        const result = await this.client.executeShortcut(keys);
        if (!result.success) {
            await this.failWithScreenshot(`Shortcut failed: ${keys}`, 'shortcut');
            return;
        }
        this.log(`  → shortcut executed`);
    }
    async executeKey(keyName) {
        this.log(`key("${keyName}")`);
        const result = await this.client.executeKey(keyName);
        if (!result.success) {
            await this.failWithScreenshot(`Key failed: ${keyName}`, 'key');
            return;
        }
        this.log(`  → key executed`);
    }
    async failWithScreenshot(message, step, context) {
        const screenshotPath = await this.screenshotManager.captureFailure(step);
        console.log('\n' + '='.repeat(60));
        console.log(`[FAILED] ${message}`);
        console.log('='.repeat(60));
        if (context?.windowSelector)
            console.log(`Window selector: ${context.windowSelector}`);
        if (context?.xpath)
            console.log(`XPath: ${context.xpath}`);
        console.log('\nAvailable windows:');
        const windows = await this.client.listWindows();
        windows.slice(0, 5).forEach(w => console.log(`  - ${w.title} (${w.className}, ${w.processName})`));
        console.log(`\nScreenshot saved: ${screenshotPath}`);
        console.log('Process exiting for manual intervention...\n');
        process.exit(1);
    }
    getHumanizedDuration() {
        const base = types_1.DEFAULTS.move.duration;
        if (!this.humanizeEnabled)
            return base;
        const speedFactor = this.humanizeOptions.speed === 'slow' ? 1.5 : this.humanizeOptions.speed === 'fast' ? 0.5 : 1.0;
        return Math.round(base * speedFactor * (0.8 + Math.random() * 0.4));
    }
    getHumanizedCharDelay() {
        if (!this.humanizeEnabled)
            return types_1.DEFAULTS.type.charDelay;
        const base = types_1.DEFAULTS.type.charDelay;
        const speedFactor = this.humanizeOptions.speed === 'slow' ? 2 : this.humanizeOptions.speed === 'fast' ? 0.5 : 1.0;
        return { min: Math.round(base.min * speedFactor), max: Math.round(base.max * speedFactor) };
    }
    log(message) {
        if (this.debugMode)
            console.log(`[DEBUG] ${message}`);
    }
}
exports.FluentChain = FluentChain;
//# sourceMappingURL=fluent-chain.js.map