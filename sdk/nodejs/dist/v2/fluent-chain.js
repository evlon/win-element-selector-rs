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
    // 等待操作
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
                this.log(`shortcut("${action.text}") - TODO`);
                break;
            case 'key':
                this.log(`key("${action.text}") - TODO`);
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
        if (!result.success)
            await this.failWithScreenshot(`${modeText} failed`, modeText);
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