/**
 * 测试日志级别优化
 * 
 * INFO: 给用户看的，简洁易懂的操作进度
 * DEBUG: 给开发者调试用的，包含技术细节
 */

import { SDK, ElementNotFoundError } from '../src';

async function testLoggingLevels() {
    console.log('=== 测试日志级别优化 ===\n');
    
    const sdk = new SDK({ baseUrl: 'http://localhost:8080' });
    
    // 检查服务状态
    console.log('1. 健康检查...\n');
    const health = await sdk.health();
    console.log(`   服务状态: ${health.status}\n`);
    
    try {
        // 执行自动化流程
        console.log('2. 开始执行自动化流程...\n');
        
        await sdk.flow()
            .humanize()
            .window({ title: '元宝', className: 'Tauri Window', processName: 'yuanbao' })
            .find('//Button[@Name="新建对话"]')
            .wait(500)
            .click()
            .wait(3000)
            .find('//Edit[@AutomationId="input-editor"]')
            .click()
            .type('测试日志输出')
            .run();

        console.log('\n✅ 所有操作成功完成\n');
    } catch (error) {
        if (error instanceof ElementNotFoundError) {
            console.error('\n❌ 元素未找到');
            console.error(`   XPath: ${error.context?.xpath}`);
            console.error(`   窗口: ${error.context?.windowSelector}`);
            console.error(`   截图: ${error.context?.screenshotPath}`);
            console.error(`   提示: ${error.context?.hint}`);
        } else if (error instanceof Error) {
            console.error('\n❌ 发生错误:', error.message);
            if (process.env.LOG_LEVEL === 'debug') {
                console.error(error.stack);
            }
        }
        process.exit(1);
    }
}

testLoggingLevels();
