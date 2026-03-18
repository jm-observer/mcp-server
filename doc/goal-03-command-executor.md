# Goal 03: Command 执行器与安全校验

## 目标

实现 command 类型 tool 的执行逻辑，包括参数模板填充、目录校验、进程执行与超时控制，并将其接入 `tools/call` 处理流程。

## 前置依赖

- Goal 01（配置解析、ToolRegistry）
- Goal 02（MCP 协议层、handler 骨架）

## 涉及文件

```
src/
├── lib.rs               # 新增导出 executor、security 模块
├── executor/
│   ├── mod.rs           # Executor trait/enum 定义
│   └── command.rs       # Command 执行器实现
├── security.rs          # 目录白名单校验、路径逃逸检测
└── protocol/
    └── handler.rs       # 接入 command executor（修改 handle_tools_call）
```

## 实现方式

### 安全模块（security.rs）

```rust
/// 检查 path 是否在 allowed_dirs 白名单内
pub fn validate_working_dir(path: &Path, allowed_dirs: &[PathBuf]) -> Result<()>;

/// 检查 sub_dir 解析后的路径是否仍在 working_dir 内（防路径逃逸）
pub fn validate_sub_dir(working_dir: &Path, sub_dir: &str) -> Result<PathBuf>;
```

实现要点：
- **强烈抵制使用 `std::fs::canonicalize`**：因为该原生库方法如果目标目录未创建就会抛系统级硬报错（例如 Git clone 会预建目录情况），且在 Windows 下还会导致 UNC 路径污染(`\\\\?\\`)阻断常见 CLI 程序。
- 应替换为 **纯词法层面路径规范化库 (Lexical Path Normalization，如 `path-clean` 等 crate)**
- `validate_working_dir`：使用词法规范化将相对路径整理后，去匹配在是否在 `allowed_dirs` 内
- `validate_sub_dir`：拼接 `working_dir/sub_dir` 后一样走词法规范化，比对前缀确保仍位于挂载路径即可消除 `../../` 上越界逃逸攻击。

### 参数模板填充

```rust
/// 将 "${var}" 占位符替换为实际参数值
pub fn resolve_template(template: &str, args: &HashMap<String, Value>) -> Result<String>;

/// 对 args 列表中的每个元素执行模板填充
pub fn resolve_args(
    templates: &[String],
    args: &HashMap<String, Value>,
) -> Result<Vec<String>>;
```

- 匹配 `${name}` 模式，从 args 中查找对应值
- 值类型为 string 直接替换，number/boolean 转字符串
- **可选参数缺失应对处理**：如果在 `args` 数组中遇到了独立占据整个元素的 `"${optional_var}"` 而传入的参数里未填选项时，应当**从该 args 数组里剔除这一整项**，实现可选的动态过滤机制；对于内嵌变量则使用空字符串处理替换。

### Command 执行器（command.rs）

```rust
pub struct CommandExecutor {
    allowed_dirs: Vec<PathBuf>,
}

pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl CommandExecutor {
    pub async fn execute(
        &self,
        tool: &RegisteredTool,
        arguments: &HashMap<String, Value>,
    ) -> Result<CommandResult>;
}
```

执行流程：
1. 解析参数模板：填充 `command`、`args`、`sub_dir` 中的 `${var}`
2. 确定工作目录：
   - 基础目录 = `tool.working_dir`（来自 [config].working_dir）
   - 如有 `sub_dir`，拼接并调用 `validate_sub_dir` 校验
   - 调用 `validate_working_dir` 校验白名单
3. 执行命令：使用 `tokio::process::Command::new(command).args(args)` (**严格禁止将带有变量的内容交给 `sh -c` 进行字符串拼接并抛诸于 Shell ，防止一切脚本化注入**)
   - 设置 `current_dir` 并应用配置传入的环境变量： `.envs(&tool.env)`
   - **防 OOM 限流与输出截断**：对于 `stdout` 以及 `stderr`，不要进行原始的 `child.wait_with_output()` （防止恶意死循环输出无尽流搞垮内存），改为挂起受限的异步 `Reader` 流只保留规定界限以内的部分 (如每流最多 50KB 或者尾百行记录）。
   - 设置超时：`tokio::time::timeout(Duration::from_secs(effective_timeout), ...)`
4. 收集结果：防爆截取后的短日志 stdout、stderr（UTF-8 lossy），执行真实 exit_code

### 接入 handler

修改 `handle_tools_call`：
- 判断 tool type 为 Command 时调用 CommandExecutor
- 将 `CommandResult` 转为 MCP `ToolCallResult`：
  - stdout 和 stderr 各作为一个 `ContentBlock`（type: "text"）
  - 非零 exit_code 时设置 `is_error: true`
  - 如果命令找不到或是遭遇到 Timeout 处决等异步执行直接崩溃的问题，此时不要直接抛错让接口不可用，将其同样视作普通的带友好消息的 ToolCallResult(`is_error: true` 和相应纯文报错原因返回)，协助 AI 获取异常。

### lib.rs 导出

```rust
pub mod config;
pub mod protocol;
pub mod executor;
pub mod security;
```

## 测试

### 单元测试

1. **安全校验**
   - `validate_working_dir`：路径在白名单内 → 通过
   - `validate_working_dir`：路径不在白名单内 → 拒绝
   - `validate_sub_dir`：正常子目录 → 通过，返回正确路径
   - `validate_sub_dir`：`../../etc` → 路径逃逸，拒绝
   - `validate_sub_dir`：`./normal/../normal` → 解析后仍在范围内，通过

2. **模板填充**
   - `${repo_url}` 替换为字符串值
   - 多个占位符同时替换
   - 不存在的变量报错
   - 无占位符的字符串原样返回

3. **Command 执行**
   - 执行简单命令（如 `echo hello`）并检查 stdout
   - 命令失败时 exit_code 非零
   - stderr 正确捕获并符合容量阀值要求
   - 工作目录设定与**环境变量**重置行为生效成功
   - 能够有效截断超标的 `stdout` 及 `stderr` 以遏制 OOM
   - 超时后能转化返回可分析的常规异常响应

### 集成测试

- 构造完整 tool 配置 → 调用 `handle_tools_call` → 验证端到端结果
- 使用临时目录作为 allowed_dirs 和 working_dir

## 完成标准

- [ ] 参数模板 `${var}` 替换正确
- [ ] 目录白名单校验生效
- [ ] 路径逃逸检测生效（`../` 攻击被拦截）
- [ ] 命令执行成功并返回 stdout/stderr/exit_code
- [ ] 超时控制生效
- [ ] tools/call 对 command 类型 tool 端到端可用
- [ ] 所有测试通过
