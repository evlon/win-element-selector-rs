"use strict";
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
var __exportStar = (this && this.__exportStar) || function(m, exports) {
    for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports, p)) __createBinding(exports, m, p);
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.randomFloat = exports.randomInt = exports.sleep = exports.buildWindowSelector = exports.HttpClient = exports.ActionChain = exports.HumanizeContext = exports.ElementSelectorSDK = void 0;
const client_1 = require("./client");
const humanize_context_1 = require("./humanize-context");
const types_1 = require("./types");
const utils_1 = require("./utils");
class ElementSelectorSDK {
    constructor(config) {
        this.client = new client_1.HttpClient({
            baseUrl: config?.baseUrl ?? types_1.DEFAULTS.baseUrl,
            timeout: config?.timeout ?? types_1.DEFAULTS.timeout,
        });
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 基础 API
    // ═══════════════════════════════════════════════════════════════════════════
    async health() {
        return this.client.health();
    }
    async listWindows() {
        return this.client.listWindows();
    }
    async getElement(params) {
        return this.client.getElement(params);
    }
    async moveMouse(target, options) {
        return this.client.moveMouse(target, options);
    }
    async click(params) {
        return this.client.clickMouse(params);
    }
    async type(text, options) {
        return this.client.typeText(text, options);
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 拟人上下文
    // ═══════════════════════════════════════════════════════════════════════════
    async humanize(callback) {
        const ctx = new humanize_context_1.HumanizeContext(this.client);
        return callback(ctx);
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 空闲移动
    // ═══════════════════════════════════════════════════════════════════════════
    async startIdleMotion(params) {
        return this.client.startIdleMotion(params);
    }
    async stopIdleMotion() {
        return this.client.stopIdleMotion();
    }
    async getIdleMotionStatus() {
        return this.client.getIdleMotionStatus();
    }
    // ═══════════════════════════════════════════════════════════════════════════
    // 便捷方法
    // ═══════════════════════════════════════════════════════════════════════════
    static buildWindowSelector(selector) {
        return (0, utils_1.buildWindowSelector)(selector);
    }
}
exports.ElementSelectorSDK = ElementSelectorSDK;
// ═══════════════════════════════════════════════════════════════════════════════
// 导出
// ═══════════════════════════════════════════════════════════════════════════════
__exportStar(require("./types"), exports);
var humanize_context_2 = require("./humanize-context");
Object.defineProperty(exports, "HumanizeContext", { enumerable: true, get: function () { return humanize_context_2.HumanizeContext; } });
var action_chain_1 = require("./action-chain");
Object.defineProperty(exports, "ActionChain", { enumerable: true, get: function () { return action_chain_1.ActionChain; } });
var client_2 = require("./client");
Object.defineProperty(exports, "HttpClient", { enumerable: true, get: function () { return client_2.HttpClient; } });
var utils_2 = require("./utils");
Object.defineProperty(exports, "buildWindowSelector", { enumerable: true, get: function () { return utils_2.buildWindowSelector; } });
Object.defineProperty(exports, "sleep", { enumerable: true, get: function () { return utils_2.sleep; } });
Object.defineProperty(exports, "randomInt", { enumerable: true, get: function () { return utils_2.randomInt; } });
Object.defineProperty(exports, "randomFloat", { enumerable: true, get: function () { return utils_2.randomFloat; } });
exports.default = ElementSelectorSDK;
//# sourceMappingURL=index.js.map