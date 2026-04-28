// sdk/nodejs/examples/idle-motion-demo.ts
// 空闲移动示例 - 在元素区域内随机移动鼠标

import { ElementSelectorSDK } from '../src';

async function main() {
    const sdk = new ElementSelectorSDK();

    try {
        // 1. 获取窗口列表，找到记事本
        console.log('=== 获取窗口列表 ===');
        const windows = await sdk.listWindows();
        const notepadWindow = windows.find(w => w.processName === 'Notepad');
        
        if (!notepadWindow) {
            console.log('请先打开一个记事本窗口！');
            return;
        }
        
        // 构建精确的窗口选择器
        const windowSelector = {
            title: notepadWindow.title,
            className: notepadWindow.className,
            processName: notepadWindow.processName,
        };
        console.log('记事本窗口:', windowSelector);
        console.log();

        // 2. 检查当前状态
        console.log('=== 检查空闲移动状态 ===');
        const status = await sdk.getIdleMotionStatus();
        console.log('当前状态:', status);
        console.log();

        // 3. 启动空闲移动（使用精确选择器）
        console.log('=== 启动空闲移动 ===');
        console.log('将在记事本窗口的编辑区域内随机移动鼠标...');
        console.log('提示：移动鼠标或按键会自动暂停，静止 3 秒后恢复');
        console.log();
        
        await sdk.startIdleMotion({
            window: windowSelector,  // 使用精确选择器
            xpath: '//Document',
            
            speed: 'normal',
            moveInterval: 800,
            idleTimeout: 30000,
            
            humanIntervention: {
                enabled: true,
                pauseOnMouse: true,
                pauseOnKeyboard: true,
                resumeDelay: 3000
            }
        });
        
        console.log('空闲移动已启动！');
        console.log();

        // 4. 监控状态变化
        console.log('=== 监控状态（10秒）===');
        for (let i = 0; i < 10; i++) {
            await new Promise(r => setTimeout(r, 1000));
            const currentStatus = await sdk.getIdleMotionStatus();
            console.log(`[${i + 1}s] 活跃: ${currentStatus.active}, 暂停: ${currentStatus.paused}`);
            
            if (currentStatus.pauseReason) {
                console.log(`    暂停原因: ${currentStatus.pauseReason}`);
            }
        }
        console.log();

        // 5. 执行操作（使用 safeClick）
        console.log('=== 执行操作（自动暂停空闲移动）===');
        // 使用 safeClick 确保操作成功
        await sdk.safeClick({
            window: windowSelector,
            xpath: '//Document',
            options: { humanize: true }
        });
        console.log('点击完成，空闲移动已恢复');
        console.log();

        // 6. 获取最终状态
        const finalStatus = await sdk.getIdleMotionStatus();
        console.log('最终状态:', {
            active: finalStatus.active,
            paused: finalStatus.paused,
            duration: finalStatus.runningDurationMs
        });
        console.log();

        console.log('=== 停止空闲移动 ===');
        const stopResult = await sdk.stopIdleMotion();
        console.log(`已停止，运行时长: ${stopResult.durationMs}ms`);
        console.log();

    } catch (error) {
        console.error('错误:', error);
        
        try {
            await sdk.stopIdleMotion();
        } catch {}
    }
}

main().catch(console.error);