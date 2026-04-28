// sdk/nodejs/examples/basic-usage.ts
// 基本使用示例 - 展示安全操作流程

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

           // 构建精确的窗口选择器
        const windowSelector = {
            title: "Untitled - Notepad",
            className: "Notepad",
            processName:"Notepad",
        };        
        console.log('窗口选择器:', windowSelector);
        
        // **关键**: 先激活窗口，确保它是前台窗口
        const activateResult = await sdk.activateWindow(windowSelector);
        console.log('激活结果:', activateResult.success ? '成功' : '失败');
        console.log();

        // 4. 查找元素
        console.log('=== 4. 查找元素 ===');
        const element = await sdk.getElement({
            windowSelector: `/Window[@Name='Untitled - Notepad' and @ClassName='Notepad']`,
            xpath: '//Document',  // 记事本的编辑区域
            randomRange: 0.55
        });
        
        if (element.found && element.element) {
            console.log('找到元素:');
            console.log(`  位置: (${element.element.rect.x}, ${element.element.rect.y})`);
            console.log(`  大小: ${element.element.rect.width} x ${element.element.rect.height}`);
        } else {
            console.log('未找到元素:', element.error);
        }
        console.log();

        // 5. 鼠标移动
        console.log('=== 5. 鼠标移动 ===');
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

        // 6. 安全点击 - 先激活窗口再点击
        console.log('=== 6. 安全点击 ===');
        const clickResult = await sdk.safeClick({
            window: windowSelector,
            xpath: '//Document',  // 点击编辑区域
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

        // 7. 安全打字 - 先激活窗口并聚焦元素，再打字
        console.log('=== 7. 安全打字 ===');
        const typeResult = await sdk.safeType(
            windowSelector,
            '//Document',  // 记事本编辑区域
            'Hello, 这是自动输入的测试文本！',
            { charDelay: { min: 30, max: 80 } }
        );
        console.log('打字结果:', typeResult.success ? '成功' : '失败');
        console.log(`  字符数: ${typeResult.charsTyped}`);
        console.log(`  耗时: ${typeResult.durationMs}ms`);
        console.log();

        console.log('=== 完成 ===');
        console.log('提示: 使用 safeClick 和 safeType 方法可以确保操作成功');
        console.log('关键步骤: 1) 使用精确的窗口选择器  2) 先激活窗口  3) 再执行操作');

    } catch (error) {
        console.error('错误:', error);
    }
}

main().catch(console.error);