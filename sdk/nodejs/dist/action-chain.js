"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.ActionChain = void 0;
class ActionChain {
    constructor(context) {
        this.context = context;
        this.actions = [];
    }
    click(params) {
        this.actions.push({ type: 'click', params });
        return this;
    }
    type(text, options) {
        this.actions.push({ type: 'type', text, options });
        return this;
    }
    move(target, options) {
        this.actions.push({ type: 'move', params: { target, options } });
        return this;
    }
    wait(ms) {
        this.actions.push({ type: 'wait', duration: ms });
        return this;
    }
    async execute() {
        for (const action of this.actions) {
            switch (action.type) {
                case 'click':
                    await this.context.click(action.params);
                    break;
                case 'type':
                    await this.context.type(action.text, action.options);
                    break;
                case 'move':
                    const moveParams = action.params;
                    await this.context.move({
                        target: moveParams.target,
                        options: moveParams.options,
                    });
                    break;
                case 'wait':
                    await new Promise(resolve => setTimeout(resolve, action.duration));
                    break;
            }
        }
    }
}
exports.ActionChain = ActionChain;
//# sourceMappingURL=action-chain.js.map