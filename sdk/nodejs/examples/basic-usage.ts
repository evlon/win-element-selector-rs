// sdk/nodejs/examples/basic-usage.ts
// Element Selector SDK 流式调用示例

import { SDK, ProfileStats } from '../src';

async function main() {
    const sdk = new SDK();

    try {
        console.log('=== Element Selector SDK 流式调用示例 ===');
        console.log();

        // 1. 健康检查
        console.log('1. 健康检查...');
        const health = await sdk.health();
        console.log(`   服务状态: ${health.status}`);
        console.log();

        // 2. 获取窗口列表
        console.log('2. 获取窗口列表...');
        const windows = await sdk.listWindows();
        const notepad = windows.find(w => w.processName === 'Notepad');
        
        if (!notepad) {
            console.log('   请先打开一个记事本窗口！');
            return;
        }
        console.log(`   找到记事本: ${notepad.title}`);
        console.log();

        // 3. 流式调用 - 基础示例
        console.log('3. 流式调用 - 基础示例...');
        await sdk.flow()
            .humanize()                     // 开启拟人化
            .window({
                // title: notepad.title,
                className: notepad.className,
                processName: notepad.processName,
            })
            .find('//Document')             // 查找编辑区域
            .click()                        // 点击
            .type('SDK 流式调用测试')        // 打字
            .run();                         // 执行
        console.log('   完成！');
        console.log();

        // 4. 流式调用 - 性能监控
        console.log('4. 性能监控示例...');
        const chain = sdk.flow()
            .profile()                      // 开启性能监控
            .window({
                // title: notepad.title,
                className: notepad.className,
                processName: notepad.processName,
            })
            .find('//Document')
            .click()
            .type('\n第二行内容')
            .wait(500);
        
        const stats = await chain.run() as ProfileStats;
        console.log(`   总耗时: ${stats.totalTime}ms`);
        console.log(`   步骤: ${stats.steps.map((s: {step: string, time: number}) => `${s.step}(${s.time}ms)`).join(' → ')}`);
        console.log();

        // 5. 元素信息查询
        console.log('5. 元素信息查询...');
        const info = await sdk.flow()
            .window({
                // title: notepad.title,
                className: notepad.className,
                processName: notepad.processName,
            })
            .find('//Document')
            .inspect();
        
        if (info) {
            console.log(`   名称: ${info.name}`);
            console.log(`   类型: ${info.controlType}`);
            console.log(`   位置: (${info.rect.x}, ${info.rect.y})`);
            console.log(`   大小: ${info.rect.width}x${info.rect.height}`);
        }
        console.log();

        // 6. 等待元素出现
        console.log('6. 等待元素示例...');
        const elem = await sdk.flow()
            .window({
                // title: notepad.title,
                className: notepad.className,
                processName: notepad.processName,
            })
            .waitFor('//Document', { timeout: 5000 });
        console.log(`   等待完成，元素: ${elem.name}`);
        console.log();

        // 7. 快捷键示例
        console.log('7. 快捷键示例...');
        await sdk.flow()
            .window({
                // title: notepad.title,
                className: notepad.className,
                processName: notepad.processName,
            })
            .find('//Document')
            .click()
            .shortcut('Ctrl+A')            // 全选
            .wait(200)
            .shortcut('Ctrl+C')            // 复制
            .run();
        console.log('   快捷键执行完成！');
        console.log();

        // 8. 断言示例
        console.log('8. 断言示例...');
        await sdk.flow()
            .window({
                // title: notepad.title,
                className: notepad.className,
                processName: notepad.processName,
            })
            .assertExists('//Document');    // 断言元素存在
        
        await sdk.flow()
            .window({
                // title: notepad.title,
                className: notepad.className,
                processName: notepad.processName,
            })
            .assertEnabled('//Document');   // 断言元素可用
        console.log('   断言通过！');
        console.log();

        // 9. 数据提取示例
        console.log('9. 数据提取示例...');
        const items = await sdk.flow()
            .window({
                // title: notepad.title,
                className: notepad.className,
                processName: notepad.processName,
            })
            .findAll('//Document');
        console.log(`   找到 ${items.length} 个元素`);
        console.log();

        console.log('=== 所有示例完成 ===');

    } catch (error) {
        console.error('错误:', error);
    }
}

main().catch(console.error);