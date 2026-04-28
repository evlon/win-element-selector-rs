// sdk/nodejs/examples/basic-usage.ts
// 基本使用示例

import { ElementSelectorSDK } from '../src';

async function main() {
    // 创建 SDK 客户端
    const sdk = new ElementSelectorSDK({
        baseUrl: 'http://127.0.0.1:8080',
        timeout: 30000
    });

    try {
        // 1. 健康检查
        console.log('=== 1. 健康检查 ===');
        const health = await sdk.health();
        console.log('服务状态:', health);
        console.log();

        // 2. 获取窗口列表
        console.log('=== 2. 窗口列表 ===');
        const windows = await sdk.listWindows();
        console.log(`找到 ${windows.length} 个窗口`);
        windows.slice(0, 5).forEach(w => {
            console.log(`  - ${w.title} (${w.processName})`);
        });
        console.log();

        // 3. 查找元素
        console.log('=== 3. 查找元素 ===');
        // 示例：查找记事本的编辑框
        const windowSelector = "Window[@ClassName='Notepad']";
        const xpath = "//Edit[@ClassName='Edit']";
        
        const element = await sdk.getElement({
            windowSelector,
            xpath,
            randomRange: 0.55
        });
        
        if (element.found && element.element) {
            console.log('找到元素:');
            console.log(`  位置: (${element.element.rect.x}, ${element.element.rect.y})`);
            console.log(`  大小: ${element.element.rect.width} x ${element.element.rect.height}`);
            console.log(`  中心: (${element.element.center.x}, ${element.element.center.y})`);
        } else {
            console.log('未找到元素:', element.error);
        }
        console.log();

        // 4. 鼠标移动
        console.log('=== 4. 鼠标移动 ===');
        if (element.found && element.element) {
            const moveResult = await sdk.moveMouse(element.element.centerRandom, {
                humanize: true,
                trajectory: 'bezier',
                duration: 600
            });
            console.log('移动结果:', moveResult.success ? '成功' : '失败');
            console.log(`  耗时: ${moveResult.durationMs}ms`);
        }
        console.log();

        // 5. 点击元素
        console.log('=== 5. 点击元素 ===');
        const clickResult = await sdk.click({
            window: { className: 'Notepad' },
            xpath: '//Edit',
            options: {
                humanize: true,
                randomRange: 0.55,
                pauseBefore: 100,
                pauseAfter: 100
            }
        });
        console.log('点击结果:', clickResult.success ? '成功' : '失败');
        if (!clickResult.success) {
            console.log('  错误:', clickResult.error);
        }
        console.log();

        // 6. 打字
        console.log('=== 6. 打字 ===');
        const typeResult = await sdk.type('Hello, 这是自动输入的测试文本！', {
            charDelay: { min: 30, max: 80 }
        });
        console.log('打字结果:', typeResult.success ? '成功' : '失败');
        console.log(`  字符数: ${typeResult.charsTyped}`);
        console.log(`  耗时: ${typeResult.durationMs}ms`);
        console.log();

    } catch (error) {
        console.error('错误:', error);
    }
}

main().catch(console.error);