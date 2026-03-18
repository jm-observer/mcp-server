# Goal 10: 辅助工具 - Help 递归解析

## 目标

实现命令 help 文本的递归采集：从顶层命令开始，由 LLM 分析 help 文本识别子命令，逐层递归执行 `--help`，收集所有子命令的 help 文本。

## 前置依赖

- Goal 09（MCP 客户端连接，direct_command 执行）

## 涉及文件

```
mcp-tool-generator/src/
├── llm_client.rs         # vLLM API 客户端（本 Goal 引入，Goal 11 复用）
├── crawler.rs            # 递归爬取逻辑（LLM 驱动）
├── prompt.rs             # prompt 模板（子命令识别 + Goal 11 的 TOML 生成）
└── types.rs              # 数据结构定义
```

## 实现方式

### vLLM 客户端（llm_client.rs）

本 Goal 引入 LLM 客户端，Goal 11 直接复用。

```rust
pub struct LlmClient {
    client: reqwest::Client,
    base_url: String,             // 如 http://localhost:8000
    model: String,
}

pub struct ChatMessage {
    pub role: String,             // "system" / "user" / "assistant"
    pub content: String,
}

impl LlmClient {
    /// 调用 /v1/chat/completions
    pub async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String>;
}
```

### 数据结构（types.rs）

```rust
/// 一个命令/子命令的 help 信息
pub struct CommandHelp {
    pub full_command: Vec<String>,    // 如 ["cargo", "build"]
    pub help_text: String,            // --help 输出的原始文本
    pub children: Vec<CommandHelp>,   // 子命令（递归）
}

/// 扁平化后的结果，用于后续传给 LLM 生成 TOML
pub struct FlatCommand {
    pub full_command: Vec<String>,    // 如 ["cargo", "build"]
    pub help_text: String,
}
```

### LLM 驱动的子命令识别

不手写解析逻辑，由 LLM 分析 help 文本来识别子命令：

```rust
/// 调用 LLM 从 help 文本中识别子命令
pub async fn extract_subcommands(
    llm: &LlmClient,
    command: &str,
    help_text: &str,
) -> Result<Vec<String>>;
```

**Prompt 设计（prompt.rs）：**

```
System:
你是一个命令行工具分析器。根据命令的 --help 输出，识别出所有可用的子命令。

规则：
1. 只返回子命令名称列表，每行一个
2. 不包含选项/标志（如 --verbose, -h）
3. 不包含 "help" 子命令本身
4. 如果没有子命令，返回空（仅输出 "NONE"）
5. 只返回命令名，不要描述

User:
命令: {command}

Help 输出:
{help_text}

请列出所有子命令：
```

**输出解析：**
- LLM 返回纯文本，每行一个子命令名
- 返回 "NONE" 或空时表示无子命令
- 过滤掉明显不是命令名的行（空行、包含空格的行、以 `-` 开头的行）

### 递归爬取（crawler.rs）

```rust
pub struct HelpCrawler {
    mcp_client: McpClient,
    llm_client: LlmClient,
    max_depth: usize,
}

impl HelpCrawler {
    /// 从顶层命令开始递归爬取
    pub async fn crawl(&mut self, command: &str) -> Result<CommandHelp>;

    /// 递归内部实现
    async fn crawl_recursive(
        &mut self,
        command_parts: &[String],
        depth: usize,
    ) -> Result<CommandHelp>;

    /// 将树形结构扁平化
    pub fn flatten(root: &CommandHelp) -> Vec<FlatCommand>;
}
```

递归流程：

1. 执行 `command --help`（通过 MCP direct_command）
2. 将 help 文本发给 LLM，识别子命令列表
3. 如未达最大深度，对每个子命令递归执行 `command subcommand --help`
4. LLM 再次分析子命令的 help，继续递归
5. 构建 `CommandHelp` 树

边界处理：
- 达到 `max_depth` 时停止递归，仅记录当前层 help
- 命令执行失败（exit_code 非零）跳过该子命令，记录警告日志
- 某些命令用 `-h` 而非 `--help`，执行失败时可尝试回退到 `-h`
- LLM 返回异常时（超时、解析失败）视为无子命令，不中断流程

### 输出

`flatten()` 将树形结构转为 `Vec<FlatCommand>`，每个元素包含完整命令路径和 help 文本，供 Goal 11 传给 LLM 生成 TOML。

## 测试

### 单元测试

1. **LLM 客户端**
   - 请求格式符合 OpenAI chat API
   - 正确解析 API 响应
   - 超时/错误处理

2. **子命令提取（LLM 输出解析）**
   - 正常返回多个子命令名 → 解析为列表
   - 返回 "NONE" → 空列表
   - 返回包含杂质行 → 过滤后得到干净列表
   - 空响应 → 空列表

3. **扁平化**
   - 两层嵌套 → 正确扁平化，full_command 路径正确
   - 单层无子命令 → 返回单个元素

### 集成测试

1. **单层爬取**
   - 爬取一个无子命令的工具 → 返回单节点，children 为空

2. **多层爬取**
   - 爬取一个有子命令的工具（如 `git`，depth=1）
   - LLM 识别出子命令列表
   - 验证返回的子命令数量合理
   - 验证 help_text 非空

3. **深度限制**
   - max_depth=0 → 只采集顶层
   - max_depth=1 → 只采集一层子命令

4. **失败容错**
   - 子命令执行失败 → 跳过，不影响其他子命令
   - LLM 调用失败 → 视为无子命令，继续执行

### Mock 测试

- mock LLM 响应，验证爬取逻辑不依赖真实 LLM 也能测试
- 预设不同的 LLM 返回内容，验证各种边界情况

## 完成标准

- [ ] vLLM 客户端正常调用 OpenAI 兼容 API
- [ ] LLM 正确识别 help 文本中的子命令
- [ ] 递归爬取按深度限制正确执行
- [ ] 命令执行失败或 LLM 调用失败时优雅跳过
- [ ] 扁平化输出包含完整命令路径和 help 文本
- [ ] 单元测试、Mock 测试和集成测试通过
