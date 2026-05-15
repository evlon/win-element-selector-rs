"use strict";
// ═══════════════════════════════════════════════════════════════════════════════
// 基础类型
// ═══════════════════════════════════════════════════════════════════════════════
Object.defineProperty(exports, "__esModule", { value: true });
exports.DEFAULTS = void 0;
// ═══════════════════════════════════════════════════════════════════════════════
// 默认值
// ═══════════════════════════════════════════════════════════════════════════════
exports.DEFAULTS = {
    baseUrl: 'http://127.0.0.1:8080',
    timeout: 60000, // 增加到 60 秒，避免长时间操作超时
    move: {
        humanize: true,
        trajectory: 'bezier',
        duration: 600,
    },
    click: {
        humanize: true,
        randomRange: 0.55,
        pauseBefore: 150, // 点击前等待 150ms，让鼠标稳定
        pauseAfter: 200, // 点击后等待 200ms，给应用响应时间
    },
    idleMotion: {
        speed: 'normal',
        moveInterval: 800,
        idleTimeout: 60000,
        humanIntervention: {
            enabled: true,
            pauseOnMouse: true,
            pauseOnKeyboard: true,
            resumeDelay: 3000,
        },
    },
    type: {
        charDelay: {
            min: 50,
            max: 150,
        },
    },
};
//# sourceMappingURL=types.js.map