import { useState } from 'react';
import {
  ArrowRight,
  BracketsCurly,
  Check,
  CirclesThree,
  Copy,
  FlowArrow,
  PlugsConnected,
  ShieldCheck,
} from '@phosphor-icons/react';
import { useLang, useSite, useVersion, withBase } from '@rspress/core/runtime';
import { moveTabFocus, useCopyFeedback } from './HomeControls';

type Locale = 'zh' | 'en';
type Surface = 'http' | 'websocket' | 'message';

const surfaceIds: readonly Surface[] = ['http', 'websocket', 'message'];

const copy = {
  zh: {
    title: ['模块化应用。', 'Rust 级控制。'],
    body: '组合模块、Provider、Controller 与协议管线，同时保留适配器和运行时边界。',
    start: '开始使用',
    architecture: '查看应用模型',
    demoRegion: '交互式 A3S Boot 执行管线',
    surfaceTabs: '协议入口',
    input: '注册入口',
    pipeline: '执行阶段',
    installTitle: '默认即可启动，按需打开能力。',
    installBody:
      '默认包含 Axum、属性宏和优雅停机。其他模块全部通过 feature 显式启用。',
    copyCommand: '复制依赖配置',
    copied: '已复制',
    copyFailed: '复制失败',
    contractTitle: '熟悉的模块边界，明确的 Rust 所有权。',
    contractBody:
      'Boot 借鉴 NestJS 的组织方式，但不会隐藏依赖图、协议适配器或错误类型。',
    capabilities: [
      {
        title: '模块与依赖注入',
        body: '可见性、导入导出、scope、异步 factory 和生命周期都进入类型化容器。',
        code: 'ModuleRef',
      },
      {
        title: 'Controller 与宏',
        body: '普通 Rust 类型通过属性生成注册，同时保留显式 builder API。',
        code: '#[controller]',
      },
      {
        title: '确定性请求管线',
        body: 'Middleware、Guard、Interceptor、Pipe、验证和 Filter 有固定顺序。',
        code: 'ExecutionContext',
      },
      {
        title: '多协议执行模型',
        body: 'HTTP、WebSocket 与消息传输共享 scope、验证和增强器语义。',
        code: 'MessageTransport',
      },
    ],
    flowTitle: '从模块声明到可服务应用，只有四个阶段。',
    flowBody: '每个阶段都能被测试、检查和替换。',
    flow: [
      ['声明边界', '模块列出 Provider、Controller、Gateway 与导入'],
      ['编译图', '验证 token、可见性、scope 与路由冲突'],
      ['解析实例', '按 singleton、request 或 transient 构造依赖'],
      ['连接适配器', '由 Axum 或消息 transport 接管网络边界'],
    ],
    systemsTitle: '一个应用核心，面向多种入口。',
    appTitle: '应用与技术模块',
    appBody:
      '配置、日志、缓存、数据库、会话、安全、CQRS、事件、调度、健康与文件能力按需组合。',
    appPoints: ['Provider-first 设计', '全局与局部增强器', '测试模块覆盖'],
    protocolTitle: '协议与后台工作',
    protocolBody:
      'HTTP、WebSocket、TCP、Redis、NATS、MQTT、RabbitMQ、Kafka、gRPC、队列与 iLink 使用明确契约。',
    protocolPoints: ['适配器可替换', '协议专属上下文', '交付语义不被抽象掩盖'],
    browse: '浏览所有能力',
    boundariesTitle: '框架提供机制，部署策略仍由应用决定。',
    boundaries: [
      '默认 HTTP 适配器是 Axum，核心应用模型不依赖 Axum。',
      '可选 feature 不会在未启用时引入对应服务依赖。',
      '队列是至少一次交付，业务处理器必须保持幂等。',
      '分布式限流、broker durability 与 secret 存储由部署选择。',
    ],
    production: '阅读生产边界',
    ctaTitle: '从一个模块和一条路由开始。',
    ctaBody: '先运行默认 Axum 应用，再按业务边界启用模块与协议。',
  },
  en: {
    title: ['Modular applications.', 'Rust-level control.'],
    body: 'Compose modules, providers, controllers, and protocol pipelines without hiding adapters or runtime boundaries.',
    start: 'Get started',
    architecture: 'Read the application model',
    demoRegion: 'Interactive A3S Boot execution pipeline',
    surfaceTabs: 'Protocol entry point',
    input: 'Registered entry point',
    pipeline: 'Execution stages',
    installTitle: 'Start with defaults. Add capabilities explicitly.',
    installBody:
      'Defaults include Axum, attribute macros, and graceful shutdown. Every other module is feature-gated.',
    copyCommand: 'Copy dependency',
    copied: 'Copied',
    copyFailed: 'Copy failed',
    contractTitle: 'Familiar module boundaries. Explicit Rust ownership.',
    contractBody:
      'Boot borrows NestJS organization without hiding the dependency graph, protocol adapter, or error types.',
    capabilities: [
      {
        title: 'Modules and dependency injection',
        body: 'Visibility, imports, exports, scopes, async factories, and lifecycle live in a typed container.',
        code: 'ModuleRef',
      },
      {
        title: 'Controllers and macros',
        body: 'Attributes generate registration for ordinary Rust types while explicit builders remain available.',
        code: '#[controller]',
      },
      {
        title: 'Deterministic request pipeline',
        body: 'Middleware, guards, interceptors, pipes, validation, and filters have a fixed order.',
        code: 'ExecutionContext',
      },
      {
        title: 'Multi-protocol execution',
        body: 'HTTP, WebSocket, and message transports share scope, validation, and enhancer semantics.',
        code: 'MessageTransport',
      },
    ],
    flowTitle: 'Four stages from module declaration to a serving app.',
    flowBody: 'Every stage remains testable, inspectable, and replaceable.',
    flow: [
      [
        'Declare boundaries',
        'Modules list providers, controllers, gateways, and imports',
      ],
      [
        'Compile the graph',
        'Validate tokens, visibility, scopes, and route conflicts',
      ],
      [
        'Resolve instances',
        'Construct singleton, request, or transient dependencies',
      ],
      [
        'Attach adapters',
        'Axum or a message transport takes the network boundary',
      ],
    ],
    systemsTitle: 'One application core. Multiple entry points.',
    appTitle: 'Application and technique modules',
    appBody:
      'Compose config, logging, cache, database, sessions, security, CQRS, events, scheduling, health, and files on demand.',
    appPoints: [
      'Provider-first design',
      'Global and local enhancers',
      'Testing module overrides',
    ],
    protocolTitle: 'Protocols and background work',
    protocolBody:
      'HTTP, WebSocket, TCP, Redis, NATS, MQTT, RabbitMQ, Kafka, gRPC, queues, and iLink use explicit contracts.',
    protocolPoints: [
      'Replaceable adapters',
      'Protocol-specific contexts',
      'Delivery semantics remain visible',
    ],
    browse: 'Browse every capability',
    boundariesTitle:
      'The framework supplies mechanisms. Applications choose deployment policy.',
    boundaries: [
      'Axum is the default HTTP adapter, while the application core remains Axum-independent.',
      'Optional features do not pull service dependencies into disabled builds.',
      'Queue delivery is at least once, so business processors must be idempotent.',
      'Distributed rate limits, broker durability, and secret storage are deployment choices.',
    ],
    production: 'Read production boundaries',
    ctaTitle: 'Start with one module and one route.',
    ctaBody:
      'Run the default Axum app, then enable modules and protocols at business boundaries.',
  },
} as const;

const surfaces = {
  http: {
    label: 'HTTP',
    code: `#[controller("/users")]
impl UserController {
  #[get("/{id}")]
  async fn find(#[param("id")] id: Uuid) {}
}`,
    stages: ['Middleware', 'Guard', 'Interceptor', 'Pipe', 'Handler'],
  },
  websocket: {
    label: 'WebSocket',
    code: `#[websocket_gateway("/events")]
impl EventsGateway {
  #[subscribe_message("join")]
  async fn join(#[message_body] room: String) {}
}`,
    stages: ['Upgrade', 'Guard', 'Pipe', 'Handler', 'Room'],
  },
  message: {
    label: 'Message',
    code: `#[message_controller]
impl MathController {
  #[message_pattern("sum")]
  async fn sum(#[payload] values: Vec<i64>) {}
}`,
    stages: ['Transport', 'Scope', 'Guard', 'Pipe', 'Reply'],
  },
} as const;

function MarkdownHome({
  locale,
  release,
}: {
  locale: Locale;
  release: string;
}) {
  const text = copy[locale];
  return (
    <main>
      <h1>{text.title.join(' ')}</h1>
      <p>{text.body}</p>
      <h2>{text.installTitle}</h2>
      <pre>
        <code>{`a3s-boot = "${release}"`}</code>
      </pre>
      <h2>{text.contractTitle}</h2>
      {text.capabilities.map((item) => (
        <section key={item.title}>
          <h3>{item.title}</h3>
          <p>{item.body}</p>
        </section>
      ))}
      <h2>{text.flowTitle}</h2>
      <p>{text.flowBody}</p>
      <h2>{text.systemsTitle}</h2>
      <p>{text.appBody}</p>
      <p>{text.protocolBody}</p>
    </main>
  );
}

export function HomeLayout() {
  const locale: Locale = useLang() === 'zh' ? 'zh' : 'en';
  const text = copy[locale];
  const version = useVersion();
  const release = version.replace(/^v/, '');
  const { site } = useSite();
  const [surface, setSurface] = useState<Surface>('http');
  const { copyStatus, copyText } = useCopyFeedback();
  const routePrefix = [
    version !== site.multiVersion.default ? version : '',
    locale !== site.lang ? locale : '',
  ]
    .filter(Boolean)
    .join('/');
  const route = (pathname: string) => {
    const normalized = pathname.replace(/^\/+/, '');
    return withBase(`/${[routePrefix, normalized].filter(Boolean).join('/')}`);
  };
  const installCommand = `a3s-boot = "${release}"`;
  const activeSurface = surfaces[surface];

  if (import.meta.env.SSG_MD) {
    return <MarkdownHome locale={locale} release={release} />;
  }

  const copyInstallCommand = () => copyText(installCommand);

  return (
    <main className="product-home boot-home" data-boot-home>
      <section className="product-hero">
        <div className="product-hero__copy">
          <h1>
            {text.title.map((line) => (
              <span key={line}>{line}</span>
            ))}
          </h1>
          <p>{text.body}</p>
          <div className="product-actions">
            <a
              className="product-button product-button--primary"
              href={route('/getting-started/quick-start')}
            >
              {text.start}
              <ArrowRight aria-hidden="true" size={17} weight="bold" />
            </a>
            <a
              className="product-button product-button--secondary"
              href={route('/core/application-model')}
            >
              {text.architecture}
            </a>
          </div>
        </div>

        <div aria-label={text.demoRegion} className="boot-demo" role="region">
          <div
            aria-label={text.surfaceTabs}
            className="boot-demo__tabs"
            role="tablist"
          >
            {surfaceIds.map((item, index) => (
              <button
                aria-controls="boot-demo-panel"
                aria-selected={surface === item}
                id={`boot-surface-tab-${item}`}
                key={item}
                onClick={() => setSurface(item)}
                onKeyDown={(event) =>
                  moveTabFocus(event, surfaceIds, index, setSurface)
                }
                role="tab"
                tabIndex={surface === item ? 0 : -1}
                type="button"
              >
                {surfaces[item].label}
              </button>
            ))}
          </div>
          <div
            aria-labelledby={`boot-surface-tab-${surface}`}
            className="boot-demo__body"
            id="boot-demo-panel"
            role="tabpanel"
          >
            <div className="boot-demo__code">
              <span>{text.input}</span>
              <pre>
                <code>{activeSurface.code}</code>
              </pre>
            </div>
            <div className="boot-demo__pipeline" aria-live="polite">
              <span>{text.pipeline}</span>
              <ol>
                {activeSurface.stages.map((stage, index) => (
                  <li key={stage}>
                    <b>{String(index + 1).padStart(2, '0')}</b>
                    <strong>{stage}</strong>
                    {index < activeSurface.stages.length - 1 && (
                      <ArrowRight aria-hidden="true" size={14} weight="bold" />
                    )}
                  </li>
                ))}
              </ol>
            </div>
          </div>
        </div>
      </section>

      <section className="install-rail">
        <div>
          <h2>{text.installTitle}</h2>
          <p>{text.installBody}</p>
        </div>
        <div className="install-command">
          <code>{installCommand}</code>
          <button
            aria-label={
              copyStatus === 'copied'
                ? text.copied
                : copyStatus === 'failed'
                  ? text.copyFailed
                  : text.copyCommand
            }
            onClick={copyInstallCommand}
            type="button"
          >
            {copyStatus === 'copied' ? (
              <Check aria-hidden="true" size={17} weight="bold" />
            ) : (
              <Copy aria-hidden="true" size={17} />
            )}
            <span aria-live="polite">
              {copyStatus === 'copied'
                ? text.copied
                : copyStatus === 'failed'
                  ? text.copyFailed
                  : text.copyCommand}
            </span>
          </button>
        </div>
      </section>

      <section className="product-section contract-section">
        <header className="section-heading">
          <h2>{text.contractTitle}</h2>
          <p>{text.contractBody}</p>
        </header>
        <div className="contract-grid">
          {text.capabilities.map((item, index) => {
            const Icon = [
              CirclesThree,
              BracketsCurly,
              FlowArrow,
              PlugsConnected,
            ][index];
            return (
              <article key={item.title}>
                <div className="contract-grid__icon">
                  <Icon aria-hidden="true" size={21} />
                </div>
                <code>{item.code}</code>
                <h3>{item.title}</h3>
                <p>{item.body}</p>
              </article>
            );
          })}
        </div>
      </section>

      <section className="product-section flow-section">
        <header className="section-heading">
          <h2>{text.flowTitle}</h2>
          <p>{text.flowBody}</p>
        </header>
        <ol className="query-flow">
          {text.flow.map(([title, body], index) => (
            <li key={title}>
              <span>{String(index + 1).padStart(2, '0')}</span>
              <h3>{title}</h3>
              <p>{body}</p>
            </li>
          ))}
        </ol>
      </section>

      <section className="product-section runtime-section">
        <header className="section-heading">
          <h2>{text.systemsTitle}</h2>
        </header>
        <div className="runtime-comparison">
          <article>
            <div>
              <CirclesThree aria-hidden="true" size={24} />
            </div>
            <h3>{text.appTitle}</h3>
            <p>{text.appBody}</p>
            <ul>
              {text.appPoints.map((point) => (
                <li key={point}>
                  <Check aria-hidden="true" size={15} weight="bold" />
                  {point}
                </li>
              ))}
            </ul>
          </article>
          <article>
            <div>
              <PlugsConnected aria-hidden="true" size={24} />
            </div>
            <h3>{text.protocolTitle}</h3>
            <p>{text.protocolBody}</p>
            <ul>
              {text.protocolPoints.map((point) => (
                <li key={point}>
                  <Check aria-hidden="true" size={15} weight="bold" />
                  {point}
                </li>
              ))}
            </ul>
          </article>
        </div>
        <a className="text-link" href={route('/reference/features-and-api')}>
          {text.browse}
          <ArrowRight aria-hidden="true" size={16} weight="bold" />
        </a>
      </section>

      <section className="product-section boundaries-section">
        <div>
          <h2>{text.boundariesTitle}</h2>
          <a
            className="text-link"
            href={route('/reference/architecture-and-roadmap')}
          >
            {text.production}
            <ArrowRight aria-hidden="true" size={16} weight="bold" />
          </a>
        </div>
        <ul>
          {text.boundaries.map((boundary) => (
            <li key={boundary}>
              <ShieldCheck aria-hidden="true" size={17} weight="bold" />
              <span>{boundary}</span>
            </li>
          ))}
        </ul>
      </section>

      <section className="product-cta">
        <div>
          <h2>{text.ctaTitle}</h2>
          <p>{text.ctaBody}</p>
        </div>
        <a
          className="product-button product-button--primary"
          href={route('/getting-started/quick-start')}
        >
          {text.start}
          <ArrowRight aria-hidden="true" size={17} weight="bold" />
        </a>
      </section>
    </main>
  );
}
