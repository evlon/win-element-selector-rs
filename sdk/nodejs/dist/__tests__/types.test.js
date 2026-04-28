"use strict";
// sdk/nodejs/src/__tests__/types.test.ts
// 类型定义单元测试
Object.defineProperty(exports, "__esModule", { value: true });
const types_1 = require("../types");
describe('Types', () => {
    describe('DEFAULTS', () => {
        test('should have correct default values', () => {
            expect(types_1.DEFAULTS.baseUrl).toBe('http://127.0.0.1:8080');
            expect(types_1.DEFAULTS.timeout).toBe(30000);
            expect(types_1.DEFAULTS.move.humanize).toBe(true);
            expect(types_1.DEFAULTS.move.trajectory).toBe('bezier');
            expect(types_1.DEFAULTS.move.duration).toBe(600);
            expect(types_1.DEFAULTS.click.randomRange).toBe(0.55);
            expect(types_1.DEFAULTS.idleMotion.moveInterval).toBe(800);
            expect(types_1.DEFAULTS.idleMotion.idleTimeout).toBe(60000);
            expect(types_1.DEFAULTS.type.charDelay.min).toBe(50);
            expect(types_1.DEFAULTS.type.charDelay.max).toBe(150);
        });
    });
    describe('Point', () => {
        test('should create point correctly', () => {
            const point = { x: 100, y: 200 };
            expect(point.x).toBe(100);
            expect(point.y).toBe(200);
        });
    });
    describe('Rect', () => {
        test('should have correct properties', () => {
            const rect = { x: 10, y: 20, width: 100, height: 50 };
            expect(rect.x).toBe(10);
            expect(rect.y).toBe(20);
            expect(rect.width).toBe(100);
            expect(rect.height).toBe(50);
        });
    });
    describe('WindowSelector', () => {
        test('should allow partial fields', () => {
            const selector1 = { title: '微信' };
            expect(selector1.title).toBe('微信');
            expect(selector1.className).toBeUndefined();
            const selector2 = {
                className: 'mmui::MainWindow',
                processName: 'Weixin',
            };
            expect(selector2.className).toBe('mmui::MainWindow');
            expect(selector2.processName).toBe('Weixin');
        });
    });
    describe('IdleMotionParams', () => {
        test('should create with required fields', () => {
            const params = {
                window: { title: 'Test' },
                xpath: '//Button',
            };
            expect(params.window.title).toBe('Test');
            expect(params.xpath).toBe('//Button');
            expect(params.speed).toBeUndefined();
        });
        test('should allow optional fields', () => {
            const params = {
                window: { title: 'Test' },
                xpath: '//Pane',
                speed: 'fast',
                moveInterval: 500,
                idleTimeout: 30000,
                humanIntervention: {
                    enabled: true,
                    pauseOnMouse: false,
                    resumeDelay: 2000,
                },
            };
            expect(params.speed).toBe('fast');
            expect(params.moveInterval).toBe(500);
            expect(params.idleTimeout).toBe(30000);
            expect(params.humanIntervention?.pauseOnMouse).toBe(false);
        });
    });
});
//# sourceMappingURL=types.test.js.map