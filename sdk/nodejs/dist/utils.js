"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.sleep = sleep;
exports.randomInt = randomInt;
exports.randomFloat = randomFloat;
exports.buildWindowSelector = buildWindowSelector;
function sleep(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
}
function randomInt(min, max) {
    return Math.floor(Math.random() * (max - min + 1)) + min;
}
function randomFloat(min, max) {
    return Math.random() * (max - min) + min;
}
function buildWindowSelector(selector) {
    const predicates = [];
    if (selector.title) {
        predicates.push(`@Name='${selector.title}'`);
    }
    if (selector.className) {
        predicates.push(`@ClassName='${selector.className}'`);
    }
    if (selector.processName) {
        predicates.push(`@ProcessName='${selector.processName}'`);
    }
    if (predicates.length === 0) {
        return 'Window';
    }
    return `Window[${predicates.join(' and ')}]`;
}
//# sourceMappingURL=utils.js.map