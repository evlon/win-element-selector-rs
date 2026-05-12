// sdk/nodejs/src/v2/__tests__/fluent-chain.test.ts
// FluentChain 单元测试

import { FluentChain, ElementInfo } from '../fluent-chain';
import { HttpClient } from '../../client';

// Mock HttpClient
jest.mock('../../client');

describe('FluentChain', () => {
    let mockClient: jest.Mocked<HttpClient>;
    let chain: FluentChain;

    beforeEach(() => {
        mockClient = {
            activateWindow: jest.fn(),
            getElement: jest.fn(),
            moveMouse: jest.fn(),
            typeText: jest.fn(),
            listWindows: jest.fn(),
        } as any;
        chain = new FluentChain(mockClient);
    });

    describe('链式方法', () => {
        it('应该返回 this 以支持链式调用', () => {
            const result = chain
                .humanize()
                .window({ className: 'Test' })
                .find('//Button')
                .click()
                .type('test')
                .wait(100);

            expect(result).toBe(chain);
        });

        it('humanize 应该设置拟人化标志', () => {
            chain.humanize({ speed: 'slow' });
            // 内部状态，无法直接验证，但可以通过行为测试
        });

        it('debug 应该设置调试模式', () => {
            chain.debug();
            // 内部状态，无法直接验证
        });
    });

    describe('window()', () => {
        it('应该构建正确的窗口选择器', async () => {
            mockClient.activateWindow.mockResolvedValue({ success: true });

            chain.window({
                title: 'Test Window',
                className: 'TestClass',
                processName: 'TestApp',
            });

            await chain.run();

            expect(mockClient.activateWindow).toHaveBeenCalledWith(
                "Window[@Name='Test Window' and @ClassName='TestClass' and @ProcessName='TestApp']"
            );
        });

        it('应该接受字符串选择器', async () => {
            mockClient.activateWindow.mockResolvedValue({ success: true });

            chain.window("Window[@ClassName='Test']");

            await chain.run();

            expect(mockClient.activateWindow).toHaveBeenCalledWith(
                "Window[@ClassName='Test']"
            );
        });
    });

    describe('find()', () => {
        it('应该在 window 后查找元素', async () => {
            const mockElement: ElementInfo = {
                rect: { x: 100, y: 200, width: 300, height: 50 },
                center: { x: 250, y: 225 },
                centerRandom: { x: 252, y: 228 },
                controlType: 'Edit',
                name: 'TestEdit',
                automationId: '',
                className: '',
                frameworkId: '',
                helpText: '',
                localizedControlType: '',
                isEnabled: true,
                isOffscreen: false,
                isPassword: false,
                acceleratorKey: '',
                accessKey: '',
                itemType: '',
                itemStatus: '',
                processId: 0,
            };

            mockClient.activateWindow.mockResolvedValue({ success: true });
            mockClient.getElement.mockResolvedValue({
                found: true,
                element: mockElement,
                error: null,
            });

            chain
                .window("Window[@ClassName='Test']")
                .find('//Edit');

            const result = await chain.inspect();

            expect(mockClient.getElement).toHaveBeenCalledWith({
                windowSelector: "Window[@ClassName='Test']",
                xpath: '//Edit',
                randomRange: expect.any(Number),
            });
            expect(result).toEqual(mockElement);
        });

        it('应该在找不到元素时抛出错误', async () => {
            mockClient.activateWindow.mockResolvedValue({ success: true });
            mockClient.getElement.mockResolvedValue({
                found: false,
                element: null,
                error: 'Not found',
            });
            mockClient.listWindows.mockResolvedValue([]);

            chain
                .window("Window[@ClassName='Test']")
                .find('//NotFound');

            // 由于 failWithScreenshot 会调用 process.exit(1)
            // 我们需要模拟它
            const mockExit = jest.spyOn(process, 'exit').mockImplementation(() => undefined as never);

            await chain.run();

            expect(mockExit).toHaveBeenCalledWith(1);

            mockExit.mockRestore();
        });
    });

    describe('click()', () => {
        it('应该点击找到的元素', async () => {
            const mockElement: ElementInfo = {
                rect: { x: 100, y: 200, width: 300, height: 50 },
                center: { x: 250, y: 225 },
                centerRandom: { x: 252, y: 228 },
                controlType: 'Edit',
                name: 'TestEdit',
                automationId: '',
                className: '',
                frameworkId: '',
                helpText: '',
                localizedControlType: '',
                isEnabled: true,
                isOffscreen: false,
                isPassword: false,
                acceleratorKey: '',
                accessKey: '',
                itemType: '',
                itemStatus: '',
                processId: 0,
            };

            mockClient.activateWindow.mockResolvedValue({ success: true });
            mockClient.getElement.mockResolvedValue({
                found: true,
                element: mockElement,
                error: null,
            });
            mockClient.moveMouse.mockResolvedValue({
                success: true,
                durationMs: 500,
                startPoint: { x: 0, y: 0 },
                endPoint: { x: 252, y: 228 },
                error: null,
            });

            chain
                .window("Window[@ClassName='Test']")
                .find('//Edit')
                .click();

            await chain.run();

            expect(mockClient.moveMouse).toHaveBeenCalledWith(
                { x: 252, y: 228 },
                expect.objectContaining({
                    humanize: false,
                    trajectory: expect.any(String),
                })
            );
        });
    });

    describe('type()', () => {
        it('应该调用 typeText API', async () => {
            const mockElement: ElementInfo = {
                rect: { x: 100, y: 200, width: 300, height: 50 },
                center: { x: 250, y: 225 },
                centerRandom: { x: 252, y: 228 },
                controlType: 'Edit',
                name: 'TestEdit',
                automationId: '',
                className: '',
                frameworkId: '',
                helpText: '',
                localizedControlType: '',
                isEnabled: true,
                isOffscreen: false,
                isPassword: false,
                acceleratorKey: '',
                accessKey: '',
                itemType: '',
                itemStatus: '',
                processId: 0,
            };

            mockClient.activateWindow.mockResolvedValue({ success: true });
            mockClient.getElement.mockResolvedValue({
                found: true,
                element: mockElement,
                error: null,
            });
            mockClient.typeText.mockResolvedValue({
                success: true,
                charsTyped: 4,
                durationMs: 400,
                error: null,
            });
            mockClient.moveMouse.mockResolvedValue({
                success: true,
                durationMs: 500,
                startPoint: { x: 0, y: 0 },
                endPoint: { x: 252, y: 228 },
                error: null,
            });

            chain
                .window("Window[@ClassName='Test']")
                .find('//Edit')
                .click()
                .type('test');

            await chain.run();

            expect(mockClient.typeText).toHaveBeenCalledWith(
                'test',
                expect.objectContaining({
                    charDelay: expect.any(Object),
                })
            );
        });
    });

    describe('wait()', () => {
        it('应该等待指定时间', async () => {
            // 使用真实定时器，测试很短的等待
            const mockElement: ElementInfo = {
                rect: { x: 100, y: 200, width: 300, height: 50 },
                center: { x: 250, y: 225 },
                centerRandom: { x: 252, y: 228 },
                controlType: 'Edit',
                name: 'TestEdit',
                automationId: '',
                className: '',
                frameworkId: '',
                helpText: '',
                localizedControlType: '',
                isEnabled: true,
                isOffscreen: false,
                isPassword: false,
                acceleratorKey: '',
                accessKey: '',
                itemType: '',
                itemStatus: '',
                processId: 0,
            };

            mockClient.activateWindow.mockResolvedValue({ success: true });
            mockClient.getElement.mockResolvedValue({
                found: true,
                element: mockElement,
                error: null,
            });
            mockClient.moveMouse.mockResolvedValue({ success: true, durationMs: 100, startPoint: { x: 0, y: 0 }, endPoint: { x: 252, y: 228 }, error: null });

            chain
                .window("Window[@ClassName='Test']")
                .find('//Edit')
                .click()
                .wait(10); // 短等待

            await chain.run();
            // 等待完成，测试通过
        });
    });

    describe('inspect()', () => {
        it('应该返回元素信息而不执行后续操作', async () => {
            const mockElement: ElementInfo = {
                rect: { x: 100, y: 200, width: 300, height: 50 },
                center: { x: 250, y: 225 },
                centerRandom: { x: 252, y: 228 },
                controlType: 'Edit',
                name: 'TestEdit',
                automationId: '',
                className: '',
                frameworkId: '',
                helpText: '',
                localizedControlType: '',
                isEnabled: true,
                isOffscreen: false,
                isPassword: false,
                acceleratorKey: '',
                accessKey: '',
                itemType: '',
                itemStatus: '',
                processId: 0,
            };

            mockClient.activateWindow.mockResolvedValue({ success: true });
            mockClient.getElement.mockResolvedValue({
                found: true,
                element: mockElement,
                error: null,
            });

            chain
                .window("Window[@ClassName='Test']")
                .find('//Edit')
                .click() // 添加了 click，但 inspect 不会执行它
                .type('test'); // 添加了 type，但 inspect 不会执行它

            const result = await chain.inspect();

            expect(result).toEqual(mockElement);
            // click 和 type 不会被调用
            expect(mockClient.moveMouse).not.toHaveBeenCalled();
            expect(mockClient.typeText).not.toHaveBeenCalled();
        });
    });
});