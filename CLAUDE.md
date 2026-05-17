# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

MCP Server is a **Model Context Protocol (MCP) implementation in Rust** that dynamically registers tools via TOML configuration files. It supports both local command execution and HTTP API calls, with dual transport options (SSE and stdio). The server integrates with Claude Desktop and other MCP clients to expose configurable tools.

**Key Repository:** https://github.com/jm-observer/mcp-server

## Build and Development Commands

### Build
- `cargo build --release` - Build release binary
- `cargo build --workspace --release --features prod` - Build with prod features

### Run
The CLI uses `--cwd` to specify the **directory** containing `config.toml` and `tools.d/` (defaults to `~/.config/mcp`):

- **SSE Mode (default):** `./target/release/mcp --cwd /path/to/config/dir`
  - Listens on `host:sse_port` (SSE at `/sse`, `/message`) and `host:http_port` (HTTP RPC at `/rpc`)
  - Default ports: SSE 3000, HTTP 3001
- **Stdio Mode:** `./target/release/mcp --cwd /path/to/config/dir --stdio` - For Claude Desktop
- **Schema Export:** `./target/release/mcp --schema` - Print tool config JSON Schema (no --cwd needed)

### Code Quality (all must pass before committing)
- `cargo clippy -- -D warnings` - Linting
- `cargo fmt --check` - Formatting
- `cargo test` - Testing

### Code Style
- **Max width:** 120 columns (rustfmt.toml)
- **Indentation:** 4 spaces
- **Edition:** 2024
- No `.unwrap()` or `.expect()` in lib code; use `?` with `anyhow::Result`
- No `println!` for logs; use `log!`, `info!`, `error!` macros

## Architecture Overview

The server uses a modular architecture with clear separation of concerns:

### Core Modules (src/)

1. **config/** - Configuration loading and tool registry
   - `server.rs`: Parses config.toml (server host/port, security, directory whitelist)
   - `tool.rs`: Defines ToolRegistry, ToolDef, and ToolAction (Command vs HTTP)
   - `prompt.rs`: Manages reusable prompt templates from prompts.d/
   - `schema.rs`: Generates JSON Schema for tool configurations

2. **protocol/** - MCP protocol implementation
   - `handler.rs`: Core McpHandler that dispatches MCP requests
   - `types.rs`: JSON-RPC types (JsonRpcRequest, JsonRpcResponse, JsonRpcError)

3. **executor/** - Tool execution engines
   - `command.rs`: Executes local shell commands with ${var} template resolution
   - `http.rs`: Executes HTTP requests (GET/POST) with parameter templating

4. **transport/** - Network protocols
   - `sse.rs`: Server-Sent Events with session management (90s TTL, auto-cleanup)
   - `stdio.rs`: Stdio-based transport with LSP framing
   - `http.rs`: HTTP /rpc endpoint for direct JSON-RPC calls

5. **client/** - Test/example clients
   - `sse_client.rs`: Connects to SSE endpoint and maintains session

### Tool Configuration System

**Static registration via TOML files (tools.d/ directory):**
- Each .toml file contains [config] section and [[tools]] array definitions
- Parameters use ${var} placeholders in command args
- Missing optional parameters auto-removed from args

**Three tool categories:**
1. Builtin file tools: list_dir, read_file, write_file, list_allowed_dirs
2. Direct command tool: direct_command (optional, gated by allow_direct_command)
3. Custom tools: defined in tools.d/*.toml files

### Configuration Merging

- Timeout: minimum of tool/file/global levels (global is ceiling)
- Environment variables: file-level merged with tool-level (tool takes precedence)
- Working directory: allowed_dirs is security boundary; working_dir is group default

### Security Features

- Directory whitelisting (executions confined to allowed_dirs)
- Path escape prevention (resolved paths stay within working_dir)
- Output truncation (50KB limit on stdout/stderr)
- Process injection protection (native argument passing, no shell concatenation)
- Direct command execution gated by allow_direct_command

## Project Structure

```
mcp-server/
├── Cargo.toml                  # Workspace with main crate + mcp-tool-generator
├── config.toml                 # Server configuration
├── tools.d/                    # Tool definition directory
├── src/
│   ├── main.rs                 # Entry point and startup logic
│   ├── lib.rs                  # Module exports
│   ├── config/                 # Config parsing and registries
│   ├── protocol/               # MCP protocol handling
│   ├── executor/               # Command and HTTP execution
│   ├── transport/              # SSE, stdio, HTTP transports
│   └── client/                 # Test client implementations
├── mcp-tool-generator/         # Companion tool for auto-generating configs
├── doc/                        # Development documentation (18 goal documents)
└── Makefile.toml, rustfmt.toml, clippy.toml
```

## Key Data Flow

### Tool Call Execution
1. Transport receives request (SSE/stdio/HTTP) → JSON-RPC deserialized
2. McpHandler::handle_request dispatches by method name
3. handle_tools_call validates tool and parameters
4. CommandExecutor or HttpExecutor executes with resolved templates
5. Output returned as JSON result or error

### Startup (main.rs)
1. Parse CLI args (--config, --stdio, --schema)
2. Load config.toml and server configuration
3. Initialize logger via custom_utils::logger::logger_feature
4. Create ToolRegistry and register:
   - Builtin file tools
   - Builtin direct_command tool (if enabled)
   - Custom tools from tools.d/ (recursive loading)
5. Load prompts from prompts.d/ directory
6. Start transport(s):
   - If --stdio: run stdio server with LSP framing
   - Else: run both SSE (3000) and HTTP (3001) servers concurrently

## MCP Protocol Methods

| Method | Purpose |
|--------|---------|
| initialize | Handshake with capabilities (tools, resources, prompts) |
| tools/list | Returns registered tools with JSON Schema |
| tools/call | Executes a tool with arguments |
| resources/list | Lists configured resources |
| resources/read | Reads a resource |
| resources/templates/list | Lists resource templates |
| prompts/list | Lists reusable prompt templates |
| prompts/get | Retrieves prompt with variable substitution |
| ping | Health check |

## Dependencies

**Async/HTTP:** tokio, actix-web, reqwest
**Serialization:** serde, serde_json, toml
**Utilities:** schemars, thiserror, log, flexi_logger, custom-utils, uuid, clap
**For mcp-tool-generator:** async-openai (OpenAI API client)

## Configuration Files

### config.toml (Server)
```toml
[server]
host = "0.0.0.0"
sse_port = 3000
http_port = 3001

[defaults]
timeout_secs = 600
[[defaults.directories]]
path = "/path/to/data"
description = "Data directory description"

[security]
allow_direct_command = true
```

### tools.d/*.toml (Tool Definitions)
```toml
[config]
timeout_secs = 60
env = { VAR = "value" }

[[tools]]
name = "tool_name"
description = "Tool description"
type = "command"
command = "git"
args = ["status"]

[[tools.parameters]]
name = "param_name"
type = "string"
required = true
```

## Common Patterns

### Logging
```rust
use log::{info, error, warn};
info!("Tool {} called", tool_name);
error!("Execution failed: {}", e);
```

### Error Handling (lib code)
```rust
fn execute(tool: &RegisteredTool) -> anyhow::Result<String> {
    let output = Command::new("cmd").arg("arg").status()?;
    Ok(format!("{:?}", output))
}
```

### Template Resolution
Both command and HTTP executors use ${var_name} syntax:
```rust
let resolved = CommandExecutor::resolve_template("git ${action}", &args)?;
```

## Development Tips

- Use stdio mode with mcp-client binary for local testing
- Environment variables set in config.env or tools[].env per TOML
- Parameter templates: ${var_name} for substitution
- SSE sessions auto-expire after 90 seconds inactivity
- Tool output capped at 50KB to prevent context overflow
- Docker deployment recommended (see README.md)

## Related Tools

**mcp-tool-generator** (in workspace):
- Auto-generates TOML configs by parsing --help with LLM
- Useful for bootstrapping from existing CLI commands
- Uses OpenAI API for command structure analysis

## Claude Desktop Integration

```json
{
  "mcpServers": {
    "mcp-server": {
      "command": "/path/to/mcp",
      "args": ["--cwd", "/path/to/config/dir", "--stdio"]
    }
  }
}
```

## Code Guidelines (from AGENTS.md)

- **Communicate with the user in Chinese**
- Use Rust 2024 edition and tokio for async
- Prefer anyhow::Result in lib code
- Use log macros instead of println!
- HTTP client: always async reqwest::Client
- Keep main.rs minimal (startup only)
- No #[allow(...)] without explanatory comment
- Don't add dependencies without user consent
- Keep changes small and focused; don't mix refactoring with feature changes

## CI / Build Targets

- Targets: `x86_64-pc-windows-msvc`, `aarch64-unknown-linux-gnu`
- Pushing a `v*` tag triggers a GitHub Release

## Resources

- MCP Specification: https://modelcontextprotocol.io/
- README.md: Features, Docker setup, configuration reference
- doc/: 18 goal documents for each development phase
- AGENTS.md: Agent behavior and code style guidelines
