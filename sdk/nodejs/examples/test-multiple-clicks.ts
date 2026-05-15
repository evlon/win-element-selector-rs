/**
 * 测试多个 click 是否都能正确执行
 */

import { SDK, ElementNotFoundError } from '../src';

async function testMultipleClicks() {
    console.log('=== 测试多个点击操作 ===\n');
    
    const sdk = new SDK({ baseUrl: 'http://localhost:8080' });
    
    // 检查服务状态
    console.log('1. 健康检查...\n');
    const health = await sdk.health();
    console.log(`   服务状态: ${health.status}\n`);
    
    try {
        console.log('2. 开始执行多次点击测试...\n');
        
        await sdk.flow()
            .humanize()
            .window({ title: '元宝', className: 'Tauri Window', processName: 'yuanbao' })
            // 第一次查找和点击
            .find('//Button[@Name="新建对话"]')
            .wait(500)
            .click()
            .wait(2000)
            // 第二次查找和点击（不同的元素）
            .find('//Edit[@AutomationId="input-editor"]')
            .click()
            .wait(1000)
            // 输入测试文本
            .type('测试多次点击')
            .run();

        console.log('\n✅ 所有操作成功完成\n');
        console.log('验证点：');
        console.log('  ✓ 第一次点击应该点击了"新建对话"按钮');
        console.log('  ✓ 第二次点击应该点击了输入框');
        console.log('  ✓ 输入框中应该有"测试多次点击"文本');
    } catch (error) {
        if (error instanceof ElementNotFoundError) {
            console.error('\n❌ 元素未找到');
            console.error(`   XPath: ${error.context?.xpath}`);
            console.error(`   窗口: ${error.context?.windowSelector}`);
            console.error(`   截图: ${error.context?.screenshotPath}`);
        } else if (error instanceof Error) {
            console.error('\n❌ 发生错误:', error.message);
            if (process.env.LOG_LEVEL === 'debug') {
                console.error(error.stack);
            }
        }
        process.exit(1);
    }
}

testMultipleClicks();
