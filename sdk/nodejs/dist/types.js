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
    timeout: 30000,
    move: {
        humanize: true,
        trajectory: 'bezier',
        duration: 600,
    },
    click: {
        humanize: true,
        randomRange: 0.55,
        pauseBefore: 0,
        pauseAfter: 0,
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