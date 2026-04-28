// sdk/nodejs/src/__tests__/utils.test.ts
// 工具函数单元测试

import {
    sleep,
    randomInt,
    randomFloat,
    buildWindowSelector,
} from '../utils';

describe('Utils', () => {
    describe('sleep', () => {
        test('should resolve after specified time', async () => {
            const start = Date.now();
            await sleep(100);
            const elapsed = Date.now() - start;
            expect(elapsed).toBeGreaterThanOrEqual(90); // Allow some tolerance
        });
    });

    describe('randomInt', () => {
        test('should return integer within range', () => {
            for (let i = 0; i < 100; i++) {
                const result = randomInt(0, 10);
                expect(result).toBeGreaterThanOrEqual(0);
                expect(result).toBeLessThanOrEqual(10);
                expect(Number.isInteger(result)).toBe(true);
            }
        });

        test('should handle single value range', () => {
            const result = randomInt(5, 5);
            expect(result).toBe(5);
        });
    });

    describe('randomFloat', () => {
        test('should return float within range', () => {
            for (let i = 0; i < 100; i++) {
                const result = randomFloat(0.0, 1.0);
                expect(result).toBeGreaterThanOrEqual(0.0);
                expect(result).toBeLessThanOrEqual(1.0);
            }
        });
    });

    describe('buildWindowSelector', () => {
        test('should build selector with title', () => {
            const result = buildWindowSelector({ title: '微信' });
            expect(result).toBe("Window[@Name='微信']");
        });

        test('should build selector with multiple predicates', () => {
            const result = buildWindowSelector({
                title: '微信',
                className: 'mmui::MainWindow',
            });
            expect(result).toBe("Window[@Name='微信' and @ClassName='mmui::MainWindow']");
        });

        test('should build selector with all predicates', () => {
            const result = buildWindowSelector({
                title: 'Test',
                className: 'TestClass',
                processName: 'TestApp',
            });
            expect(result).toContain("@Name='Test'");
            expect(result).toContain("@ClassName='TestClass'");
            expect(result).toContain("@ProcessName='TestApp'");
        });

        test('should return Window for empty selector', () => {
            const result = buildWindowSelector({});
            expect(result).toBe('Window');
        });
    });
});