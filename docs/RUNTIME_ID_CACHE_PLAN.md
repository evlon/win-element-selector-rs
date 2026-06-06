# RuntimeId 缓存优化方案 v2

> 日期：2026-06-06
> 范围：SDK (TypeScript element.ts) + API (Rust element.rs) + Core (element_cache.rs, find.rs) 全链路改造

---

## 一、设计原则（用户明确要求）

1. **无隐式 fallback** — 缓存命不中就直接报错返回，不让系统偷偷切到 XPath 搜索。用户自己决定怎么处理。
2. **可配置缓存策略** — 全局 TTL + 每个 find 函数可指定自己的缓存时间。
3. **SDK + API + Core 一起规划** — 三端并行改造，不分开做。

---

## 二、现状分析

### 2.1 已有基础设施

| 组件 | 位置 | 现状 |
|------|------|------|
| **Element Cache** | `core/element_cache.rs` | LRU 缓存，key=RuntimeId string，value=UIElement，最大 512 条，但**无 TTL** ✅ |
| **RuntimeId 提取** | `core/uia/helpers.rs` | `runtime_id_key(elem) -> Option<Vec<i32>>` ✅ |
| **find_from_element_cached** | `core/uia/find.rs:1357` | 接收 runtimeId，从缓存取父元素后做 XPath 搜索 ✅ |
| **locate_first_from / locate_one_from / locate_all_from** | `core/uia/find.rs:1457-1497` | 二次定位 API ✅ |
| **POST /api/element/find-from** | `api/element.rs:636` | HTTP 端点，接收 `{runtimeId, xpath}` ✅ |
| **ElementInfo.runtimeId** | SDK types.ts & API types.rs | 字段已有 ✅ |
| **findFromElement()** | SDK client.ts | HTTP 调用方法已有 ✅ |

### 2.2 当前瓶颈

**核心问题：SDK Element 的每个操作都走 XPath 全窗口搜索，已知 runtimeId 但完全不用！**

```
用户操作调用链（现状）:
  el.click()
    → SDK resolveXpath()  [JS, 本地]
    → POST /api/click { element: "/Window[1]/Pane[2]/Button[5]" }
    → 后端 find_all_elements_detailed()  [Rust, 全窗口 XPath 搜索, 50-200ms]
    → 找到元素 → 点击
```

每次 `click/type/hover/refresh/visibility` 都在重做全窗口 XPath 搜索，而 `this.info.runtimeId` 从未使用。

### 2.3 浪费场景

| 操作 | 当前方式 | 浪费程度 |
|------|----------|----------|
| `el.click()` | 全窗口 XPath 搜索 | 🔴 高 |
| `el.refresh()` | 全窗口 XPath 搜索 | 🔴 高 |
| `el.hover()` | 全窗口 XPath 搜索 | 🟡 中 |
| `el.type()` | 全窗口 XPath 搜索 | 🔴 高 |
| `el.flash()` | 全窗口 XPath 搜索 | 🟡 中 |
| `el.checkVisibility()` | 全窗口 XPath 搜索 | 🟡 中 |
| `el.waitUntilGone()` / `el.waitFor()` | 循环内每次全窗口搜索 | 🔴 极高 |
| `el.find("//Button")` | 拼接 XPath → 全窗口搜索 | 🔴 高 |
| `el.children()` / `el.parent()` / etc. | 全窗口 XPath 搜索 | 🟡 中 |

---

## 三、核心设计

### 3.1 缓存架构增强

```
ElementCache (现有)
  ├─ elements: HashMap<RuntimeId, UIElement>
  ├─ insertion_order: VecDeque<String>  (LRU)
  └─ 最大 512 条

ElementCache (增强后)
  ├─ elements: HashMap<RuntimeId, CachedElement>
  │     ├─ element: UIElement
  │     └─ cached_at: Instant  (缓存时间戳)
  ├─ insertion_order: VecDeque<String>  (LRU)
  ├─ max_size: usize = 512
  └─ default_ttl: Option<Duration>  (全局默认 TTL，None = 永不过期)
```

### 3.2 操作链路

```
el.click() (优化后):
  → POST /api/click { 
      element: "/Window[1]/Pane[2]/Button[5]",  // XPath 保留（用于生成精确XPath）
      runtimeId: "42,1234567890,1"               // 缓存优先
    }
  → 后端: 只通过 runtimeId 查缓存获取 UIElement
    ├─ 命中 + 未过期 → 直接操作（~1ms）
    └─ 未命中 / 过期 → 返回错误（无 fallback！）
```

### 3.3 缓存 TTL 配置层级

```
全局默认 TTL     (SDKConfig.cacheTTL, 默认 None = 永不过期)
  ├─ findOne() TTL    (options.cacheTTL, 覆盖全局)
  ├─ findFirst() TTL  (options.cacheTTL, 覆盖全局)
  ├─ findAll() TTL    (options.cacheTTL, 覆盖全局)
  ├─ children() TTL   (options.cacheTTL, 覆盖全局)
  └─ ...其他 find 函数
```

---

## 四、SDK 端改造

### 4.1 `types.ts` — 新增类型

```typescript
// ═══════════════════════════════════════════════════════
// 缓存配置
// ═══════════════════════════════════════════════════════

/** 缓存 TTL 配置。null = 永不过期，number = 毫秒 */
export type CacheTTL = number | null;

export interface SDKConfig {
    baseUrl: string;
    timeout?: number;
    autoWait?: AutoWaitConfig;
    logging?: LoggingConfig;
    idleMotion?: IdleOptions;
    scroll?: ScrollConfig;
    speedFactor?: number;

    /** 全局元素缓存 TTL（毫秒），默认 null = 永不过期 */
    cacheTTL?: CacheTTL;
}

// ═══════════════════════════════════════════════════════
// 所有操作请求体新增 runtimeId
// ═══════════════════════════════════════════════════════

export interface ClickParams {
    window: WindowSelector | string;
    element: string;
    runtimeId?: string;    // ← 新增
    options?: ClickOptions;
}

export interface ElementQueryParams {
    window: string;
    element: string;
    runtimeId?: string;    // ← 新增
    randomRange?: number;
}

export interface ElementVisibilityRequest {
    window: string;
    element: string;
    runtimeId?: string;    // ← 新增
    container?: string;
}

export interface ElementFlashRequest {
    window: string;
    element: string;
    runtimeId?: string;    // ← 新增
    timeout: number;
}

export interface InspectRequest {
    window: string;
    element: string;
    runtimeId?: string;    // ← 新增
    format?: 'json' | 'txt';
}

// Hover 参数
export interface HoverMouseParams {
    window: string;
    element: string;
    runtimeId?: string;    // ← 新增
    duration?: number;
    humanize?: boolean;
}

// Type 参数（已有 window/element 可选字段，加 runtimeId）
// Navigate 请求
export interface NavigateRequest {
    window: string;
    element: string;
    runtimeId?: string;    // ← 新增
    steps: NavigateStep[];
}

// ═══════════════════════════════════════════════════════
// find 系列函数的选项新增 cacheTTL
// ═══════════════════════════════════════════════════════

export interface FindOptions {
    /** 覆盖全局缓存 TTL（毫秒），null = 永不过期 */
    cacheTTL?: CacheTTL;
    /** 用于唯一标识当前元素的属性名列表 */
    propNames?: string[];
}
```

### 4.2 `element.ts` — Element 类改动

```typescript
export class Element {
    readonly windowSelector: string;
    readonly findSelector: string;
    readonly info: ElementInfo;
    readonly foundElementCount: number;
    readonly selector: string;

    private client: HttpClient;
    private autoWaitConfig: AutoWaitConfig;
    private logger: OperationLogger;
    private cacheTTL: CacheTTL;  // ← 新增：此元素的缓存 TTL

    constructor(
        client: HttpClient,
        xpathStr: string,
        windowSelector: string,
        findSelector: string,
        info: ElementInfo,
        autoWaitConfig: AutoWaitConfig,
        logger: OperationLogger,
        foundElementCount: number = 1,
        cacheTTL?: CacheTTL,  // ← 新增参数
    ) {
        // ... 现有逻辑 ...
        this.cacheTTL = cacheTTL ?? null;
    }

    // ═══════════════════════════════════════════════════
    // 私有辅助
    // ═══════════════════════════════════════════════════

    private get runtimeId(): string {
        return this.info.runtimeId || '';
    }

    // ═══════════════════════════════════════════════════
    // P0: 核心动作 — 全部走 runtimeId 缓存
    // ═══════════════════════════════════════════════════

    async click(options?: ClickOptions, ...propNames: string[]): Promise<void> {
        this.logger.logOperation('点击元素', this.info);

        const waitBefore = options?.waitBefore ?? DEFAULTS.click.waitBefore;
        if (waitBefore && waitBefore > 0) await delay(waitBefore);

        const useXpath = this.resolveXpath(propNames);

        const result = await this.client.clickMouse({
            window: this.windowSelector,
            element: useXpath,
            runtimeId: this.runtimeId,  // ← 传入 runtimeId
            options: { /* ... 同现状 ... */ },
        });

        if (!result.success) {
            this.logger.logError('点击元素', new ActionFailedError('click', 'Click failed', undefined));
            throw new ActionFailedError('click', 'Click failed', undefined);
        }

        this.logger.logSuccess('点击元素', { clickPoint: result.clickPoint, elementInfo: this.info });

        const waitAfter = options?.waitAfter ?? DEFAULTS.click.waitAfter;
        if (waitAfter && waitAfter > 0) {
            await delay(waitAfter);
        } else if (this.autoWaitConfig.enabled) {
            await this.maybeAutoWait('afterClick');
        }
    }

    async hover(options?: { duration?: number; humanize?: boolean }, ...propNames: string[]): Promise<void> {
        this.logger.logOperation('悬停在元素上', this.info);
        const useXpath = this.resolveXpath(propNames);

        const result = await this.client.hoverMouse({
            window: this.windowSelector,
            element: useXpath,
            runtimeId: this.runtimeId,  // ← 新增
            duration: options?.duration ?? 500,
            humanize: options?.humanize ?? true,
        });

        if (!result.success) {
            throw new ActionFailedError('hover', result.error ?? 'Hover failed', undefined);
        }
        this.logger.logSuccess('悬停在元素上');
        await this.maybeAutoWait('afterFind');
    }

    // type() — 同样传入 runtimeId
    // rightClick() — 同样传入 runtimeId

    // ═══════════════════════════════════════════════════
    // P0: refresh — 通过 runtimeId 从后端获取最新信息
    // ═══════════════════════════════════════════════════

    /**
     * 刷新元素最新状态（原地更新 this.info）。
     *
     * **无参数时**：通过 runtimeId 从缓存获取最新属性（~1ms，无 fallback）。
     *   - 缓存未命中/过期 → 抛出 ElementNotFoundError
     *
     * **有参数时**（propNames）：构造精确 XPath 通过 find API 重新搜索（全窗口搜索）。
     *
     * @example
     * await el.refresh();                        // runtimeId 缓存刷新
     * await el.refresh('name', 'automationId');  // XPath 重新搜索
     */
    async refresh(...propNames: string[]): Promise<void> {
        // 无参数 + 有 runtimeId → 缓存刷新
        if (propNames.length === 0 && this.runtimeId) {
            const response = await this.client.refreshByRuntimeId(
                this.windowSelector,
                this.runtimeId
            );
            if (response.found && response.element) {
                delete (response.element as any).elementSelector;
                Object.assign(this.info, response.element);
                return;
            }
            // 缓存未命中，抛出错误（无 fallback）
            throw new ElementNotFoundError(
                `runtimeId=${this.runtimeId}`,
                this.windowSelector,
                '缓存中的元素已失效，请重新查找'
            );
        }

        // 有参数 或 无 runtimeId → XPath 搜索
        const useXpath = this.resolveXpath(propNames);
        const response = await this.client.find({
            window: this.windowSelector,
            element: useXpath,
        });
        if (!response.found || !response.element) {
            throw new ElementNotFoundError(useXpath, this.windowSelector);
        }
        delete (response.element as any).elementSelector;
        Object.assign(this.info, response.element);
    }

    /**
     * 通过 XPath 重新查找来刷新元素（基于 findFirst/findOne）。
     *
     * 适用场景：runtimeId 缓存已过期，但你知道当前元素在 DOM 中的位置没变，
     * 通过 XPath 重新找到元素，同时更新 this.info。
     *
     * @param findFn - 查找函数，返回重新找到的 Element
     *
     * @example
     * await el.refreshByXpath(() => el.find('//Button'));
     * await el.refreshByXpath(() => el.findOne('//Button[@Name="确定"]'));
     */
    async refreshByXpath(findFn: () => Promise<Element>): Promise<void> {
        const newEl = await findFn();
        delete (newEl.info as any).elementSelector;
        Object.assign(this.info, newEl.info);
    }

    // ═══════════════════════════════════════════════════
    // P1: 查询/断言 — 走 runtimeId 缓存
    // ═══════════════════════════════════════════════════

    async flash(options?: FlashOptions, ...propNames: string[]): Promise<void> {
        this.logger.logOperation('高亮闪烁元素', this.info);
        const useXpath = this.resolveXpath(propNames);

        const result = await this.client.flashElement(
            this.windowSelector,
            useXpath,
            options?.timeout ?? 1000,
            this.runtimeId,  // ← 新增
        );

        if (!result.success) {
            this.logger.logError('高亮闪烁元素', new ActionFailedError('flash', result.error ?? 'Flash failed', undefined));
            throw new ActionFailedError('flash', result.error ?? 'Flash failed', undefined);
        }
        this.logger.logSuccess('高亮闪烁元素');
    }

    async checkVisibility(containerXPath?: string, ...propNames: string[]): Promise<ElementVisibilityResult> {
        const useXpath = this.resolveXpath(propNames);
        return this.client.getElementVisibility(
            this.windowSelector, useXpath, containerXPath, this.runtimeId
        );
    }

    // inspect() — 传入 runtimeId

    async assertExists(...propNames: string[]): Promise<void> {
        // 有 runtimeId 时直接验证缓存是否有效
        if (this.runtimeId) {
            const response = await this.client.refreshByRuntimeId(
                this.windowSelector, this.runtimeId
            );
            if (response.found) return;
            throw new ElementNotFoundError(
                `runtimeId=${this.runtimeId}`,
                this.windowSelector,
                '元素已不存在'
            );
        }
        // 无 runtimeId 走 XPath
        const useXpath = this.resolveXpath(propNames);
        const response = await this.client.find({
            window: this.windowSelector,
            element: useXpath,
        });
        if (!response.found || !response.element) {
            throw new ElementNotFoundError(useXpath, this.windowSelector);
        }
    }

    // ═══════════════════════════════════════════════════
    // P2: 子元素查找 — 全部改用 findFromElement
    // ═══════════════════════════════════════════════════

    /** findOne/findFirst 的公共实现（P2 优化版） */
    private async findElement(
        xpath: string,
        propNames: string[],
        expectSingle: boolean,
        options?: FindOptions,
    ): Promise<Element> {
        if (!this.runtimeId) {
            // 无 runtimeId：回退到 XPath 拼接（初始场景）
            return this.findElementByXPath(xpath, propNames, expectSingle);
        }

        // 有 runtimeId：使用 findFromElement API
        const relativeXpath = this.buildRelativeXpath(xpath, propNames);
        const response = await this.client.findFromElement({
            runtimeId: this.runtimeId,
            xpath: relativeXpath,
            searchStrategy: 'Fast',
        });

        if (!response.found || response.elements.length === 0) {
            throw new ElementNotFoundError(relativeXpath, this.windowSelector);
        }

        if (expectSingle && response.total > 1) {
            throw new Error(`findOne 匹配到 ${response.total} 个元素，期望恰好 1 个`);
        }

        const el = response.elements[0];
        const findSelector = this.buildChildXpath(xpath);
        return new Element(
            this.client,
            findSelector,
            this.windowSelector,
            findSelector,
            el,
            this.autoWaitConfig,
            this.logger,
            response.total,
            options?.cacheTTL ?? this.cacheTTL,  // ← 传递缓存 TTL
        );
    }

    // findOne/findFirst/find 都调用 findElement，新增 options 参数
    async findOne(xpath: string, options?: FindOptions): Promise<Element> {
        return this.findElement(xpath, options?.propNames ?? [], true, options);
    }

    async findFirst(xpath: string, options?: FindOptions): Promise<Element> {
        return this.findElement(xpath, options?.propNames ?? [], false, options);
    }

    async find(xpath: string, options?: FindOptions): Promise<Element> {
        return this.findFirst(xpath, options);
    }

    // findAll 也改用 findFromElement
    async findAll(xpath: string, options?: FindOptions): Promise<ElementList> {
        if (!this.runtimeId) {
            return this.findAllByXPath(xpath, options?.propNames ?? []);
        }

        const relativeXpath = this.buildRelativeXpath(xpath, options?.propNames ?? []);
        const response = await this.client.findFromElement({
            runtimeId: this.runtimeId,
            xpath: relativeXpath,
            searchStrategy: 'Fast',
        });

        if (!response.found || response.elements.length === 0) {
            return this.emptyElementList(relativeXpath);
        }

        const fullXPath = this.resolveRelativeXpath(xpath, options?.propNames ?? []);
        const totalCount = response.total;

        const elements: Element[] = response.elements.map((item) => {
            return new Element(
                this.client, fullXPath, this.windowSelector,
                item.findSelector || fullXPath,
                item, this.autoWaitConfig, this.logger,
                totalCount, options?.cacheTTL ?? this.cacheTTL,
            );
        });

        // position() 也通过 runtimeId 定位
        const positionFn = async (n: number): Promise<Element> => {
            const nthXpath = `(${fullXPath})[position()=${n}]`;
            const resp = await this.client.findFromElement({
                runtimeId: this.runtimeId!,
                xpath: `(${this.buildRelativeXpath(xpath, [])})[position()=${n}]`,
                searchStrategy: 'Fast',
            });
            if (!resp.found || resp.elements.length === 0) {
                throw new ElementNotFoundError(nthXpath, this.windowSelector);
            }
            return new Element(
                this.client, nthXpath, this.windowSelector,
                nthXpath, resp.elements[0],
                this.autoWaitConfig, this.logger, 1,
                options?.cacheTTL ?? this.cacheTTL,
            );
        };

        return Object.assign(elements, { position: positionFn }) as ElementList;
    }

    // children() — 同样通过 runtimeId + findFromElement
    // parent() / next() / prev() — 可通过 navigate_from_element 或 compass

    // ═══════════════════════════════════════════════════
    // P3: 等待/循环 — 复用 runtimeId 缓存
    // ═══════════════════════════════════════════════════

    async waitUntilGone(options?: WaitOptions, ...propNames: string[]): Promise<void> {
        const timeout = options?.timeout ?? 10000;
        const interval = options?.interval ?? 500;
        const startTime = Date.now();

        while (Date.now() - startTime < timeout) {
            // 有 runtimeId 时通过缓存验证（极快，~1ms）
            if (this.runtimeId) {
                const resp = await this.client.refreshByRuntimeId(
                    this.windowSelector, this.runtimeId
                );
                if (!resp.found) return; // 缓存未命中 = 元素消失
            } else {
                const useXpath = this.resolveXpath(propNames);
                const response = await this.client.find({
                    window: this.windowSelector,
                    element: useXpath,
                });
                if (!response.found) return;
            }
            await delay(interval);
        }

        throw new Error(`Element did not disappear within ${timeout}ms`);
    }

    async waitFor(options?: WaitOptions, ...propNames: string[]): Promise<Element> {
        const timeout = options?.timeout ?? 10000;
        const interval = options?.interval ?? 500;
        const startTime = Date.now();

        while (Date.now() - startTime < timeout) {
            if (this.runtimeId) {
                const resp = await this.client.refreshByRuntimeId(
                    this.windowSelector, this.runtimeId
                );
                if (resp.found && resp.element) {
                    delete (resp.element as any).elementSelector;
                    Object.assign(this.info, resp.element);
                    return this;
                }
            } else {
                try {
                    const useXpath = this.resolveXpath(propNames);
                    const response = await this.client.find({
                        window: this.windowSelector,
                        element: useXpath,
                    });
                    if (response.found && response.element) {
                        return new Element(
                            this.client, useXpath, this.windowSelector,
                            response.findSelector || useXpath,
                            response.element!, this.autoWaitConfig,
                            this.logger, response.total ?? 1,
                        );
                    }
                } catch { /* keep polling */ }
            }
            await delay(interval);
        }

        throw new Error(`Element did not appear within ${timeout}ms`);
    }

    // ═══════════════════════════════════════════════════
    // P4: 定位导航 — compass/navigate 支持 runtimeId
    // ═══════════════════════════════════════════════════

    async compass(path: string, ...propNames: string[]): Promise<Element> {
        const tokens = this.parseCompassPath(path);
        const baseXpath = this.resolveXpath(propNames);
        const steps = tokens.map(/* ... 同现状 ... */);

        const response = await this.client.navigateElement(
            this.windowSelector, baseXpath, steps, this.runtimeId  // ← 新增
        );

        if (!response.found || !response.element) {
            throw new ElementNotFoundError(path, this.windowSelector);
        }

        const findSelector = this.buildCompassXpath(baseXpath, tokens);
        return new Element(
            this.client, findSelector, this.windowSelector,
            findSelector, response.element,
            this.autoWaitConfig, this.logger, 1,
            this.cacheTTL,
        );
    }

    // ═══════════════════════════════════════════════════
    // 辅助：构造相对 XPath（去掉父元素前缀）
    // ═══════════════════════════════════════════════════

    private buildRelativeXpath(xpath: string, propNames: string[]): string {
        // 从完整的 resolveRelativeXpath 结果中提取相对部分
        const fullXPath = this.resolveRelativeXpath(xpath, propNames);
        // 去掉 findSelector 前缀
        const prefix = this.resolveXpath(propNames);
        if (fullXPath.startsWith(prefix + '//')) {
            return '//' + fullXPath.substring(prefix.length + 2);
        }
        if (fullXPath.startsWith(prefix + '/')) {
            return '/' + fullXPath.substring(prefix.length + 1);
        }
        return fullXPath;
    }

    private buildChildXpath(xpath: string): string {
        // 构造子元素的完整 XPath（用于 Element.findSelector）
        return this.resolveRelativeXpath(xpath, []);
    }
}
```

### 4.3 `client.ts` — HttpClient 改动

```typescript
export class HttpClient {
    // ... 现有代码 ...

    // ═══════════════════════════════════════════════════
    // 新增：通过 runtimeId 刷新元素信息
    // ═══════════════════════════════════════════════════

    async refreshByRuntimeId(
        windowSelector: string,
        runtimeId: string
    ): Promise<{ found: boolean; element: ElementInfo | null }> {
        return this.requestWithRetry(async () => {
            const response = await this.client.post<{
                found: boolean;
                element: ElementInfo | null;
            }>('/api/element/refresh', {
                window: windowSelector,
                runtimeId,
            });
            return response.data;
        }, {
            endpoint: '/api/element/refresh',
            operation: 'refreshByRuntimeId',
            window: windowSelector,
            extra: { runtimeId: runtimeId.substring(0, 16) },
        });
    }

    // ═══════════════════════════════════════════════════
    // 修改：所有现有方法增加 runtimeId 参数
    // ═══════════════════════════════════════════════════

    async clickMouse(params: ClickParams): Promise<ClickResult> {
        return this.requestWithRetry(async () => {
            const response = await this.client.post<ClickResult>('/api/mouse/click', {
                window: params.window,
                element: params.element,
                runtimeId: params.runtimeId,  // ← 新增
                options: { /* ... 同现状 ... */ },
            });
            return response.data;
        });
    }

    async hoverMouse(params: { window: string; element: string; runtimeId?: string; duration?: number; humanize?: boolean }): Promise<{ success: boolean; hoverPoint: Point | null; error: string | null }> {
        return this.requestWithRetry(async () => {
            const response = await this.client.post('/api/mouse/hover', {
                window: params.window,
                element: params.element,
                runtimeId: params.runtimeId,  // ← 新增
                options: {
                    humanize: params.humanize ?? DEFAULTS.move.humanize,
                    duration: params.duration ?? 500,
                },
            });
            return response.data;
        });
    }

    async flashElement(
        windowSelector: string,
        elementXPath: string,
        timeout?: number,
        runtimeId?: string,  // ← 新增
    ): Promise<FlashResult> {
        return this.requestWithRetry(async () => {
            const response = await this.client.post<FlashResult>('/api/element/flash', {
                window: windowSelector,
                element: elementXPath,
                runtimeId,  // ← 新增
                timeout: timeout ?? 1000,
            });
            return response.data;
        });
    }

    async getElementVisibility(
        windowSelector: string,
        elementXPath: string,
        containerXPath?: string,
        runtimeId?: string,  // ← 新增
    ): Promise<ElementVisibilityResult> {
        return this.requestWithRetry(async () => {
            const body: any = {
                window: windowSelector,
                element: elementXPath,
                runtimeId,  // ← 新增
            };
            if (containerXPath) body.container = containerXPath;
            const response = await this.client.post<ElementVisibilityResult>('/api/element/visibility', body);
            return response.data;
        });
    }

    async inspectElement(
        windowSelector: string,
        elementXPath: string,
        format?: 'json' | 'txt' | 'text',
        runtimeId?: string,  // ← 新增
    ): Promise<InspectResponse> {
        // ... 同现状，请求体加 runtimeId ...
    }

    async navigateElement(
        windowSelector: string,
        baseXPath: string,
        steps: NavigateStep[],
        runtimeId?: string,  // ← 新增
    ): Promise<NavigateResponse> {
        return this.requestWithRetry(async () => {
            const response = await this.client.post<NavigateResponse>('/api/element/navigate', {
                window: windowSelector,
                element: baseXPath,
                steps,
                runtimeId,  // ← 新增
            });
            return response.data;
        });
    }
}
```

---

## 五、API 端改造 (`api/types.rs` + `api/element.rs`)

### 5.1 `api/types.rs` — 所有请求体加 `runtime_id`

```rust
// ═══════════════════════════════════════════════════════
// 元素 API 请求体统一新增 runtime_id
// ═══════════════════════════════════════════════════════

pub struct ElementQuery {
    pub window: String,
    pub element: String,
    /// RuntimeId，用于缓存查找（优先于 XPath 搜索）
    #[serde(default, rename = "runtimeId", skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    // ... 其余字段不变 ...
}

pub struct ElementVisibilityRequest {
    pub window: String,
    pub element: String,
    /// RuntimeId 优先
    #[serde(default, rename = "runtimeId", skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    #[serde(default)]
    pub container: Option<String>,
}

pub struct ElementFlashRequest {
    pub window: String,
    pub element: String,
    #[serde(default, rename = "runtimeId", skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    #[serde(default = "default_flash_timeout")]
    pub timeout: u64,
}

pub struct InspectRequest {
    pub window: String,
    pub element: String,
    #[serde(default, rename = "runtimeId", skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    // ... 其余字段不变 ...
}

pub struct NavigateRequest {
    pub window: String,
    pub element: String,
    /// 基准元素的 runtimeId（优先于 element XPath）
    #[serde(default, rename = "runtimeId", skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    pub steps: Vec<NavigateStep>,
}

// ═══════════════════════════════════════════════════════
// 鼠标 API 请求体统一新增 runtime_id
// ═══════════════════════════════════════════════════════

pub struct MouseClickRequest {
    pub window: WindowSelectorOrString,
    pub element: String,
    #[serde(default, rename = "runtimeId", skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    pub options: Option<MouseClickOptions>,
}

pub struct MouseHoverRequest {
    pub window: WindowSelectorOrString,
    pub element: String,
    #[serde(default, rename = "runtimeId", skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    pub options: Option<MouseHoverOptions>,
}

pub struct MouseDragRequest {
    pub window: WindowSelectorOrString,
    #[serde(rename = "sourceElement")]
    pub source_element: String,
    #[serde(default, rename = "sourceRuntimeId", skip_serializing_if = "Option::is_none")]
    pub source_runtime_id: Option<String>,
    #[serde(rename = "targetElement")]
    pub target_element: String,
    #[serde(default, rename = "targetRuntimeId", skip_serializing_if = "Option::is_none")]
    pub target_runtime_id: Option<String>,
    pub options: Option<MouseDragOptions>,
}

// ═══════════════════════════════════════════════════════
// 新增：通过 RuntimeId 刷新元素
// ═══════════════════════════════════════════════════════

/// POST /api/element/refresh 请求
#[derive(Debug, Clone, Deserialize)]
pub struct RefreshByRuntimeIdRequest {
    pub window: String,
    #[serde(rename = "runtimeId")]
    pub runtime_id: String,
}

/// POST /api/element/refresh 响应
#[derive(Debug, Clone, Serialize)]
pub struct RefreshByRuntimeIdResponse {
    pub found: bool,
    pub element: Option<ElementInfo>,
    pub error: Option<String>,
}

// ═══════════════════════════════════════════════════════
// 新增：缓存控制 API
// ═══════════════════════════════════════════════════════

/// PUT /api/element/cache/config 请求
#[derive(Debug, Clone, Deserialize)]
pub struct CacheConfigRequest {
    /// 全局缓存 TTL（毫秒），null = 永不过期
    #[serde(default, rename = "cacheTTL")]
    pub cache_ttl_ms: Option<u64>,
}

/// GET /api/element/cache/stats 响应
#[derive(Debug, Clone, Serialize)]
pub struct CacheStatsResponse {
    pub size: usize,
    pub max_size: usize,
    pub default_ttl_ms: Option<u64>,
}
```

### 5.2 `api/element.rs` — 端点改造

```rust
// ═══════════════════════════════════════════════════════
// 通用辅助：通过 runtimeId 获取元素（无 fallback）
// ═══════════════════════════════════════════════════════

/// 从缓存获取元素。缓存未命中时返回错误（无 XPath fallback）。
fn resolve_element_by_runtime_id(
    runtime_id: &str,
) -> Result<UIElement, String> {
    crate::core::element_cache::get_cached_element(runtime_id)
        .ok_or_else(|| format!("元素不在缓存中: runtimeId={}", runtime_id))
}

// ═══════════════════════════════════════════════════════
// 新增：POST /api/element/refresh
// ═══════════════════════════════════════════════════════

pub async fn refresh_by_runtime_id(
    body: web::Json<RefreshByRuntimeIdRequest>,
) -> impl Responder {
    let runtime_id = body.runtime_id.clone();

    let result = tokio::task::spawn_blocking(move || {
        match crate::core::element_cache::get_cached_element(&runtime_id) {
            Some(elem) => {
                // 从缓存的 UIElement 读取最新属性
                let data = crate::core::uia::element_info_from_uia(
                    &elem, None, 0.0, &mut rand::thread_rng()
                );
                data.map(|d| (true, Some(d), None))
                    .unwrap_or((false, None, Some("无法读取元素属性".to_string())))
            }
            None => (false, None, Some("元素不在缓存中".to_string())),
        }
    }).await;

    match result {
        Ok((found, element_data, error)) => {
            let element_info = element_data.map(Into::into);
            HttpResponse::Ok().json(RefreshByRuntimeIdResponse {
                found,
                element: element_info,
                error,
            })
        }
        Err(e) => HttpResponse::InternalServerError().json(RefreshByRuntimeIdResponse {
            found: false,
            element: None,
            error: Some(format!("线程错误: {}", e)),
        }),
    }
}

// ═══════════════════════════════════════════════════════
// 修改：get_element — 优先 runtimeId 缓存
// ═══════════════════════════════════════════════════════

pub async fn get_element(/* ... */) -> impl Responder {
    // ...
    let runtime_id = element_query.runtime_id.clone();

    // 如果有 runtimeId，直接从缓存获取（无 fallback）
    if let Some(rid) = runtime_id {
        let result = tokio::task::spawn_blocking(move || {
            match crate::core::element_cache::get_cached_element(&rid) {
                Some(elem) => {
                    let data = crate::core::uia::element_info_from_uia(
                        &elem, None, random_range, &mut rand::thread_rng()
                    );
                    data.map(|d| vec![d])
                        .unwrap_or_default()
                }
                None => vec![],  // 缓存未命中 → 返回空（无 fallback）
            }
        }).await;

        // ... 返回结果（未命中返回 found=false）
    }

    // 无 runtimeId 时才走 XPath 搜索（原有逻辑）
    // ...
}

// 同理修改 get_all_elements, get_element_visibility, flash_element,
// inspect_element, navigate_element 等端点。

// ═══════════════════════════════════════════════════════
// 新增：缓存控制端点
// ═══════════════════════════════════════════════════════

/// PUT /api/element/cache/config
/// 设置全局缓存 TTL
pub async fn set_cache_config(
    body: web::Json<CacheConfigRequest>,
) -> impl Responder {
    crate::core::element_cache::set_default_ttl(
        body.cache_ttl_ms.map(std::time::Duration::from_millis)
    );
    HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
}

/// GET /api/element/cache/stats
/// 获取缓存统计
pub async fn get_cache_stats() -> impl Responder {
    let (size, max_size, ttl) = crate::core::element_cache::stats();
    HttpResponse::Ok().json(CacheStatsResponse {
        size,
        max_size,
        default_ttl_ms: ttl.map(|d| d.as_millis() as u64),
    })
}

/// POST /api/element/cache/clear
/// 清除所有缓存
pub async fn clear_element_cache() -> impl Responder {
    crate::core::element_cache::clear_cache();
    HttpResponse::Ok().json(serde_json::json!({ "cleared": true }))
}
```

---

## 六、Core 层改造 (`core/element_cache.rs`)

### 6.1 增强缓存结构

```rust
use std::time::{Duration, Instant};

/// 缓存条目（含时间戳）
struct CachedElement {
    element: UIElement,
    cached_at: Instant,
}

struct ElementCache {
    elements: HashMap<String, CachedElement>,
    insertion_order: VecDeque<String>,
    max_size: usize,
    /// 全局默认 TTL。None = 永不过期。
    default_ttl: Option<Duration>,
}

impl ElementCache {
    fn insert(&mut self, key: String, element: UIElement) {
        if self.elements.contains_key(&key) {
            return;
        }
        while self.elements.len() >= self.max_size {
            if let Some(oldest) = self.insertion_order.pop_front() {
                self.elements.remove(&oldest);
            } else { break; }
        }
        self.elements.insert(key.clone(), CachedElement {
            element,
            cached_at: Instant::now(),
        });
        self.insertion_order.push_back(key);
    }

    /// 获取缓存元素，自动检查 TTL 过期
    fn get(&mut self, key: &str) -> Option<UIElement> {
        if let Some(entry) = self.elements.get(key) {
            // 检查是否过期
            if let Some(ttl) = self.default_ttl {
                if entry.cached_at.elapsed() > ttl {
                    // 过期 → 移除并返回 None
                    self.elements.remove(key);
                    self.insertion_order.retain(|k| k != key);
                    return None;
                }
            }
            // 有效 → LRU 提升
            self.insertion_order.retain(|k| k != key);
            self.insertion_order.push_back(key.to_string());
            Some(entry.element.clone())
        } else {
            None
        }
    }

    /// 带自定义 TTL 的获取（每个 find 函数可覆盖全局 TTL）
    fn get_with_ttl(&mut self, key: &str, ttl: Option<Duration>) -> Option<UIElement> {
        if let Some(entry) = self.elements.get(key) {
            let effective_ttl = ttl.or(self.default_ttl);
            if let Some(ttl) = effective_ttl {
                if entry.cached_at.elapsed() > ttl {
                    self.elements.remove(key);
                    self.insertion_order.retain(|k| k != key);
                    return None;
                }
            }
            self.insertion_order.retain(|k| k != key);
            self.insertion_order.push_back(key.to_string());
            Some(entry.element.clone())
        } else {
            None
        }
    }
}
```

### 6.2 新增公共 API

```rust
/// 设置全局默认 TTL
pub fn set_default_ttl(ttl: Option<Duration>) {
    let mut cache = recover_lock(get_cache().write());
    cache.default_ttl = ttl;
}

/// 获取缓存统计
pub fn stats() -> (usize, usize, Option<Duration>) {
    let cache = recover_lock(get_cache().read());
    (cache.len(), cache.max_size, cache.default_ttl)
}

/// 获取缓存元素（带自定义 TTL 覆盖）
pub fn get_cached_element_with_ttl(
    runtime_id: &str,
    ttl: Option<Duration>,
) -> Option<UIElement> {
    let mut cache = recover_lock(get_cache().write());
    cache.get_with_ttl(runtime_id, ttl)
}

/// 移除特定缓存条目
pub fn remove_cached_element(runtime_id: &str) {
    let mut cache = recover_lock(get_cache().write());
    cache.elements.remove(runtime_id);
    cache.insertion_order.retain(|k| k != runtime_id);
}
```

### 6.3 新增统一元素解析函数

```rust
// src/core/uia/actions.rs (新文件)

/// 从缓存解析元素用于动作操作。
/// 缓存未命中 → 返回 Err（无 fallback）。
pub fn resolve_element_for_action(
    runtime_id: &str,
    window_selector: &str,
    element_xpath: &str,
) -> anyhow::Result<UIElement> {
    // 只从缓存获取
    match crate::core::element_cache::get_cached_element(runtime_id) {
        Some(elem) => {
            // 验证元素仍然有效（轻量检查）
            if is_element_valid(&elem) {
                Ok(elem)
            } else {
                // 元素失效，从缓存移除
                crate::core::element_cache::remove_cached_element(runtime_id);
                anyhow::bail!(
                    "缓存元素已失效: runtimeId={}, xpath={}",
                    runtime_id, element_xpath
                )
            }
        }
        None => {
            anyhow::bail!(
                "元素不在缓存中: runtimeId={}, xpath={}",
                runtime_id, element_xpath
            )
        }
    }
}

fn is_element_valid(elem: &UIElement) -> bool {
    elem.get_control_type().is_ok() || elem.get_name().is_ok()
}
```

---

## 七、鼠标控制层改造 (`src/core/uia/click.rs` 等)

所有动作函数接收 `runtime_id: Option<&str>`，通过缓存解析元素：

```rust
// 修改前：
pub fn click_element(window: &str, xpath: &str, options: &MouseClickOptions) -> Result<...> {
    let elements = find_all_elements_detailed(window, xpath, ...)?;
    // ...
}

// 修改后：
pub fn click_element(
    window: &str,
    xpath: &str,
    runtime_id: Option<&str>,
    options: &MouseClickOptions,
) -> Result<...> {
    let elem = if let Some(rid) = runtime_id {
        // 走缓存（无 fallback）
        resolve_element_for_action(rid, window, xpath)?
    } else {
        // 走 XPath 搜索
        let elements = find_all_elements_detailed(window, xpath, ...)?;
        elements.into_iter().next()
            .ok_or_else(|| anyhow::anyhow!("Element not found"))?
    };
    // ... 后续操作使用 elem ...
}
```

同样模式改造 `hover_element`, `drag_element`, `type_text` 等。

---

## 八、性能预估

| 操作 | 优化前 | 优化后（缓存命中） |
|------|--------|--------------------|
| click | 50-200ms | **1-3ms** |
| hover | 50-200ms | **1-3ms** |
| type (value) | 50-200ms | **1-3ms** |
| refresh | 50-200ms | **1-3ms** |
| flash | 50-200ms | **1-3ms** |
| checkVisibility | 50-200ms | **1-3ms** |
| find (子元素) | 50-200ms | **5-15ms** (子树搜索) |
| waitUntilGone (10次) | 500-2000ms | **10-30ms** |
| waitFor (10次) | 500-2000ms | **10-30ms** |

典型表单填写：**250ms → ~80ms，提升约 3x**。

---

## 九、兼容性

- `runtimeId` 字段全部 `Option`/`optional`，旧 SDK 不传也能正常工作（走 XPath 路径）
- `cacheTTL` 全部可选，不传用全局默认
- 无隐式 fallback 行为由**后端**控制：有 runtimeId 就只查缓存，不命中直接返回错误
- XPath 搜索路径完整保留，作为无 runtimeId 场景的正常路径

---

## 十、实施步骤

### Phase 1: 缓存基础设施（1-2天）

1. **Core**：`element_cache.rs` 加 TTL 支持 + 新公共 API
2. **Core**：新建 `actions.rs`，实现 `resolve_element_for_action()`
3. **API**：新增 `POST /api/element/refresh` 端点
4. **API**：新增 `PUT /api/element/cache/config` + `GET /api/element/cache/stats` + `POST /api/element/cache/clear`
5. **测试**：验证缓存 TTL 过期逻辑

### Phase 2: 动作链路改造（1-2天）

1. **API types.rs**：所有请求体加 `runtime_id: Option<String>`
2. **API element.rs**：所有端点优先查缓存，无 fallback
3. **Core**：`click.rs` / `hover.rs` / `drag.rs` / `type.rs` 等动作函数加 `runtime_id` 参数
4. **SDK client.ts**：所有 HTTP 方法加 `runtimeId` 参数
5. **SDK element.ts**：`click/hover/type/refresh/flash/visibility/inspect` 传 `runtimeId`

### Phase 3: 子元素查找改造（1-2天）

1. **SDK element.ts**：`find/findOne/findFirst/findAll/children` 改用 `findFromElement`
2. **SDK element.ts**：`parent/next/prev` 支持 runtimeId 起点
3. **SDK element.ts**：`compass/navigate` 传入 runtimeId
4. **SDK types.ts**：新增 `FindOptions { cacheTTL, propNames }`

### Phase 4: 等待/循环 + 清理（1天）

1. **SDK element.ts**：`waitUntilGone/waitFor` 复用缓存
2. 边缘场景测试（缓存过期、元素销毁、并发）
3. 文档更新
