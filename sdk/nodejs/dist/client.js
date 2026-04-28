"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.HttpClient = void 0;
const axios_1 = __importDefault(require("axios"));
const types_1 = require("./types");
class HttpClient {
    constructor(config) {
        this.client = axios_1.default.create({
            baseURL: config.baseUrl,
            timeout: config.timeout ?? types_1.DEFAULTS.timeout,
            headers: {
                'Content-Type': 'application/json',
            },
        });
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
        const response = await this.client.get('/api/element', {
            params: {
                windowSelector: params.windowSelector,
                xpath: params.xpath,
                randomRange: params.randomRange ?? types_1.DEFAULTS.click.randomRange,
            },
        });
        return response.data;
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
        return response.data;
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
    handleError(error) {
        if (axios_1.default.isAxiosError(error)) {
            const axiosError = error;
            const message = axiosError.response?.data?.error
                ?? axiosError.message
                ?? 'Unknown HTTP error';
            return new Error(`SDK Error: ${message}`);
        }
        return error instanceof Error ? error : new Error(String(error));
    }
}
exports.HttpClient = HttpClient;
//# sourceMappingURL=client.js.map