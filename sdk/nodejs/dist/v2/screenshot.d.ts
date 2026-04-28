export declare class ScreenshotManager {
    private screenshotDir;
    constructor(baseDir?: string);
    /**
     * 通用截图方法
     * @param outputPath 输出路径
     */
    capture(outputPath: string): Promise<string>;
    /**
     * 捕获失败截图
     * @param step 失败的步骤名称
     * @returns 截图文件路径
     */
    captureFailure(step: string): Promise<string>;
    /**
     * 捕获全屏截图
     */
    captureScreen(name: string): Promise<string>;
    /**
     * 捕获窗口截图
     */
    captureWindow(windowTitle: string): Promise<string>;
    /**
     * 自动命名截图
     */
    captureAuto(): Promise<string>;
    /**
     * 生成文件名（带时间戳）
     */
    private generateFilename;
    /**
     * 确保目录存在
     */
    private ensureDirectoryExists;
    /**
     * 创建占位文件（待实现真实截图）
     */
    private createPlaceholder;
}
export declare const globalScreenshotManager: ScreenshotManager;
//# sourceMappingURL=screenshot.d.ts.map