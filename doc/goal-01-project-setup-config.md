# Goal 01: 项目初始化与配置解析

## 目标

搭建 Rust 项目骨架，实现 `config.toml` 和 `tools.d/*.toml` 的配置解析，建立 lib.rs 对外暴露核心模块。

## 涉及文件

```
Cargo.toml
src/
├── main.rs              # 入口，加载配置并启动（本阶段仅加载配置并打印）
├── lib.rs               # library 入口，导出 config 模块
└── config/
    ├── mod.rs            # 模块声明
    ├── server.rs         # ServerConfig: 解析 config.toml
    └── tool.rs           # ToolConfig: 解析 tools.d/*.toml，构建 tool 注册表
config.toml              # 示例 server 配置
tools.d/
├── git.toml             # 示例 command tool
└── http_test.toml       # 示例 http tool
```

## 实现方式

### 依赖

- `serde` + `serde_derive`：结构体序列化/反序列化
- `toml`：TOML 文件解析
- `thiserror`：错误类型定义
- `tracing` + `tracing-subscriber`：日志

### ServerConfig（config.toml）

```rust
pub struct ServerConfig {
    pub server: ServerSection,
    pub defaults: DefaultsSection,
    pub security: SecuritySection,
}

pub struct ServerSection {
    pub host: String,       // 默认 "127.0.0.1"
    pub port: u16,          // 默认 3000
}

pub struct DefaultsSection {
    pub timeout_secs: u64,          // 全局超时上限
    pub allowed_dirs: Vec<PathBuf>, // 目录白名单
}

pub struct SecuritySection {
    pub allow_direct_command: bool,  // 默认 false
}
```

### ToolConfig（tools.d/*.toml）

每个 `.toml` 文件结构：

```rust
pub struct ToolFile {
    pub config: Option<ToolFileConfig>,  // [config] 公共配置
    pub tools: Vec<ToolDef>,             // [[tools]] 定义列表
}

pub struct ToolFileConfig {
    pub working_dir: Option<String>,
    pub timeout_secs: Option<u64>,
    pub env: Option<HashMap<String, String>>, // 全局环境变量列表
    pub base_url: Option<String>,       // HTTP 类型用
}

pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub r#type: Option<ToolType>,       // 默认 command
    // command 类型字段
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>, // 单个命令独立的环境变量
    pub sub_dir: Option<String>,
    // http 类型字段
    pub method: Option<String>,
    pub path: Option<String>,
    pub body: Option<String>,
    pub content_type: Option<String>,
    // 通用
    pub timeout_secs: Option<u64>,
    pub parameters: Option<Vec<ParameterDef>>,
}

pub enum ToolType {
    Command,
    Http,
}

pub struct ParameterDef {
    pub name: String,
    pub description: String,
    pub r#type: String,                 // "string", "number", "boolean"
    pub required: bool,
}
```

### Tool 注册表

```rust
pub struct ToolRegistry {
    tools: HashMap<String, RegisteredTool>,
}

pub struct RegisteredTool {
    pub def: ToolDef,
    pub working_dir: Option<PathBuf>,   // 来自 [config].working_dir
    pub base_url: Option<String>,       // 来自 [config].base_url
    pub effective_timeout: u64,         // 合并后的超时
    pub env: HashMap<String, String>,   // 组合后的环境变量字典
}
```

加载流程：
1. 扫描 `tools.d/` 目录下所有 `.toml` 文件
2. 逐个解析，继承 `[config]` 的公共配置到每个 tool
3. 合并 `env`：同名 key 下 tool 定义覆盖配置 `[config]` 级别定域 
4. 计算 `effective_timeout`（取 tool 级别和全局中较小的）
5. 插入 HashMap，name 重复则返回错误

### lib.rs 导出

```rust
pub mod config;
```

### main.rs（本阶段）

```rust
fn main() {
    // 初始化日志
    // 加载 ServerConfig
    // 加载 ToolRegistry
    // 打印加载结果（tool 数量、名称列表）
}
```

## 测试

### 单元测试

1. **ServerConfig 解析**
   - 正常解析完整 config.toml
   - 缺少可选字段时使用默认值
   - 格式错误时返回明确错误

2. **ToolFile 解析**
   - 解析包含 `[config]` + 多个 `[[tools]]` 的文件
   - 无 `[config]` 时 tool 正常加载
   - command 类型 tool 解析（含参数定义）
   - http 类型 tool 解析

3. **ToolRegistry 构建**
   - 从多个 toml 文件构建注册表
   - tool name 重复时报错
   - timeout 合并逻辑：取 tool 级别和全局中较小的
   - `[config].working_dir` 正确继承到每个 tool
   - `env` 环境变量字典能按层次正确应用合并（覆盖逻辑正常）

### 集成测试

- 使用 `tests/` 目录下的示例配置文件，验证完整加载流程

## 完成标准

- [ ] `cargo build` 编译通过
- [ ] `cargo test` 全部通过
- [ ] 能正确解析示例 config.toml 和 tools.d/*.toml
- [ ] tool name 重复检测生效
- [ ] timeout 合并逻辑正确
- [ ] lib.rs 导出 config 模块可被外部 crate 使用
