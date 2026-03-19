# Goal 13: LLM 输出 JSON 反序列化生成 TOML 配置

## 目标

重构 tool 配置生成流程：让 LLM 输出符合 `ToolDef` / `ToolFile` 结构约束的 JSON，程序侧完成反序列化、校验、分组，再统一序列化为 TOML。替代当前“LLM 直接输出 TOML + 字符串截取 + 手工拼接”的方式。

这次重构的核心不是“换一种输出格式”，而是把生成链路从**文本生成**切换为**结构生成**：

- LLM 负责语义提取和字段填充
- Rust 代码负责结构校验、错误处理和最终格式化
- 最终 TOML 只作为落盘格式，不再作为 LLM 与程序之间的协议格式

## 动机

- LLM 生成 JSON 的稳定性明显高于 TOML
- `serde_json::from_str` 可以直接做结构化校验，字段缺失、类型错误、枚举 tag 错误都能快速失败
- `toml::to_string_pretty` 可以统一输出格式，避免不同 prompt/模型输出风格造成的噪声
- 当前 `toml_output.rs` 依赖字符串拼接，难以保证长期可维护性
- 当前 `parse_llm_response` 只是“提取文本块”，无法保证 TOML 内容真的满足 `ToolDef` 语义约束
- `ToolDef` / `ToolAction` / `ParameterDef` 已具备 `Serialize`、`Deserialize`、`JsonSchema` 基础设施，重构成本可控

## 前置依赖

- Goal 07（JSON Schema 输出）
- Goal 11（当前 LLM 生成 TOML 的链路）
- Goal 12（`ParameterDef.arg` 已引入，生成器需要与该新结构保持一致）

## 非目标

本 Goal 不处理以下事项，避免范围膨胀：

- 不重新设计 `ParameterDef.arg` 的语义
- 不在本 Goal 内引入多轮 LLM 自动修复
- 不改变 executor 对 tool 配置的消费逻辑
- 不重做 help crawler / subcommand crawler 流程
- 不优化模型选择、采样参数或 prompt 调度策略

如果后续需要“JSON 解析失败后自动二次提示模型修复”，应单独开 Goal。

## 当前问题

### 1. LLM 与程序之间的协议过于脆弱

当前链路要求模型直接产出 TOML，再通过字符串截取提取 `[[tools]]` 块。这会带来几个问题：

- 模型容易输出解释文字、注释、多余标题
- TOML 语法比 JSON 更容易在细节处出错
- 即使 TOML 可解析，也不代表它符合 Rust 侧期望的数据结构

### 2. 生成与落盘职责混杂

当前 `ToolOutput` 同时承担：

- 持有 LLM 原始 TOML 文本
- 持有危险标记
- 为最终 TOML 合并做中间格式

这实际上是在把“文本协议”继续往后传，导致后续阶段仍然围绕字符串处理，而不是结构处理。

### 3. dangerous 的表达不够结构化

当前 dangerous 判断是靠响应文本里搜 `# DANGEROUS` 或关键字。这个信号太弱：

- 模型可能判断成 dangerous 但没输出注释
- 也可能在描述文字里出现 dangerous，导致误判
- 无法作为结构字段参与测试和后续逻辑

## 数据流变更

### 当前流程

```text
help_text
  -> build_toml_generation_prompt(要求 LLM 输出 TOML)
  -> LLM 原始文本
  -> parse_llm_response(字符串截取 TOML 块 + 模糊识别 dangerous)
  -> ToolOutput { toml_block, is_dangerous, command }
  -> generate_toml_file(手工拼接字符串)
  -> .toml 文件
```

### 目标流程

```text
help_text
  -> build_json_generation_prompt(要求 LLM 输出 JSON)
  -> LLM 原始文本
  -> extract_json
  -> serde_json::from_str::<ToolDef>()
  -> ToolOutput { tool_def, command }
  -> 按 dangerous 分组
  -> 组装 ToolFile / 单个 ToolFile
  -> toml::to_string_pretty(...)
  -> .toml 文件
```

## 设计原则

- LLM 只输出一个单独 `ToolDef` JSON 对象，不直接输出 `[config]`
- `[config]` 由程序统一注入，避免模型胡乱生成全局配置
- `dangerous` 只作为中间生成字段存在，不写入最终 TOML
- safe / dangerous 分组由程序决定，不能再依赖 LLM 输出注释文本
- 任何结构解析失败都应快速返回错误，不做静默容错

## 与当前仓库代码的对应关系

当前仓库现状：

- `src/config/tool.rs` 中 `ParameterDef.arg` 已存在，Goal 13 文档必须与 Goal 12 保持一致
- `ToolDef` 目前还没有 `dangerous` 字段
- `ToolFileConfig` 目前没有 `Default` derive，但 `ToolRegistry::register` 已手工构造默认值
- `mcp-tool-generator/src/prompt.rs` 仍是 `build_toml_generation_prompt` + `parse_llm_response`
- `mcp-tool-generator/src/types.rs` 的 `ToolOutput` 仍持有 `toml_block`
- `mcp-tool-generator/src/toml_output.rs` 仍是纯字符串拼接
- `mcp-tool-generator/src/main.rs` 和 `src/bin/generate_tool_toml.rs` 仍走旧链路

因此这次重构涉及的是真实代码替换，不只是文档命名调整。

## 涉及文件

```text
src/
└── config/
    └── tool.rs                     # ToolDef 增加 dangerous；ToolFileConfig 增加 Default

mcp-tool-generator/src/
├── main.rs                         # 旧调用链切换到 JSON -> ToolDef -> TOML
├── prompt.rs                       # prompt 改写；新增 JSON 提取/解析
├── toml_output.rs                  # 从字符串拼接改为结构体序列化
├── types.rs                        # ToolOutput 改持有 ToolDef
└── bin/
    ├── generate_tool_toml.rs       # 样例/调试入口同步改造
    └── extract_subcommand_by_llm.rs
                                     # 注释或示例代码中的旧接口名同步更新，避免误导
```

### 可选波及文件

如果仓库中存在基于 `ToolDef` 的手工构造点，也要同步补字段：

- `mcp-tool-generator/src/bin/extract_tool_action_by_llm.rs`
- 其他所有 `ToolDef { ... }` struct literal

否则新增 `dangerous` 后会直接编译失败。

## 核心设计

### 1. `ToolDef` 增加 `dangerous` 字段

在 [`src/config/tool.rs`](../src/config/tool.rs) 的 `ToolDef` 上新增：

```rust
#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    #[serde(flatten, default)]
    pub action: ToolAction,
    pub env: Option<HashMap<String, String>>,
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub parameters: Option<Vec<ParameterDef>>,
    #[serde(default, skip_serializing)]
    pub dangerous: bool,
}
```

含义：

- `default`：JSON 中缺失时默认 `false`
- `skip_serializing`：写入 TOML 时自动忽略
- 不使用 `skip`：因为仍需要它出现在 JSON Schema 中，让 LLM 看见并填写

### 2. `ToolFileConfig` 增加 `Default`

当前代码里已经存在手工默认值：

```rust
file.config.unwrap_or_else(|| ToolFileConfig {
    working_dir: None,
    timeout_secs: None,
    env: None,
    base_url: None,
});
```

这说明 `Default` 语义是清晰的，可以直接收敛为：

```rust
#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, Default)]
pub struct ToolFileConfig {
    pub working_dir: Option<String>,
    pub timeout_secs: Option<u64>,
    pub env: Option<HashMap<String, String>>,
    pub base_url: Option<String>,
}
```

这样 `toml_output.rs` 在组装 `ToolFile` 时可统一使用 `..Default::default()`。

### 3. Prompt 改为要求输出单个 `ToolDef` JSON

`build_toml_generation_prompt` 重命名为：

```rust
pub fn build_json_generation_prompt(
    command: &FlatCommand,
    json_schema: &str,
) -> Vec<ChatCompletionRequestMessage>;
```

#### System Prompt

```text
你是一个 MCP tool 配置生成器。根据命令的 --help 输出，生成符合给定 JSON Schema 的单个 tool 定义。

规则：
1. 只输出一个 JSON 对象，不要输出 TOML，不要输出解释文字
2. 输出对象必须符合 JSON Schema
3. 该对象表示单个 tool，而不是整个 ToolFile
4. 从 help 文本中提取参数，放入 parameters 数组
5. 固定命令放入 action.command / action.args
6. 可选参数尽量通过 parameters[*].arg 表达，不要把可选 flag/value 直接塞进固定 args
7. 判断命令是否有副作用，并设置 dangerous:
   - false: 查询、列举、读取、预览、校验、dry-run
   - true: 删除、修改、写入、提交、发送、部署、安装、发布
8. 如果 help 文本不足以确定某个字段，保守填写最小可用值，但不能编造不存在的参数
9. 只输出 JSON

JSON Schema:
<schema>
{schema_content}
</schema>
```

#### User Prompt

```text
命令: {full_command}

Help 输出:
{help_text}

请生成该命令对应的单个 tool 定义（JSON）。
```

### 4. JSON 提取与反序列化

`parse_llm_response` 重命名为：

```rust
pub fn parse_json_response(response: &str, command: Vec<String>) -> anyhow::Result<ToolOutput>;
```

内部拆成两个职责更清晰：

```rust
fn extract_json(response: &str) -> anyhow::Result<&str>;

pub fn parse_json_response(response: &str, command: Vec<String>) -> anyhow::Result<ToolOutput> {
    let json_str = extract_json(response)?;
    let tool_def: ToolDef = serde_json::from_str(json_str)?;
    Ok(ToolOutput { tool_def, command })
}
```

#### JSON 提取策略

按优先级处理：

1. 提取 ```` ```json ... ``` ````
2. 尝试把整段响应直接作为 JSON 解析
3. 回退到“第一个 `{` 到最后一个 `}`”的截取

要求：

- 任何失败都返回明确错误
- 不做“提取成功但结构不完整时自动补字段”的容错
- 错误信息里应保留上下文，例如 “missing field `name`” / “unknown variant `cmd`”

### 5. `ToolOutput` 改为持有结构化结果

`mcp-tool-generator/src/types.rs` 中：

```rust
#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub tool_def: ToolDef,
    pub command: Vec<String>,
}
```

移除：

- `toml_block`
- `is_dangerous`

原因：

- danger 信息现在来自 `tool_def.dangerous`
- TOML 不再作为中间态保存

### 6. TOML 输出切到结构序列化

`generate_toml_file` 不再拼接 LLM 返回文本，而是自己组装 `ToolFile`：

```rust
pub fn generate_toml_file(command_name: &str, outputs: &[ToolOutput]) -> String
```

实现思路：

1. 按 `tool_def.dangerous` 分成 safe / dangerous
2. safe tools 组成一个 `ToolFile`
3. dangerous tools 逐个单独序列化，再按行加注释
4. 文件头与 `[config]` 统一由程序生成

示意代码：

```rust
pub fn generate_toml_file(command_name: &str, outputs: &[ToolOutput]) -> String {
    let mut safe_defs = Vec::new();
    let mut dangerous_defs = Vec::new();

    for output in outputs {
        if output.tool_def.dangerous {
            dangerous_defs.push(output.tool_def.clone());
        } else {
            safe_defs.push(output.tool_def.clone());
        }
    }

    let safe_file = ToolFile {
        config: Some(ToolFileConfig {
            working_dir: Some(".".into()),
            ..Default::default()
        }),
        tools: safe_defs,
    };

    let mut out = format!(
        "# Auto-generated tool configuration for: {}\n\
         # Generated by mcp-tool-generator\n\n",
        command_name
    );

    out.push_str(&toml::to_string_pretty(&safe_file).unwrap());

    if !dangerous_defs.is_empty() {
        out.push_str("\n\n# --- dangerous tools (uncomment to enable) ---\n\n");
        for def in dangerous_defs {
            let single = ToolFile {
                config: None,
                tools: vec![def],
            };
            let toml_str = toml::to_string_pretty(&single).unwrap();
            for line in toml_str.lines() {
                out.push_str("# ");
                out.push_str(line);
                out.push('\n');
            }
            out.push('\n');
        }
    }

    out
}
```

### 7. dangerous 注释块的处理约束

需要明确一个细节：dangerous 工具块应只注释 `[[tools]]` 片段，不应重复注释 `[config]`。

因此建议：

- safe 部分：一个完整 `ToolFile { config: Some(...), tools: safe_defs }`
- dangerous 部分：`ToolFile { config: None, tools: vec![single] }`
- 若 `toml::to_string_pretty` 在 `config: None` 时仍输出多余结构，则应显式过滤，仅保留 `[[tools]]` 段

这是一个需要实际验证的点。

## 分阶段实施

### Phase 1: 结构定义切换

目标：先让核心类型可承载新协议。

改动：

- `ToolDef` 增加 `dangerous`
- `ToolFileConfig` derive `Default`
- 所有 `ToolDef { ... }` 初始化点补 `dangerous: false`

完成标准：

- 项目可编译
- JSON Schema 中能看到 `dangerous`
- TOML 序列化时不输出 `dangerous`

### Phase 2: prompt 与解析链路切换

目标：LLM 输出从 TOML 改为 JSON。

改动：

- `build_toml_generation_prompt` -> `build_json_generation_prompt`
- `parse_llm_response` -> `extract_json` + `parse_json_response`
- `ToolOutput` 改持有 `ToolDef`
- `main.rs` / `generate_tool_toml.rs` 切换调用链

完成标准：

- 对单个 help 文本，能从 LLM 响应中成功拿到 `ToolDef`
- 非法 JSON / 不符合 schema 的 JSON 会显式报错

### Phase 3: TOML 输出切换

目标：彻底移除字符串拼接 TOML 的核心路径。

改动：

- `toml_output.rs` 使用 `ToolFile` + `toml::to_string_pretty`
- safe / dangerous 分组改读 `tool_def.dangerous`
- 删除旧 `toml_block` 依赖

完成标准：

- safe 工具输出为正常 TOML
- dangerous 工具输出为整块注释
- 结果文件能被 `toml::from_str::<ToolFile>()` 验证

### Phase 4: 清理与补测

目标：移除旧概念，防止后续维护混淆。

改动：

- 删除/替换注释中对 “LLM 输出 TOML” 的旧说法
- 调整 bin 示例与调试入口
- 增补单元测试和集成测试

完成标准：

- 仓库内不再残留旧接口名的主要调用
- 测试覆盖新 JSON 解析和 TOML 序列化路径

## 兼容性与迁移影响

### 对 mcp-server 的影响

运行时消费的是最终 TOML 文件，不是 LLM 原始输出，因此：

- 对 `mcp-server` executor 基本无行为变化
- 主要变化发生在 `mcp-tool-generator`

### 对已有 TOML 文件的影响

- 旧 TOML 文件无需迁移
- 新生成的 TOML 只是格式更稳定，不应破坏现有解析

### 对 schema 的影响

因为 `dangerous` 会进入 JSON Schema：

- LLM 看得到该字段
- 如果后续外部工具也使用同一 schema，需要接受它是“生成阶段字段”
- 但由于最终 TOML 不会序列化该字段，运行态配置仍保持干净

## 风险与兜底

### 风险 1: `toml` 对 flatten enum 的序列化表现不符合预期

`ToolAction` 使用：

```rust
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolAction { ... }
```

而 `ToolDef` 对其使用 `#[serde(flatten)]`。这需要重点验证输出 TOML 是否符合预期，例如：

```toml
type = "command"
command = "cargo"
args = ["build"]
```

如果不符合预期，备选方案：

- 升级 `toml` crate
- 为输出层引入专用中间结构，而不是直接序列化 `ToolDef`

### 风险 2: LLM 可能输出 `ToolFile` 而不是 `ToolDef`

这是 prompt 约束问题，但不能完全依赖 prompt。

兜底策略：

- `parse_json_response` 只接受 `ToolDef`
- 失败时直接报错
- 后续若高频出现，可再考虑额外兼容 `{"tools":[...]}` 形式，但不建议作为首版范围

### 风险 3: `dangerous` 判断可能不稳定

结构化后只能保证“有字段”，不能保证“判断正确”。

因此测试与人工验证中需要重点看：

- `rm` / `delete` / `clean` / `publish` / `install` 是否能稳定标成 `true`
- `list` / `show` / `get` / `status` 是否能稳定标成 `false`

### 风险 4: 空 safe 集合的输出格式

若一个命令组里所有工具都被标记为 dangerous，需要明确最终输出行为：

- 是否仍输出 `[config]`
- safe 部分是否允许空 `tools = []`
- dangerous 注释区是否仍是唯一内容

建议行为：

- 保留文件头与 `[config]`
- safe `tools` 为空也可以接受，只要 TOML 合法

## 错误处理策略

首版建议保持严格失败：

- JSON 提取失败：返回错误
- JSON 反序列化失败：返回错误
- TOML 序列化失败：返回错误

不要做这些隐式修复：

- 自动补缺失字段
- 自动把 `dangerous: "true"` 转成 `bool`
- 自动猜测 `ToolFile` 和 `ToolDef`

因为一旦做了隐式修复，后续调试 prompt 和模型行为会变得困难。

## 测试

### 单元测试

1. JSON 提取
   - 从 ```` ```json ``` ```` 代码块提取
   - 从裸 JSON 文本提取
   - 含前后解释文字时能提取首个对象
   - 无对象时返回错误

2. ToolDef 反序列化
   - 合法 JSON -> `ToolDef`
   - `dangerous = true` 正确解析
   - 缺少 `dangerous` 时默认 `false`
   - 缺少必填字段时报错
   - `type = "command" | "http"` 正确反序列化

3. TOML 序列化
   - `ToolFile` 能生成合法 TOML
   - `dangerous` 不出现在 TOML 输出
   - `ToolAction` flatten 后字段位置符合预期
   - `parameters[*].arg` 能正常出现在 TOML 中

4. TOML 合并输出
   - safe tools 正常输出
   - dangerous tools 被整块注释
   - dangerous 区不会重复输出 `[config]`
   - 全 dangerous / 全 safe / safe+dangerous 混合三种情况都覆盖

### 集成测试

1. 端到端 mock
   - 输入 help 文本
   - mock LLM 返回 JSON
   - 反序列化 -> 生成 TOML
   - 最终 `toml::from_str::<ToolFile>()` 可解析

2. 真链路验证
   - 选一个只读命令，例如 `cargo metadata` / `git status`
   - 选一个有副作用命令，例如 `cargo clean` / `git push`
   - 检查 dangerous 分类是否符合预期

### 回归测试

需要覆盖 Goal 12 引入的 `arg`：

- 生成器输出的参数定义优先落在 `parameters[*].arg`
- 固定 `args` 不应重新退化成“flag/value 混在固定数组”

## 验收步骤

建议按这个顺序验收：

1. `cargo test` 至少覆盖 prompt / parse / toml_output 三块
2. 运行 `mcp-tool-generator/src/bin/generate_tool_toml.rs`
3. 检查 LLM 原始输出是否为 JSON
4. 检查生成的 `.toml` 中是否不存在 `dangerous = ...`
5. 用 `deserialize_tool_toml` 或 `toml::from_str::<ToolFile>()` 回读验证
6. 用 mcp-server 真实加载该 `.toml` 验证运行态兼容性

## 完成标准

- [ ] `ToolDef` 增加 `dangerous: bool` 字段（`default` + `skip_serializing`）
- [ ] `ToolFileConfig` 实现 `Default`
- [ ] 所有 `ToolDef { ... }` 初始化点补上 `dangerous`
- [ ] prompt 改为要求 LLM 输出单个 `ToolDef` JSON
- [ ] `parse_json_response` 能从 LLM 响应中提取 JSON 并反序列化为 `ToolDef`
- [ ] `ToolOutput` 改为持有 `ToolDef` 而不是 TOML 字符串
- [ ] `generate_toml_file` 使用 `toml::to_string_pretty` 生成最终输出
- [ ] dangerous 工具的判断来源是 `tool_def.dangerous`，不再依赖文本关键字
- [ ] dangerous 工具在输出中被整块注释，且不重复输出 `[config]`
- [ ] `toml` 对 flatten enum 的序列化结果验证通过
- [ ] `parameters[*].arg` 在新链路下保持兼容
- [ ] 单元测试通过
- [ ] 端到端生成的 `.toml` 能被 mcp-server 配置解析器正确加载

## 建议提交顺序

如果拆 commit，建议这样分：

1. 类型定义与 schema 变更
2. prompt / parse JSON 变更
3. TOML 序列化输出变更
4. 测试与示例入口清理

这样每一步都容易 review，也方便在中间状态定位问题。
