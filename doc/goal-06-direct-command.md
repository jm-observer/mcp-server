# Goal 06: 直接命令执行

## 目标

当 `config.toml` 中 `allow_direct_command = true` 时，server 自动注册一个内置 tool，允许执行任意命令。用于辅助工具等受信场景。

## 前置依赖

- Goal 03（Command 执行器）
- Goal 04（HTTP server 集成）

## 涉及文件

```
src/
├── executor/
│   └── command.rs       # 复用已有命令执行逻辑
└── protocol/
    └── handler.rs       # 注册内置 tool，handle_tools_call 处理
```

## 实现方式

### 内置 Tool 定义

当 `allow_direct_command = true` 时，在 ToolRegistry 中注册一个特殊 tool：

```
name: "direct_command"
description: "Execute an arbitrary shell command"
parameters:
  - command: string, required — 要执行的命令
  - args: array of string, optional — 命令参数
  - working_dir: string, optional — 工作目录（仍受 allowed_dirs 白名单约束）
```

### 执行逻辑

与普通 command tool 的区别：
- 命令和参数直接来自请求，不走模板填充
- `working_dir` 由请求指定（可选），不指定时使用某个默认值或不设置
- **仍然受 `allowed_dirs` 白名单约束**：指定的 working_dir 必须在白名单内

```rust
pub async fn execute_direct(
    &self,
    command: &str,
    args: &[String],
    working_dir: Option<&Path>,
) -> Result<CommandResult>;
```

### 注册时机

在 `McpHandler` 或 `ToolRegistry` 构建时：

```rust
if server_config.security.allow_direct_command {
    registry.register_builtin_direct_command();
}
```

### 安全边界

即使开启了直接命令执行，`allowed_dirs` 白名单仍然生效：
- 如果请求提供了 `working_dir`，必须通过白名单校验
- 这确保了即使是直接命令也不能在任意目录操作

## 测试

### 单元测试

1. **开关控制**
   - `allow_direct_command = false` → tools/list 不包含 direct_command
   - `allow_direct_command = true` → tools/list 包含 direct_command

2. **命令执行**
   - 调用 direct_command 执行 `echo hello` → 返回 stdout
   - 指定合法 working_dir → 命令在该目录执行
   - 指定非法 working_dir → 白名单拒绝

3. **参数校验**
   - 缺少 command 参数 → 错误
   - args 为空 → 正常执行无参数命令

### 集成测试

- 端到端：通过 MCP tools/call 调用 direct_command → 返回命令执行结果

## 完成标准

- [ ] `allow_direct_command = true` 时 direct_command tool 自动注册
- [ ] `allow_direct_command = false` 时不暴露该 tool
- [ ] 直接命令执行正确返回 stdout/stderr/exit_code
- [ ] working_dir 白名单校验仍然生效
- [ ] 所有测试通过
