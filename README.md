# MCP Server

A [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server implemented in Rust. Dynamically registers tools via configuration files, supporting local command execution and HTTP API calls.

[中文文档](README_cn.md)

## Features

- **Configuration-driven**: All tools are dynamically registered via TOML files under `tools.d/` — no hardcoded tools
- **Multiple tool types**: Local command execution (command) and HTTP API calls (http)
- **Dual transport**: SSE (Server-Sent Events) and stdio
- **Security**: Directory whitelisting, path escape prevention, output truncation, process injection protection
- **Parameter templates**: Flexible parameterized commands using `${var}` placeholders
- **Tool generator**: Bundled `mcp-tool-generator` that auto-generates tool configs via LLM

## Quick Start

### Simplified Path Management

The server now **ignores custom `--cwd` arguments** and always uses the current working directory as its workspace. Configuration (`config.toml`) and tools (`tools.d/`) are expected to be located relative to the directory where the binary is executed.

### Recommended Docker Deployment

Running the server inside a Docker container is the preferred way to manage file paths and ensure a consistent environment.

```Dockerfile
FROM rust:latest AS builder
WORKDIR /usr/src/mcp
COPY . .
RUN cargo build --release

FROM debian:buster-slim
COPY --from=builder /usr/src/mcp/target/release/mcp-server /usr/local/bin/mcp-server
WORKDIR /app
COPY config.toml ./
COPY tools.d ./tools.d
EXPOSE 3000
CMD ["mcp-server"]
```

```bash
# Build the Docker image
docker build -t mcp-server .
# Run the container
docker run -d -p 3000:3000 \
    -v $(pwd)/config.toml:/app/config.toml \
    -v $(pwd)/tools.d:/app/tools.d \
    --name mcp-server mcp-server
```

This approach provides filesystem isolation and eliminates the need for manual path handling.



### Build

```bash
cargo build --release
```

### Run

**SSE mode (default):**

```bash
./target/release/mcp-server --config config.toml
# Listens on http://127.0.0.1:3000
```

**Stdio mode:**

```bash
./target/release/mcp-server --config config.toml --stdio
```

**Export tool config JSON Schema:**

```bash
./target/release/mcp-server --schema
```

### Integration with Claude Desktop

Add the following to your Claude Desktop configuration:

```json
{
  "mcpServers": {
    "mcp-server": {
      "command": "/path/to/mcp-server",
      "args": ["--config", "/path/to/config.toml", "--stdio"]
    }
  }
}
```

## Configuration

### Server Configuration (config.toml)

```toml
[server]
host = "127.0.0.1"
port = 3000

[defaults]
timeout_secs = 60
allowed_dirs = ["/data/repos", "/home/user/workspace"]

[security]
allow_direct_command = true
```

| Field | Description |
|-------|-------------|
| `server.host` | HTTP listen address |
| `server.port` | HTTP listen port |
| `defaults.timeout_secs` | Global timeout ceiling (seconds) |
| `defaults.allowed_dirs` | Whitelisted directories for tool execution |
| `security.allow_direct_command` | Whether to expose the arbitrary command execution tool |

### Tool Configuration (tools.d/*.toml)

Each `.toml` file may contain a `[config]` section for shared settings and multiple `[[tools]]` definitions.

#### Command Type

```toml
[config]
working_dir = "/data/repos"
timeout_secs = 60
env = { "GIT_SSL_NO_VERIFY" = "1" }

[[tools]]
name = "git_status"
description = "Show git status of a project"
command = "git"
args = ["status"]
sub_dir = "${project}"

[[tools.parameters]]
name = "project"
description = "Project folder name"
type = "string"
required = true
```

#### HTTP Type

```toml
[config]
base_url = "https://httpbin.org"
timeout_secs = 10

[[tools]]
name = "http_get_ip"
description = "Get current IP address"
type = "http"
method = "GET"
path = "/ip"
```

### Configuration Merging Rules

- **Timeout**: Takes the minimum of tool-level, file-level, and global `defaults.timeout_secs`; the global value acts as an absolute ceiling
- **Environment variables**: `[config].env` merges with `[[tools]].env`; tool-level takes precedence
- **Working directory**: `allowed_dirs` serves as the security boundary, `working_dir` as the group directory, and `sub_dir` as a relative subdirectory beneath it

### Parameters

Use `${var}` placeholders in `args` and `sub_dir` to reference parameters. Parameters are defined via `[[tools.parameters]]`:

| Field | Description |
|-------|-------------|
| `name` | Parameter name |
| `type` | Parameter type (string / number / boolean) |
| `description` | Parameter description |
| `required` | Whether the parameter is required |

Optional parameters that are not provided are automatically removed from args.

## MCP Protocol Support

| Method | Description |
|--------|-------------|
| `initialize` | Handshake, returns server capabilities |
| `initialized` | Notification (no response) |
| `ping` | Health check |
| `tools/list` | List all registered tools with JSON Schema |
| `tools/call` | Execute a specified tool |

## Transport Protocols

### SSE (Server-Sent Events)

- `GET /sse`: Creates a session and returns the endpoint URL via SSE
- `POST /message?sessionId=<id>`: Receives MCP requests and sends responses back via SSE

### Stdio

Start with `--stdio` flag. Reads JSON-RPC requests from stdin and writes responses to stdout (one per line). Logs are sent to stderr to avoid interfering with protocol communication.

## Security

- **Directory whitelisting**: All tool working directories must be within `allowed_dirs`
- **Path escape prevention**: Resolved `sub_dir` must remain inside `working_dir`, blocking `../../` traversal
- **Output truncation**: stdout/stderr limited to 50KB, preventing memory exhaustion and LLM context overflow
- **Injection protection**: Uses native process argument passing, never shell string concatenation
- **Direct command switch**: Arbitrary command execution gated by `allow_direct_command`

## Tool Generator: mcp-tool-generator

A companion tool that auto-generates tool configuration files. It recursively parses `--help` output of a command and uses an LLM (vLLM) to generate TOML configs for each subcommand.

### Usage

```bash
cargo run -p mcp-tool-generator -- \
  -s /path/to/mcp-server \
  -c config.toml \
  -u http://localhost:8000 \
  -m 3 \
  -o output.toml \
  cargo
```

### Arguments

| Argument | Description | Default |
|----------|-------------|---------|
| `<command_name>` | Command to analyze | (required) |
| `-s, --mcp-server` | Path to mcp-server binary | `mcp-server` |
| `-c, --server-config-path` | Server config file path | `config.toml` |
| `-u, --vllm-url` | vLLM API endpoint | `http://localhost:8000` |
| `-m, --max-depth` | Max recursion depth (capped at 5) | `3` |
| `-o, --output-path` | Output file path | stdout |

### Workflow

1. Connects to mcp-server via stdio, using the `direct_command` tool to run help commands
2. Recursively discovers subcommands (identified by LLM)
3. Sends all help text along with the JSON Schema to vLLM
4. LLM generates tool configs for each subcommand and flags safety concerns
5. Outputs a merged `.toml` file (dangerous commands marked with comments)

## Project Structure

```
├── Cargo.toml                 # Workspace definition
├── config.toml                # Server configuration
├── tools.d/                   # Tool configuration directory
│   └── test.toml
├── src/                       # MCP Server source
│   ├── main.rs                # Entry point, SSE/stdio startup
│   ├── lib.rs                 # Library exports
│   ├── config/                # Configuration parsing
│   │   ├── server.rs          # Server config structs
│   │   ├── tool.rs            # Tool registry and loading
│   │   └── schema.rs          # JSON Schema generation
│   ├── protocol/              # MCP protocol
│   │   ├── types.rs           # JSON-RPC type definitions
│   │   └── handler.rs         # Request dispatching
│   ├── executor/              # Execution engines
│   │   ├── command.rs         # Command executor
│   │   └── http.rs            # HTTP executor
│   ├── security.rs            # Path security validation
│   └── transport/             # Transport layer
│       ├── sse.rs             # SSE transport
│       └── stdio.rs           # Stdio transport
├── mcp-tool-generator/        # Tool generator
│   └── src/
│       ├── main.rs            # CLI entry point
│       ├── config.rs          # Configuration
│       ├── mcp_client.rs      # Stdio MCP client
│       ├── llm_client.rs      # vLLM API client
│       ├── crawler.rs         # Recursive help crawler
│       ├── prompt.rs          # LLM prompt building
│       ├── toml_output.rs     # TOML output generation
│       └── types.rs           # Data structures
└── doc/                       # Development docs
```

## License

MIT
