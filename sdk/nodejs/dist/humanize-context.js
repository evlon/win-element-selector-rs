"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.HumanizeContext = void 0;
const types_1 = require("./types");
const action_chain_1 = require("./action-chain");
class HumanizeContext {
    constructor(client) {
        this.client = client;
    }
    async click(params) {
        const mergedParams = {
            ...params,
            options: {
                humanize: params.options?.humanize ?? types_1.DEFAULTS.click.humanize,
                randomRange: params.options?.randomRange ?? types_1.DEFAULTS.click.randomRange,
                pauseBefore: params.options?.pauseBefore ?? types_1.DEFAULTS.click.pauseBefore,
                pauseAfter: params.options?.pauseAfter ?? types_1.DEFAULTS.click.pauseAfter,
            },
        };
        return this.client.clickMouse(mergedParams);
    }
    async move(params) {
        const mergedParams = {
            target: params.target,
            options: {
                humanize: params.options?.humanize ?? types_1.DEFAULTS.move.humanize,
                trajectory: params.options?.trajectory ?? types_1.DEFAULTS.move.trajectory,
                duration: params.options?.duration ?? types_1.DEFAULTS.move.duration,
            },
        };
        return this.client.moveMouse(mergedParams.target, mergedParams.options);
    }
    async type(text, options) {
        return this.client.typeText(text, options);
    }
    async getElement(params) {
        return this.client.getElement(params);
    }
    chain() {
        return new action_chain_1.ActionChain(this);
    }
}
exports.HumanizeContext = HumanizeContext;
//# sourceMappingURL=humanize-context.js.map