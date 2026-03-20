# Goal 14: 工作区暴露与内置文件操作 Tool

## 背景

当前 server 的 `allowed_dirs` 仅作为内部安全边界存在，client（LLM）无法感知可用的目录结构。同时，所有 tool 的工作目录（`working_dir`）硬编码在 tool 配置中，`sub_dir` 作为模板拼接相对路径。这导致两个问题：

1. **LLM 无法自主发现项目**：不知道有哪些可用目录，只能靠 tool 配置里硬编码的路径
2. **缺少基础文件操作能力**：读写文件、列出目录等基础操作需要依赖外部命令包装，不够可靠

## 设计决策与依据

### 决策 1：向 LLM 暴露真实路径

**结论**：直接暴露 `allowed_dirs` 下的真实绝对路径，不做 hash 或混淆。

**依据**：

- 命令输出（编译错误、git status 等）本身包含真实路径，混淆路径会导致 LLM 在理解输出时产生混乱
- `read_file` 返回的内容中也可能引用真实路径，无法全部替换
- `allowed_dirs` 白名单已经是安全边界，暴露路径不等于放弃安全控制——LLM 知道路径但只能在白名单内操作
- 安全隔离应在网络层面实现（如仅监听 localhost），而非在路径层面做混淆
- 调试时真实路径更易排查问题

### 决策 2：项目类型由用户在对话中告知 LLM

**结论**：Server 不做项目类型探测或标注，只提供目录列表。

**依据**：

- LLM 通过 `list_dir` 获取目录结构后，用户在对话中告诉 LLM "mcp-server 是 Rust 项目" 等对应关系
- 语义理解交给 LLM + 用户对话上下文，server 保持简单
- 避免 server 端维护项目类型识别逻辑（标志文件匹配、多类型歧义等复杂度）

### 决策 3：去掉 `working_dir` + `sub_dir`，统一为 `cwd` 字段

**结论**：用 tool 级别的 `cwd` 模板字段替代原有的 `[config].working_dir` 和 `sub_dir` 两层设计。

**依据**：

- 既然 LLM 已经能看到完整目录结构并传入路径，就不需要把 base 目录硬编码在 server 端
- `working_dir`（静态 base）+ `sub_dir`（动态子路径）拆成两层是因为之前不暴露路径给 client，现在这个前提已变
- 合并为一个 `cwd` 字段更简洁，语义更清晰

### 决策 4：内置最小集文件操作 Tool

**结论**：Server 内置 `list_dir`、`read_file`、`write_file` 三个 tool。

**依据**：

- 文件操作是基础能力，不适合通过外部命令 toml 包装
- `list_dir` 同时解决了"暴露目录列表给 LLM"的需求
- 最小集先行，后续按需扩展（delete、move 等）

## 目标

1. LLM 能通过内置 tool 发现和浏览 `allowed_dirs` 下的目录与文件
2. LLM 能读写 `allowed_dirs` 范围内的文件
3. 外部 tool（如 cargo_build）的工作目录由 LLM 动态传入，通过 `cwd` 模板解析
4. 所有路径操作统一受 `allowed_dirs` 白名单约束

## 实现方式

### 1. 内置 Tool 定义

内置 tool 不通过 `tools.d/*.toml` 定义，而是在 server 代码中直接注册。

#### list_dir

```
名称: list_dir
参数:
  - path: string, required — 目标目录的绝对路径
描述: 列出指定目录下的文件和子目录
返回: 文件/目录名称列表，标注类型（file/dir）
约束: path 必须在 allowed_dirs 白名单内
```

#### read_file

```
名称: read_file
参数:
  - path: string, required — 文件的绝对路径
描述: 读取指定文件的内容
返回: 文件文本内容
约束: path 必须在 allowed_dirs 白名单内
```

#### write_file

```
名称: write_file
参数:
  - path: string, required — 文件的绝对路径
  - content: string, required — 要写入的内容
描述: 将内容写入指定文件（覆盖写入，文件不存在则创建）
返回: 写入成功/失败状态
约束: path 必须在 allowed_dirs 白名单内
```

### 2. Tool 配置变更

#### 移除 `[config].working_dir` 和 `sub_dir`

旧格式：

```toml
[config]
working_dir = "D:\\git"

[[tools]]
name = "cargo_build"
command = "cargo"
args = ["build"]
sub_dir = "${project}"
```

#### 新增 `cwd` 字段

新格式：

```toml
[[tools]]
name = "cargo_build"
description = "Compile a local package"
type = "command"
command = "cargo"
args = ["build"]
cwd = "${project}"

[[tools.parameters]]
name = "project"
description = "Project directory (absolute path)"
type = "string"
required = true
```

- `cwd` 为 tool 级别字段，支持模板变量
- Server 解析 `cwd` 模板后，对结果做 `allowed_dirs` 校验
- `cwd` 省略时，默认使用 server 进程的当前工作目录（仍需通过 `allowed_dirs` 校验）

### 3. Server 端路径校验流程

所有路径操作（内置 tool 和外部 tool 的 `cwd`）共用同一校验逻辑：

```
1. 接收路径（绝对路径）
2. 规范化路径（解析 `.`、`..`、符号链接等）
3. 检查规范化后的路径是否为某个 allowed_dirs 条目的子路径
4. 校验通过则继续执行，否则返回错误
```

### 4. config.toml 变更

`config.toml` 中 `allowed_dirs` 保持不变，仍作为安全边界：

```toml
[defaults]
allowed_dirs = ["D:\\git"]
```

移除 tool toml 中的 `[config]` 段（`working_dir` 不再需要）。

## 测试方式

### 单元测试

1. **list_dir 测试**
   - 传入 `allowed_dirs` 内的路径 → 返回目录列表
   - 传入 `allowed_dirs` 外的路径 → 返回安全错误
   - 传入不存在的路径 → 返回 IO 错误
   - 传入文件路径（非目录） → 返回错误

2. **read_file 测试**
   - 读取 `allowed_dirs` 内的文件 → 返回内容
   - 读取 `allowed_dirs` 外的文件 → 返回安全错误
   - 读取不存在的文件 → 返回 IO 错误
   - 读取二进制文件 → 合理处理（返回错误或 base64）

3. **write_file 测试**
   - 写入 `allowed_dirs` 内的路径 → 写入成功
   - 写入 `allowed_dirs` 外的路径 → 返回安全错误
   - 写入不存在的中间目录 → 返回错误（不自动创建父目录）
   - 覆盖已有文件 → 写入成功

4. **cwd 解析测试**
   - `cwd = "${project}"` 传入合法路径 → 正确设置工作目录
   - `cwd = "${project}"` 传入 `allowed_dirs` 外的路径 → 返回安全错误
   - `cwd` 省略 → 使用默认工作目录
   - `cwd` 模板变量未传值 → 返回错误

5. **路径穿越测试**
   - 传入含 `..` 的路径试图跳出 `allowed_dirs` → 返回安全错误
   - 传入符号链接指向 `allowed_dirs` 外 → 返回安全错误

### 集成测试

1. **完整流程测试**
   - 启动 server → client 调用 `list_dir` 获取目录列表 → 调用 `read_file` 读取文件 → 验证返回内容正确
   - 启动 server → client 调用 `write_file` 写入文件 → 调用 `read_file` 验证内容 → 清理

2. **外部 tool 与 cwd 联动测试**
   - 配置 `cwd = "${project}"` 的 tool → client 传入合法 project 路径 → 命令在正确目录下执行

## 迁移影响

- `[config].working_dir` 和 `sub_dir` 字段废弃，现有 `tools.d/*.toml` 需要迁移到 `cwd` 写法
- `RegisteredTool` 结构体中的 `working_dir` 字段移除
- `CommandExecutor` 中 `sub_dir` 相关的解析逻辑替换为 `cwd` 解析
- `security.rs` 中 `validate_sub_dir` 可移除，`validate_working_dir` 保留
