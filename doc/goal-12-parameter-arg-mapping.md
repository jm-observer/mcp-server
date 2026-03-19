# Goal 12: Parameter 级 arg 映射 — 可选参数的声明式 CLI 拼接

## 设计初衷

### 问题

当前的参数模板机制将固定参数和可选参数混在同一个 `args` 数组中：

```toml
args = ["build", "--bin", "${bin}", "-p", "${package}", "--features", "${features}"]
```

`resolve_args` 的跳过逻辑**仅能跳过独占整个元素的 `${var}`**，无法感知 flag 与 value 的配对关系。当 client 不传 `bin` 参数时：

- `"--bin"` — 纯字符串，原样保留（**残留**）
- `"${bin}"` — 未提供，被跳过

最终生成 `cargo build --bin -p b --features f1,f2`，`--bin` 后面缺少值，命令报错。

### 核心矛盾

`args` 数组承担了两个职责：

1. **固定参数**：如 `["build"]`，始终存在
2. **可选参数模板**：如 `["--bin", "${bin}"]`，需要整体出现或整体跳过

这两者有本质区别——固定参数无条件拼接，可选参数需要根据 client 是否传值来决定是否参与拼接。将它们混在同一个数组中无法区分。

### 解决思路

将职责分离：

- `args` **只保留固定参数**，如 `["build"]`
- 每个 `ParameterDef` 新增 `arg` 字段，声明该参数对应的 CLI 片段
- executor 根据 client 实际传入的参数，将各 parameter 的 `arg` 拼接到固定 `args` 之后

## 设计

### ParameterDef 新增 `arg` 字段

```rust
#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct ParameterDef {
    pub name: String,
    pub description: String,
    pub r#type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub arg: Option<Vec<String>>,  // 新增
}
```

`arg` 是一个字符串数组，描述该参数在 CLI 中的表现形式：

| 参数类型 | arg 定义 | client 传值 | 拼接结果 |
|---|---|---|---|
| string | `["--bin", "${bin}"]` | `"bin": "a"` | `--bin a` |
| string | `["-p", "${package}"]` | `"package": "b"` | `-p b` |
| string | `["--features", "${features}"]` | `"features": "f1,f2"` | `--features f1,f2` |
| boolean | `["--release"]` | `"release": true` | `--release` |
| boolean | `["--release"]` | `"release": false` 或不传 | *（跳过）* |
| 无 arg | `null` / 不设置 | 任意 | *（不参与 CLI 拼接，可用于 `sub_dir` 等 executor 级用途）* |

### 无 arg 参数的用途：`sub_dir` 工作目录

并非所有参数都是给命令行的。典型场景是**项目目录参数**——它不应该出现在 CLI 参数中，而是通过 `sub_dir` 模板影响命令的工作目录。

以 `cargo build` 为例，它需要在具体的项目目录下执行。`ToolAction::Command` 中已有 `sub_dir` 字段，结合 `[config].working_dir` 可以实现"先进入项目目录，再执行命令"：

```
working_dir（项目根目录的父目录）
  └── sub_dir = "${project}"（具体项目名，由 client 传入）
        └── 在此目录下执行 cargo build
```

executor 的处理流程（`command.rs`）：

```rust
let mut work_dir = tool.working_dir.clone()...;  // 来自 [config].working_dir

if let Some(sub_tpl) = sub_dir_opt {
    let sub_res = Self::resolve_template(sub_tpl, arguments)?;  // "${project}" → "mcp-server"
    work_dir = validate_sub_dir(&work_dir, &sub_res)?;          // 安全校验，防路径逃逸
}
// child_cmd.current_dir(work_dir)  → 最终在 /data/repos/mcp-server 下执行
```

此时 `project` 参数的 `arg` 为 `None`（不设置），它**不会**出现在 CLI 参数中，仅通过 `sub_dir` 模板影响工作目录。这正是 `arg` 设计为 `Option` 的原因之一——参数的作用域可以是 CLI 参数，也可以是 executor 的路径逻辑。

### 拼接规则

executor 在构建最终命令时：

1. **固定部分**：先对 `args` 做原有的 `resolve_args` 模板替换，得到基础参数列表
2. **动态部分**：遍历 tool 的 `parameters`，对于 client 提供了值的参数：
   - 若 `arg` 为 `None`：跳过，该参数不影响 CLI（纯元数据）
   - 若 `type == "boolean"`：值为 `true` 时将 `arg` 数组原样追加；`false` 或不传则跳过
   - 其他类型：对 `arg` 中每个元素做 `resolve_template` 替换后追加
3. **合并**：固定部分 + 动态部分 = 最终参数列表

### TOML 配置示例

以在项目 `mcp-server` 下执行 `cargo build --bin a -p b --features f1,f2 --release` 为例：

```toml
[config]
working_dir = "/data/repos"    # 项目根目录的父目录（需在 allowed_dirs 白名单内）

[[tools]]
name = "cargo_build"
description = "Compile a local package and all of its dependencies"
type = "command"
command = "cargo"
args = ["build"]
sub_dir = "${project}"         # 进入具体项目目录后再执行

[[tools.parameters]]
name = "project"
description = "Project directory name under working_dir"
type = "string"
required = true
# 注意：此参数没有 arg 字段，因为它不拼接到 CLI 参数里，
# 而是通过 sub_dir 模板影响工作目录

[[tools.parameters]]
name = "package"
description = "Package to build (see `cargo help pkgid`)"
type = "string"
required = false
arg = ["-p", "${package}"]

[[tools.parameters]]
name = "bin"
description = "Build only the specified binary"
type = "string"
required = false
arg = ["--bin", "${bin}"]

[[tools.parameters]]
name = "features"
description = "Comma separated list of features to activate"
type = "string"
required = false
arg = ["--features", "${features}"]

[[tools.parameters]]
name = "release"
description = "Build artifacts in release mode, with optimizations"
type = "boolean"
required = false
arg = ["--release"]

[[tools.parameters]]
name = "target"
description = "Build for the target triple"
type = "string"
required = false
arg = ["--target", "${target}"]

[[tools.parameters]]
name = "jobs"
description = "Number of parallel jobs, defaults to # of CPUs"
type = "string"
required = false
arg = ["-j", "${jobs}"]

[[tools.parameters]]
name = "all_features"
description = "Activate all available features"
type = "boolean"
required = false
arg = ["--all-features"]

[[tools.parameters]]
name = "no_default_features"
description = "Do not activate the default feature"
type = "boolean"
required = false
arg = ["--no-default-features"]

[[tools.parameters]]
name = "workspace"
description = "Build all packages in the workspace"
type = "boolean"
required = false
arg = ["--workspace"]

[[tools.parameters]]
name = "verbose"
description = "Use verbose output (-v)"
type = "boolean"
required = false
arg = ["-v"]

[[tools.parameters]]
name = "quiet"
description = "Do not print cargo log messages"
type = "boolean"
required = false
arg = ["-q"]

[[tools.parameters]]
name = "manifest_path"
description = "Path to Cargo.toml"
type = "string"
required = false
arg = ["--manifest-path", "${manifest_path}"]

[[tools.parameters]]
name = "profile"
description = "Build artifacts with the specified profile"
type = "string"
required = false
arg = ["--profile", "${profile}"]
```

### MCP Client 调用示例

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "cargo_build",
    "arguments": {
      "project": "mcp-server",
      "bin": "a",
      "package": "b",
      "features": "f1,f2",
      "release": true
    }
  }
}
```

### 执行链路

```
Step 1: 确定工作目录
  working_dir = /data/repos
  sub_dir = "${project}" → resolve → "mcp-server"
  validate_sub_dir → /data/repos/mcp-server（校验通过，无路径逃逸）
  → cd /data/repos/mcp-server

Step 2: 固定 args
  args = ["build"]

Step 3: 遍历 parameters，拼接动态参数

  parameter "project" → arg = None
    → 跳过（已通过 sub_dir 消费，不参与 CLI 拼接）

  parameter "package" → client 传了 "b"
    arg = ["-p", "${package}"]  → resolve → ["-p", "b"]

  parameter "bin" → client 传了 "a"
    arg = ["--bin", "${bin}"]   → resolve → ["--bin", "a"]

  parameter "features" → client 传了 "f1,f2"
    arg = ["--features", "${features}"] → resolve → ["--features", "f1,f2"]

  parameter "release" → client 传了 true (boolean)
    arg = ["--release"]         → 原样追加 → ["--release"]

  parameter "target" → client 未传
    → 跳过

Step 4: 合并执行
  最终: (cd /data/repos/mcp-server) cargo build -p b --bin a --features f1,f2 --release
```

## 实现方式

### 涉及文件

```
src/
├── config/
│   └── tool.rs          # ParameterDef 增加 arg 字段
└── executor/
    └── command.rs       # execute 方法增加动态参数拼接逻辑
tools.d/
└── cargo_build.toml     # 更新为新格式
```

### 1. `src/config/tool.rs`

在 `ParameterDef` 中新增：

```rust
#[serde(default)]
pub arg: Option<Vec<String>>,
```

已有 `Serialize` derive（Goal 12 前置修改中已添加），无需额外改动。

### 2. `src/executor/command.rs`

在 `execute` 方法中，现有的固定 args resolve 之后，插入动态参数拼接逻辑：

```rust
// 现有：resolve 固定 args
let mut resolved_args = Vec::new();
if let Some(t_args) = args_opt {
    resolved_args = Self::resolve_args(t_args, arguments)?;
}

// 新增：遍历 parameters，拼接 arg
if let Some(params) = &tool.def.parameters {
    for param in params {
        if let Some(arg_templates) = &param.arg {
            if let Some(value) = arguments.get(&param.name) {
                // boolean 类型：true 时追加 arg，false 时跳过
                if param.r#type == "boolean" {
                    if value.as_bool().unwrap_or(false) {
                        resolved_args.extend(arg_templates.iter().cloned());
                    }
                    continue;
                }
                // 其他类型：模板替换后追加
                let resolved = Self::resolve_args(arg_templates, arguments)?;
                resolved_args.extend(resolved);
            }
        }
    }
}
```

### 3. `tools.d/cargo_build.toml`

按上文「TOML 配置示例」部分的内容更新，取消注释，使用 `arg` 字段定义每个参数的 CLI 映射。

## 向后兼容

- `arg` 字段为 `Option`，默认 `None`，不影响已有的 tool 配置文件
- 已有的 `args` 内联模板机制（如 `["clone", "${repo_url}"]`）保持不变，二者可共存
- 只有定义了 `arg` 的参数才会参与动态拼接

## 完成标准

- [ ] `ParameterDef` 增加 `arg: Option<Vec<String>>` 字段
- [ ] `CommandExecutor::execute` 支持动态参数拼接
- [ ] `tools.d/cargo_build.toml` 使用新格式定义
- [ ] `cargo build` 编译通过
- [ ] `deserialize_tool_toml` 能正确读取并展示 `arg` 字段
- [ ] 端到端验证：MCP client 传入参数后，executor 生成正确的命令
