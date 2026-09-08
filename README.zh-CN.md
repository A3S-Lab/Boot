# A3S Boot

<p align="center">
  <strong>Language / 语言:</strong>
  <a href="README.md">English</a> ·
  <a href="README.zh-CN.md">中文</a>
</p>

<p align="center">
  <strong>A3S 的渐进式 Rust Web 框架</strong>
</p>

<p align="center">
  <em>使用类型化Provider、显式管道和可替换协议适配器构建模块化异步服务</em>
</p>

<p align="center">
  <a href="https://a3s-lab.github.io/Boot/">文档</a> •
  <a href="#overview">概述</a> •
  <a href="#features">功能</a> •
  <a href="#quick-start">快速开始</a> •
  <a href="#application-model">应用模型</a> •
  <a href="#protocols">协议</a> •
  <a href="#architecture">架构</a> •
  <a href="#development">开发</a>
</p>

---

## 概述

**A3S Boot** 是 Rust 的模块化异步服务框架，灵感来自
[Nest.js](https://nestjs.com/)。模块组织应用程序、类型Provider
供应依赖性、控制器公开路由以及协议中立的管道
应用防护、拦截器、管道、验证和异常过滤器。

Boot 不是 Axum 包装器。请求、响应、路由和执行上下文
属于框架核心； Axum 是捆绑的默认 HTTP 适配器。铁锈
属性宏在编译时生成普通的 Boot 定义，而不是
依赖运行时装饰器元数据。

[文档网站](https://a3s-lab.github.io/Boot/)提供了完整的
v0.2.0和v0.1.4的中英文指南，包括同页语言
和版本切换。

### 基本用法

```rust,no_run
use a3s_boot::{
    AxumAdapter, BootApplication, BootResponse, ControllerDefinition, Module,
    ModuleRef, ProviderDefinition, Result,
};

#[derive(Debug)]
struct GreetingService;

impl GreetingService {
    fn hello(&self) -> &'static str {
        "Hello from A3S Boot"
    }
}

#[derive(Debug)]
struct AppModule;

impl Module for AppModule {
    fn name(&self) -> &'static str {
        "app"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(GreetingService)])
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let greeting = module_ref.get::<GreetingService>()?;
        Ok(vec![ControllerDefinition::new("/")?.get("/", move |_| {
            let greeting = greeting.clone();
            async move { Ok(BootResponse::text(greeting.hello())) }
        })?])
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = BootApplication::builder().import(AppModule).build()?;
    app.serve_with(&AxumAdapter::new(), ([127, 0, 0, 1], 3000).into())
        .await
}
```

## 特征

- **应用程序模块**：组合导入、提供者、控制器、网关、
  消息控制器、导出、路由前缀和生命周期挂钩
- **类型化依赖注入**：解析类型化或命名的提供者
  单例、请求和瞬态作用域
- **HTTP 控制器**：定义与适配器无关的路由、类型化输入、JSON 或原始数据
  响应、cookie、重定向、文件、视图和服务器发送的事件
- **执行管道**：在拦截器、管道周围应用中间件、防护装置、
  全局和局部范围内的验证和异常过滤器
- **编译时宏**：对模块、Provider使用 Nest 样式属性，
  控制器、路由、提取、验证、OpenAPI 和协议处理程序
- **OpenAPI**：生成 OpenAPI 文档并从路由提供 Swagger UI
  元数据、可重用组件和安全方案
- **WebSocket 网关**：处理订阅、连接生命周期、房间、
  广播和特定于协议的管道挂钩
- **微服务**：通过调度请求响应和事件模式
  进程内或可选的网络传输
- **应用程序生命周期**：引导、关闭、延迟加载模块或创建
  仅提供者的应用程序上下文和独立的微服务
- **测试支持**：编译测试模块并覆盖Provider或管道
  无需替换应用程序代码的组件

### 特征矩阵

默认特征为 `axum`、`macros` 和 `shutdown-hooks`。其他集成
正在选择加入。

|面积 |特色|包含的功能 |
| ---| ---| ---|
| HTTP 运行时 | `axum` | Axum HTTP 和 WebSocket 适配器 |
|编译时API | `macros` |嵌套式程序属性 |
|生命周期| `shutdown-hooks` | SIGINT 和 SIGTERM 关闭处理 |
|配置| `config` | ACL 支持的类型化配置解析 |
|认证| `auth` |策略支持的身份验证防护 |
|安全| `security` | CORS、CSRF、本地或提供商支持的速率限制和安全标头 |
|会议 | `session` |会话中间件和可替换存储|
|缓存| `cache` |缓存抽象、拦截器和内存存储 |
|数据库| `database` |可替换的数据库外观和内存后端|
|活动 | `events` | A3S 事件支持的发射器和侦听器 |
| CQRS | `cqrs` |命令、查询和事件总线 |
|队列| `queue` | A3S Lane 支持的进程内作业、重试、优先级和处理器 |
|队列持久化 | `queue-postgres` | A3S ORM 支持的共享 PostgreSQL 租赁、恢复、防护和保留 |
|日程安排 | `schedule` | Cron、间隔和超时作业 |
|可观察性| `logging`、`health` |结构化日志记录和健康指标|
| HTTP 实用程序 | `http-client`、`compression` |出站 HTTP 和 gzip 响应 |
|频道 | `ilink` |腾讯微信iLink二维码登录、投票、消息和生命周期客户端 |
|内容 | `file-upload`、`static` |分段上传和静态文件 |
|背景 | `request-context` |任务本地访问当前请求|
|开放API | `openapi-schemas` |基于`schemars`的组件模式|
|交通 | `tcp-transport`、`redis-transport`、`nats-transport` | TCP、Redis 和 NATS 消息传递 |
|交通 | `mqtt-transport`、`rabbitmq-transport`、`kafka-transport` | MQTT、RabbitMQ 和 Kafka 消息传递 |
|交通 | `grpc-transport` |一元 gRPC 消息传递 |

一个特性暴露了相应的框架集成；外部运输
仍然需要他们的经纪人或服务可用。数据库、缓存、会话、
队列和调度程序 API 是后端抽象。 `queue-postgres` 功能
是持久共享队列的实现；其他捆绑实现是
并不声称支持每个生产后端。

## 快速开始

### 安装

```toml
[dependencies]
a3s-boot = "0.2.0"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

对于没有 Axum、宏或关闭信号处理的纯核心构建：

```toml
[dependencies]
a3s-boot = { version = "0.2.0", default-features = false }
```

仅启用应用程序使用的可选模块：

```toml
[dependencies]
a3s-boot = { version = "0.2.0", features = ["auth", "security", "openapi-schemas"] }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

### 基于属性的控制器

默认功能集包括编译时宏。提供者和控制者
保持正常的 Rust 类型，而属性生成其引导注册。

```rust,no_run
use std::sync::Arc;

use a3s_boot::{
    controller, get, injectable, module, param, AxumAdapter, BootFactory, Result,
};

#[injectable]
#[derive(Debug)]
struct GreetingService;

impl GreetingService {
    fn hello(&self, name: &str) -> String {
        format!("Hello, {name}")
    }
}

#[injectable]
#[derive(Debug)]
struct GreetingController {
    greeting: Arc<GreetingService>,
}

#[controller("/greetings")]
impl GreetingController {
    #[get("/{name}")]
    async fn greet(&self, #[param("name")] name: String) -> Result<String> {
        Ok(self.greeting.hello(&name))
    }
}

#[module(
    name = "app",
    providers = [GreetingService, GreetingController],
    controllers = [GreetingController],
)]
#[derive(Debug)]
struct AppModule;

#[tokio::main]
async fn main() -> Result<()> {
    let mut app = BootFactory::create(AppModule)?;
    app.listen_with(&AxumAdapter::new(), ([127, 0, 0, 1], 3000).into())
        .await
}
```

显式构建器 API 仍然可用于动态注册、适配器、
以及不喜欢使用过程宏的应用程序。

## 应用模型

### 模块和提供者

`Module` 拥有特征边界。它可以导入其他模块，注册和
导出Provider、公开 HTTP 控制器、附加 WebSocket 网关和消息
模式、配置中间件以及参与启动或关闭。

提供者使用类型化或命名的令牌并支持值、工厂、异步工厂、
和别名定义。 `ModuleRef` 解决模块可见性内的依赖关系
规则。 `ProviderRef<T>` 推迟可选或圆形图表的分辨率。
提供者范围可以是单例、请求范围或瞬态；请求范围是
通过急切的依赖关系传播。

`DynamicModule`支持运行时模块配置，而`LazyModuleLoader`
启动后加载隔离的功能模块。应用范围的管道提供商
必须立即导入，因为稍后加载它们会更改已编译的内容
处理程序。

### 工厂和生命周期

`BootFactory` 是托管入口点：

- `create` 构建支持 HTTP 的应用程序
- `create_application_context` 构建仅提供者的工作线程
- `create_microservice`构建独立的消息服务
- 异步变体支持异步提供者工厂

模块和提供者可以观察初始化、引导、销毁和
应用程序关闭。关闭钩子可以在以下情况下侦听 SIGINT 和 SIGTERM：
`shutdown-hooks` 功能已启用。

### 持久的 PostgreSQL 队列

当多个进程中的工作人员必须共享持久性时启用`queue-postgres`
工作。 `PostgresQueueBackend` 通过 A3S ORM 存储作业，将就绪作业租赁给
PostgreSQL `SKIP LOCKED`，续订实时租约，隔离过时的工作人员并恢复
工人或进程死亡后租约到期。

```toml
[dependencies]
a3s-boot = { version = "0.2.0", features = ["queue-postgres"] }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

```rust,no_run
use std::time::Duration;

use a3s_boot::{
    ModuleRef, PostgresQueueBackend, Queue, QueueContext, QueueJob, QueueOptions,
    Result,
};
use serde_json::json;

async fn run_worker(database_url: &str) -> Result<()> {
    let options = QueueOptions::new()
        .with_worker_count(4)
        .with_lease_duration(Duration::from_secs(30));
    let backend = PostgresQueueBackend::connect(database_url, "workflow", options).await?;
    let queue = Queue::new("workflow", backend);
    queue.process("resume", |job: QueueJob, _context: QueueContext| async move {
        println!("resuming {}", job.data["runId"]);
        Ok(())
    })?;
    queue.start(ModuleRef::new()).await?;
    queue.enqueue("resume", &json!({"runId": "run-42"})).await?;
    queue.shutdown().await
}
```

使用数据库 URL，其搜索路径选择专用于 Boot 的架构。的
主机应用程序创建该模式； Boot 拥有队列表及其 A3S ORM
里面有迁移账本。共享流程或应用程序架构可以使一个
组件接受另一个组件的迁移历史。

`connect` 仍然是应用以下方法的单进程便利边界
规范迁移集。生产托管单独的权限：终止
迁移过程调用`migrate_postgres_queue`，同时服务worker使用
`connect_verified` 或 `from_executor_verified`。已验证的构造函数重用
A3S ORM的只读账本准入并且从不创建表，获取一个
迁移锁，或者写入迁移历史。

后端支持调用者分配的幂等键、优先级和 FIFO/LIFO
排序、延迟、重试、处理器超时、终端保留、重复数据删除、
积极保持最新的后继者，以及优雅的租约释放。交货是
至少一次，因此处理者必须使业务效果幂等。重复工作
并且车道父/子流选项被明确拒绝。异步服务可以
在保留上使用 `jobs_async`、`failures_async`、`stats_async` 和 `clear_async`
用于非阻塞诊断的后端句柄。

### 请求管道

HTTP 处理程序通过确定性中间件和管道阶段运行：

```text
request → middleware → guards → interceptors → pipes → validation → handler
                              └──── exception filters on unrecovered errors ────┘
```

周围拦截器收到一个`CallHandler`，因此它们可以转换结果，
短路执行、恢复错误或重放剩余管道
顺序重试。重试具有至少一次语义：提供者状态和
外部副作用不会回滚。

等效的 WebSocket 和传输钩子使用相同的执行模型
特定于协议的上下文和回复。

## HTTP 和 OpenAPI

控制器支持标准 HTTP 方法、主机和路径路由、查询和正文
DTO、标头、cookie、客户端 IP 提示、自定义提取、重定向、响应
直通、流文件、服务器发送的事件以及 URI、标头或媒体类型
版本控制。

通过 `Validate` 和可选的 `ValidationSchema` 进行显式验证
实施。转换、白名单和拒绝未知字段策略可以
全局应用、每个控制器或每个处理程序。

OpenAPI 元数据可以通过构建器或属性附加。引导生成
OpenAPI 文档，支持可重用模式和安全方案，并且可以
同时提供 JSON 和 Swagger UI。启用`openapi-schemas`收集模式
来自 `schemars::JsonSchema` 类型。

可选的 HTTP 模块添加分段上传、静态文件、gzip 压缩、
视图、会话、安全策略、请求上下文和出站 HTTP 客户端。

### 提供商支持的速率限制

默认情况下，`security` 功能将 `use_global_rate_limit` 保留在进程本地。
需要跨多个流程的一个预算的应用程序可以实现
public `RateLimitProvider` 合约并注册到
`use_global_rate_limit_provider`。每个原子获取都会收到一个稳定的
策略标识符、策略范围的 SHA-256 主题摘要以及配置的
请求限制和窗口。选定的标头值和承载凭证不
以明文方式跨越提供商边界。

使用相同策略标识符的每个进程必须使用相同的限制和
窗户。提供者错误拒绝受保护的请求而不是绕过
限制。 Boot故意不选择或捆绑分布式后端；的
内置`InMemoryRateLimitProvider`不在进程之间共享状态。
该边界不包括单独的流断开、背压、
或优雅的排水工作。

## 协议

### WebSocket

`WebSocketGatewayDefinition` 和 `#[websocket_gateway]` 定义升级路径和
消息订阅。网关支持初始化、连接和断开
挂钩、直接消息、房间、广播、类型化有效负载提取、验证、
以及 WebSocket 特定的防护、拦截器、管道和过滤器。捆绑的
Axum 适配器执行真正的 WebSocket 升级。

### 微服务

消息控制器定义请求响应模式和仅事件模式。
`InProcessTransport` 始终可用于测试、工作人员和同进程
沟通。可选功能添加 TCP、Redis、NATS、MQTT、RabbitMQ、Kafka、
gRPC 在通用 `MessageTransport` 合约背后进行传输。

传输实现共享类型化的有效负载处理、范围提供者、
验证、防护、拦截器、管道、异常过滤器和客户端 API。
协议交付和持久性语义仍然取决于所选的后端。

### 微信iLink

可选的 `ilink` 功能提供了本机 Rust 协议边界
腾讯微信频道。 `IlinkModule` 导出类型化的 `IlinkClient`
提供者；客户端拥有 QR 登录请求、经过身份验证的标头、严格的
服务器 URL 验证、更新轮询、文本回复、打字调用和频道
开始/停止通知。

```rust
use a3s_boot::ilink::IlinkModule;

let module = IlinkModule::weixin("A3S/0.10.1");
```

线材默认兼容腾讯`openclaw-weixin` v2.4.6：
`iLink-App-Id: bot`，`bot_type=3`，以及打包客户端版本`2.4.6`。的
产品特定的 `bot_agent` 保留 `A3S/<version>`，因此上游诊断会这样做
不会误认来电者。 Boot故意不拥有浏览器API，
凭证持久性、所有者授权或代理/会话命令；那些
策略保留在主机应用程序中。

## 架构

应用程序核心独立于其 HTTP 服务器和消息代理：

```text
modules + typed providers
          │
 controllers / gateways / message patterns
          │
 protocol-neutral execution pipeline
          │
 BootRequest / WebSocketMessage / TransportMessage
          │
 HTTP adapter / WebSocket adapter / MessageTransport
```

来源按责任划分为`app/`、`module/`、`provider/`、
`routing/`、`pipeline/`、`websocket/` 和 `transport/`。可选的基础设施
模块是功能门控的。公众`HttpAdapter`、`MessageTransport`、以及
后端特征是主要的扩展合约。

Axum 当前是捆绑的 HTTP 适配器。编译时属性位于
单独的`a3s-boot-macros`板条箱并扩展为相同的显式定义
由核心 API 使用。

## 发展

从 `a3s-boot` crate 目录运行检查：

```bash
cargo fmt --all -- --check
cargo test
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps
```

测试套件涵盖模块和Provider、范围上下文、生命周期、
路由、管道、宏、验证、OpenAPI、WebSocket、传输和
功能门控基础设施。外部传输的测试可能需要它们
相应的服务或环境配置。

有关 Nest 兼容性计划和剩余工作，请参阅 [路线图](ROADMAP.md)]。

版本化双语文档站点位于 `website/`：

```bash
cd website
npm ci
npm run check
npm run build
npm run check:site
```

## 执照

麻省理工学院