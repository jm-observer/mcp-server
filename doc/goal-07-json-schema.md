# Goal 07: JSON Schema 输出

## 目标

为 tool 配置文件（`tools.d/*.toml`）生成 JSON Schema，供外部辅助工具（tool 配置生成器）使用，确保生成的 `.toml` 配置合法。

## 前置依赖

- Goal 01（配置结构定义）

## 涉及文件

```
src/
├── config/
│   └── schema.rs        # JSON Schema 生成逻辑
└── main.rs              # 新增 --schema 命令行参数（可选）
```

## 实现方式

### 方案

使用 `schemars` crate 为配置结构体自动生成 JSON Schema。

在 `ToolFile`、`ToolDef`、`ParameterDef` 等结构体上派生 `JsonSchema`：

```rust
use schemars::JsonSchema;

#[derive(Deserialize, JsonSchema)]
pub struct ToolFile {
    pub config: Option<ToolFileConfig>,
    pub tools: Vec<ToolDef>,
}
// ... 其他结构体同理
```

### 输出方式

提供两种获取 schema 的方式：

1. **命令行参数**：`mcp-server --schema` 输出 JSON Schema 到 stdout 后退出
2. **库函数**：`pub fn tool_config_schema() -> String`，辅助工具可通过 crate 依赖直接调用

```rust
pub fn tool_config_schema() -> String {
    let schema = schemars::schema_for!(ToolFile);
    serde_json::to_string_pretty(&schema).unwrap()
}
```

### 命令行参数

使用简单的 `std::env::args` 判断即可，不需要引入 clap：

```rust
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.contains(&"--schema".to_string()) {
        println!("{}", tool_config_schema());
        return;
    }
    // 正常启动 server...
}
```

### 依赖

新增：
- `schemars`：JSON Schema 生成（与 serde 集成）

## 测试

### 单元测试

1. **Schema 生成**
   - 生成的 schema 是合法 JSON
   - 包含 `tools` 数组定义
   - 包含 `config` 对象定义
   - tool 的 `name`、`description` 为 required 字段
   - `type` 字段的 enum 值包含 "command" 和 "http"
   - `parameters` 数组中包含 name、type、description、required

2. **Schema 验证**
   - 用生成的 schema 验证示例 tool 配置（git.toml、http_test.toml）→ 通过
   - 用生成的 schema 验证缺少必填字段的配置 → 失败

### 集成测试

- `mcp-server --schema` 输出合法 JSON Schema 并以 0 退出

## 完成标准

- [ ] `schemars` 派生正确，生成的 schema 反映配置结构
- [ ] `mcp-server --schema` 输出 JSON Schema 到 stdout
- [ ] 库函数 `tool_config_schema()` 可被外部 crate 调用
- [ ] 示例配置通过 schema 验证
- [ ] 所有测试通过
