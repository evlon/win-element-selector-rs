// sdk/nodejs/examples/e2e-test.ts
// 端到端自动化测试 - 记事本打字

import { SDK } from '../src';

async function main() {
    const sdk = new SDK();

    console.log('=== 端到端自动化测试 ===');
    console.log();

    // 1. 获取窗口信息
    console.log('1. 获取记事本窗口信息...');
    const windows = await sdk.listWindows();
    const notepad = windows.find(w => w.processName === 'Notepad');
    
    if (!notepad) {
        console.log('请先打开记事本！');
        return;
    }
    console.log(`记事本: ${notepad.title}`);
    
    // 使用 className 和 processName 选择（不依赖 title，因为打字后 title 会变）
    const winSelector = `Window[@ClassName='${notepad.className}' and @ProcessName='${notepad.processName}']`;
    console.log(`窗口选择器: ${winSelector}`);
    console.log();

    // 2. 测试流式链式调用 - 打字
    console.log('2. 测试流式链式调用...');
    const stats = await sdk.flow()
        .profile()                      // 开启性能监控
        .humanize({ speed: 'fast' })    // 快速拟人化
        .debug()                        // 调试日志
        .window(winSelector)
        .find('//Document')             // 查找编辑区域
        .click()                        // 点击
        .type('Element Selector SDK 端到端测试')  // 打字
        .wait(300)                      // 等待
        .shortcut('Ctrl+A')             // 全选
        .wait(200)
        .shortcut('Ctrl+C')             // 复制
        .wait(200)
        .key('Escape')                  // 取消选择
        .run();
    
    if (stats) {
        console.log(`总耗时: ${stats.totalTime}ms`);
        console.log(`步骤数: ${stats.steps.length}`);
    }
    console.log();

    // 3. 测试等待元素
    console.log('3. 测试 waitFor...');
    const elem = await sdk.flow()
        .window(winSelector)
        .waitFor('//Document', { timeout: 5000 });
    console.log(`找到元素: ${elem.name}`);
    console.log();

    // 4. 测试断言
    console.log('4. 测试断言...');
    await sdk.flow().window(winSelector).assertExists('//Document');
    await sdk.flow().window(winSelector).assertEnabled('//Document');
    await sdk.flow().window(winSelector).assertVisible('//Document');
    console.log('断言通过！');
    console.log();

    // 5. 测试元素信息查询
    console.log('5. 测试元素信息查询...');
    const info = await sdk.flow()
        .window(winSelector)
        .find('//Document')
        .inspect();
    
    if (info) {
        console.log('元素信息:');
        console.log(`  控件类型: ${info.controlType}`);
        console.log(`  名称: ${info.name}`);
        console.log(`  位置: (${info.rect.x}, ${info.rect.y})`);
        console.log(`  大小: ${info.rect.width}x${info.rect.height}`);
        console.log(`  可用: ${info.isEnabled}`);
    }
    console.log();

    // 6. 测试条件判断
    console.log('6. 测试条件判断...');
    const exists = await sdk.flow().window(winSelector).exists('//Document');
    console.log(`Document 存在: ${exists}`);
    
    const notExists = await sdk.flow().window(winSelector).exists('//Button[@Name="不存在"]');
    console.log(`不存在按钮: ${notExists}`);
    console.log();

    // 7. 测试重试
    console.log('7. 测试重试机制...');
    await sdk.flow()
        .retry(3, 1000)                  // 失败重试3次，间隔1秒
        .window(winSelector)
        .find('//Document')
        .click();
    console.log('重试机制测试完成');
    console.log();

    // 8. 清空内容
    console.log('8. 清空内容...');
    await sdk.flow()
        .window(winSelector)
        .find('//Document')
        .click()
        .shortcut('Ctrl+A')
        .key('Delete')
        .run();
    console.log('内容已清空');
    console.log();

    console.log('=== 端到端测试完成 ===');
}

main().catch(console.error);