// sdk/nodejs/examples/test-api.ts
// 端到端测试 - 获取记事本元素

import { SDK } from '../src';

async function main() {
    const sdk = new SDK();

    console.log('=== 端到端测试 ===');
    console.log();

    // 1. 获取窗口列表
    console.log('1. 获取窗口列表...');
    const windows = await sdk.listWindows();
    console.log(`找到 ${windows.length} 个窗口`);
    
    // 找记事本
    const notepad = windows.find(w => w.processName === 'Notepad');
    if (!notepad) {
        console.log('请先打开记事本！');
        return;
    }
    console.log(`记事本窗口: ${notepad.title}`);
    console.log(`className: ${notepad.className}`);
    console.log(`processName: ${notepad.processName}`);
    console.log();

    // 2. 构建窗口选择器
    const winSelector = `Window[@Name='${notepad.title}' and @ClassName='${notepad.className}' and @ProcessName='${notepad.processName}']`;
    console.log(`窗口选择器: ${winSelector}`);
    console.log();

    // 3. 查找所有 Edit 元素
    console.log('2. 查找 Edit 元素...');
    const edits = await sdk.flow().window(winSelector).findAll('//Edit');
    console.log(`找到 ${edits.length} 个 Edit 元素`);
    edits.forEach(e => {
        console.log(`  - ${e.name} (${e.controlType})`);
        console.log(`    rect: (${e.rect.x}, ${e.rect.y}) ${e.rect.width}x${e.rect.height}`);
    });
    console.log();

    // 4. 查找 Document 元素
    console.log('3. 查找 Document 元素...');
    const docs = await sdk.flow().window(winSelector).findAll('//Document');
    console.log(`找到 ${docs.length} 个 Document 元素`);
    docs.forEach(e => {
        console.log(`  - ${e.name} (${e.controlType})`);
        console.log(`    rect: (${e.rect.x}, ${e.rect.y}) ${e.rect.width}x${e.rect.height}`);
    });
    console.log();

    // 5. 查找所有 Button
    console.log('4. 查找 Button 元素...');
    const buttons = await sdk.flow().window(winSelector).findAll('//Button');
    console.log(`找到 ${buttons.length} 个 Button 元素`);
    buttons.forEach(e => {
        console.log(`  - ${e.name}`);
    });
    console.log();

    // 6. 查找所有 MenuItem
    console.log('5. 查找 MenuItem 元素...');
    const menus = await sdk.flow().window(winSelector).findAll('//MenuItem');
    console.log(`找到 ${menus.length} 个 MenuItem 元素`);
    menus.slice(0, 10).forEach(e => {
        console.log(`  - ${e.name}`);
    });
    console.log();

    // 7. 获取单个元素
    console.log('6. 获取单个 Document 元素...');
    const chain = sdk.flow().window(winSelector).find('//Document');
    const info = await chain.inspect();
    if (info) {
        console.log('元素信息:');
        console.log(`  name: ${info.name}`);
        console.log(`  controlType: ${info.controlType}`);
        console.log(`  isEnabled: ${info.isEnabled}`);
        console.log(`  rect: ${JSON.stringify(info.rect)}`);
        console.log(`  center: ${JSON.stringify(info.center)}`);
    }
    console.log();

    console.log('=== 测试完成 ===');
}

main().catch(console.error);