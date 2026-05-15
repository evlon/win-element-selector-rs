"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.HttpClient = void 0;
const axios_1 = __importDefault(require("axios"));
const types_1 = require("./types");
const logger_1 = require("./logger");
const errors_1 = require("./errors");
class HttpClient {
    constructor(config) {
        this.client = axios_1.default.create({
            baseURL: config.baseUrl,
            timeout: config.timeout ?? types_1.DEFAULTS.timeout,
            headers: {
                'Content-Type': 'application/json',
            },
        });
        this.logger = (0, logger_1.createLogger)('HttpClient');
    }
    async health() {
        const response = await this.client.get('/api/health');
        return response.data;
    }
    async listWindows() {
        const response = await this.client.post('/api/window/list');
        return response.data.windows;
    }
    async getElement(params) {
        const startTime = Date.now();
        this.logger.debug('GET /api/element', {
            windowSelector: params.windowSelector.substring(0, 50) + '...',
            xpath: params.xpath.substring(0, 80) + '...'
        });
        try {
            const response = await this.client.get('/api/element', {
                params: {
                    windowSelector: params.windowSelector,
                    xpath: params.xpath,
                    randomRange: params.randomRange ?? types_1.DEFAULTS.click.randomRange,
                },
            });
            const duration = Date.now() - startTime;
            this.logger.info('Element query completed', {
                duration,
                found: response.data.found
            });
            return response.data;
        }
        catch (error) {
            this.logger.error('Element query failed', { params, error: error.message });
            throw this.handleError(error, '/api/element');
        }
    }
    async moveMouse(target, options) {
        const response = await this.client.post('/api/mouse/move', {
            target,
            options: options ? {
                humanize: options.humanize ?? types_1.DEFAULTS.move.humanize,
                trajectory: options.trajectory ?? types_1.DEFAULTS.move.trajectory,
                duration: options.duration ?? types_1.DEFAULTS.move.duration,
            } : undefined,
        });
        return response.data;
    }
    async clickMouse(params) {
        const startTime = Date.now();
        this.logger.debug('POST /api/mouse/click', {
            window: params.window,
            xpath: params.xpath.substring(0, 80) + '...'
        });
        try {
            const response = await this.client.post('/api/mouse/click', {
                window: params.window,
                xpath: params.xpath,
                options: params.options ? {
                    humanize: params.options.humanize ?? types_1.DEFAULTS.click.humanize,
                    randomRange: params.options.randomRange ?? types_1.DEFAULTS.click.randomRange,
                    pauseBefore: params.options.pauseBefore ?? types_1.DEFAULTS.click.pauseBefore,
                    pauseAfter: params.options.pauseAfter ?? types_1.DEFAULTS.click.pauseAfter,
                } : undefined,
            });
            const duration = Date.now() - startTime;
            this.logger.info('Click completed', {
                duration,
                success: response.data.success,
                clickPoint: response.data.success ? response.data.clickPoint : undefined
            });
            return response.data;
        }
        catch (error) {
            this.logger.error('Click failed', { params, error: error.message });
            throw this.handleError(error, '/api/mouse/click');
        }
    }
    async startIdleMotion(params) {
        await this.client.post('/api/mouse/idle/start', {
            window: params.window,
            xpath: params.xpath,
            speed: params.speed ?? types_1.DEFAULTS.idleMotion.speed,
            moveInterval: params.moveInterval ?? types_1.DEFAULTS.idleMotion.moveInterval,
            idleTimeout: params.idleTimeout ?? types_1.DEFAULTS.idleMotion.idleTimeout,
            humanIntervention: params.humanIntervention ? {
                enabled: params.humanIntervention.enabled,
                pauseOnMouse: params.humanIntervention.pauseOnMouse ?? types_1.DEFAULTS.idleMotion.humanIntervention.pauseOnMouse,
                pauseOnKeyboard: params.humanIntervention.pauseOnKeyboard ?? types_1.DEFAULTS.idleMotion.humanIntervention.pauseOnKeyboard,
                resumeDelay: params.humanIntervention.resumeDelay ?? types_1.DEFAULTS.idleMotion.humanIntervention.resumeDelay,
            } : types_1.DEFAULTS.idleMotion.humanIntervention,
        });
    }
    async stopIdleMotion() {
        const response = await this.client.post('/api/mouse/idle/stop');
        return response.data;
    }
    async getIdleMotionStatus() {
        const response = await this.client.get('/api/mouse/idle/status');
        return response.data;
    }
    async typeText(text, options) {
        const response = await this.client.post('/api/keyboard/type', {
            text,
            charDelay: options?.charDelay ?? types_1.DEFAULTS.type.charDelay,
        });
        return response.data;
    }
    /**
     * 激活指定窗口（使其成为前台窗口）
     * @param windowSelector 窗口选择器 XPath
     * @returns 激活结果
     */
    async activateWindow(windowSelector) {
        const response = await this.client.post('/api/window/activate', {
            windowSelector,
        });
        return response.data;
    }
    /**
     * 激活窗口并使指定元素获得焦点
     * @param windowSelector 窗口选择器 XPath
     * @param xpath 元素 XPath
     * @returns 操作结果
     */
    async focusElement(windowSelector, xpath) {
        const response = await this.client.post('/api/window/focus-element', {
            windowSelector,
            xpath,
        });
        return response.data;
    }
    /**
     * 获取所有匹配元素
     * @param params 查询参数
     * @returns 所有匹配的元素列表
     */
    async getAllElements(params) {
        const response = await this.client.get('/api/element/all', {
            params: {
                windowSelector: params.windowSelector,
                xpath: params.xpath,
                randomRange: params.randomRange ?? types_1.DEFAULTS.click.randomRange,
            },
        });
        return response.data;
    }
    /**
     * 执行快捷键组合
     * @param keys 快捷键字符串，如 "Ctrl+C", "Alt+F4"
     */
    async executeShortcut(keys) {
        const response = await this.client.post('/api/keyboard/shortcut', {
            keys,
        });
        return response.data;
    }
    /**
     * 执行单个按键
     * @param key 按键名称，如 "Enter", "Tab", "Escape"
     */
    async executeKey(key) {
        const response = await this.client.post('/api/keyboard/key', {
            key,
        });
        return response.data;
    }
    handleError(error, endpoint) {
        if (axios_1.default.isAxiosError(error)) {
            const axiosError = error;
            // 超时错误
            if (axiosError.code === 'ECONNABORTED') {
                throw new errors_1.TimeoutError(endpoint || 'unknown', this.client.defaults.timeout || 30000);
            }
            // 网络错误（无响应）
            if (!axiosError.response) {
                throw new errors_1.NetworkError(axiosError, endpoint || 'unknown');
            }
            // HTTP 错误
            const message = axiosError.response?.data?.error ?? axiosError.message;
            throw new errors_1.SDKError(`HTTP ${axiosError.response.status}: ${message}`, `HTTP_${axiosError.response.status}`, {
                endpoint,
                status: axiosError.response.status,
                responseData: axiosError.response.data
            });
        }
        // 其他错误
        if (error instanceof Error) {
            throw new errors_1.SDKError(error.message, 'UNKNOWN_ERROR', { stack: error.stack });
        }
        throw new errors_1.SDKError(String(error), 'UNKNOWN_ERROR');
    }
}
exports.HttpClient = HttpClient;
//# sourceMappingURL=client.js.map