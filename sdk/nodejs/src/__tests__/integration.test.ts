// sdk/nodejs/src/__tests__/integration.test.ts
// 集成测试 - 需要运行 element-selector-server

import { ElementSelectorSDK } from '../index';

// 集成测试 - 需要服务器运行
const SERVER_URL = 'http://127.0.0.1:8080';

describe('Integration Tests', () => {
    let sdk: ElementSelectorSDK;

    beforeAll(() => {
        sdk = new ElementSelectorSDK({ baseUrl: SERVER_URL });
    });

    // 健康检查测试 - 服务器必须运行
    describe('Health Check', () => {
        test('should connect to server', async () => {
            try {
                const health = await sdk.health();
                expect(health.status).toBe('ok');
                expect(health.service).toBe('element-selector-server');
            } catch (error) {
                // 如果服务器未运行，跳过测试
                console.log('服务器未运行，跳过集成测试');
                throw error;
            }
        });
    });

    // 窗口列表测试
    describe('Window List', () => {
        test('should get window list', async () => {
            try {
                const windows = await sdk.listWindows();
                expect(Array.isArray(windows)).toBe(true);
                // 窗口数量可能为 0，取决于运行环境
                // 只验证结构正确性
                if (windows.length > 0) {
                    const firstWindow = windows[0];
                    expect(firstWindow).toHaveProperty('title');
                    expect(firstWindow).toHaveProperty('className');
                    expect(firstWindow).toHaveProperty('processId');
                    expect(firstWindow).toHaveProperty('processName');
                }
            } catch (error) {
                console.log('服务器未运行，跳过');
                throw error;
            }
        });
    });

    // 空闲移动状态测试
    describe('Idle Motion Status', () => {
        test('should get idle motion status', async () => {
            try {
                const status = await sdk.getIdleMotionStatus();
                expect(status).toHaveProperty('active');
                expect(status).toHaveProperty('paused');
                expect(typeof status.active).toBe('boolean');
                expect(typeof status.paused).toBe('boolean');
            } catch (error) {
                console.log('服务器未运行，跳过');
                throw error;
            }
        });
    });

    // 键盘打字测试
    describe('Keyboard Type', () => {
        test('should type text', async () => {
            try {
                const result = await sdk.type('测试', {
                    charDelay: { min: 10, max: 20 }
                });
                expect(result.success).toBe(true);
                expect(result.charsTyped).toBe(2);
                expect(result.durationMs).toBeGreaterThan(0);
            } catch (error) {
                console.log('服务器未运行，跳过');
                throw error;
            }
        });
    });
});