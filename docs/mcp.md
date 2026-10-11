# CADCraft and AI agents (MCP)

CADCraft speaks the [Model Context Protocol](https://modelcontextprotocol.io) over stdio.

## Start it

```sh
# Headless: an in-process drawing session, no window.
cadcraft-cli mcp

# Bridged to the running app (watch the agent draw):
cadcraft --control 7979 &
cadcraft-cli mcp --connect 127.0.0.1:7979
```

Example client configuration (Claude Code / Claude Desktop):

```json
{ "mcpServers": { "cadcraft": { "command": "cadcraft-cli", "args": ["mcp"] } } }
```

## Connect an AI IDE

Any MCP host that can spawn a stdio command works (Cursor, VS Code + GitHub Copilot/Cline,
Trae, Windsurf, Claude Desktop, Claude Code…). Generate a ready-to-paste config with the CLI —
it fills in the absolute path of the `cadcraft-cli` binary itself:

```sh
cadcraft-cli mcp-config --ide cursor        # print Cursor's ~/.cursor/mcp.json entry
cadcraft-cli mcp-config --ide vscode        # .vscode/mcp.json (servers / stdio)
cadcraft-cli mcp-config --ide trae          # Trae / Trae CN global mcp.json
cadcraft-cli mcp-config --ide windsurf
cadcraft-cli mcp-config --ide claude-desktop
cadcraft-cli mcp-config --ide cline
cadcraft-cli mcp-config                     # generic {"mcpServers": {...}} shape
```

Add `--write` to merge the entry straight into that IDE's config file (existing servers and
settings are preserved; rerunning is safe and idempotent):

```sh
cadcraft-cli mcp-config --ide cursor --write
```

Variants:

```sh
# Headless server that opens a drawing on startup:
cadcraft-cli mcp-config --open "$HOME/drawings/house.dxf"

# Bridge to the running app instead (watch the agent draw in the window):
cadcraft --control 7979 &
cadcraft-cli mcp-config --ide cursor --connect 127.0.0.1:7979
```

If the IDE only lets you edit env vars, the same bridge is picked up from
`CADCRAFT_MCP_CONNECT=127.0.0.1:7979`. The server writes nothing to stdout except protocol
messages, so it is safe to spawn from any host.

## Tools

| Tool | What it does |
|---|---|
| `command_line {text}` | Type at the command line exactly like a person: `line 0,0 @10,0 @0,5 c`, `circle 5,2 1`, `offset 0.5`, `zoom e`. Spaces act as Enter; empty text is Enter. Returns the new prompt, keywords and output. |
| `script {text}` | A multi-line script (like `.scr` files). |
| `execute {command, params}` | Any command with JSON parameters, never a dialog. `list_commands` shows ids and parameter docs. |
| `list_commands {filter?}` | The command catalog. |
| `inspect_drawing {entities?, limit?}` | Counts, extents, layers, styles, blocks, layouts, selection, undo history and entities. |
| `query_entities {type?, layer?, window?}` | Filtered entities with handles and geometry. |
| `render {width?, height?, fit?}` | A PNG image of the drawing. |
| `new_drawing {metric?}`, `open {path}`, `save {path?}`, `cancel` | Files and Escape. |
| `screenshot`, `ui_inspect`, `ui_click {x, y}` | Only when connected to the app. |

Resources: `cadcraft://drawing` (inspect JSON) and `cadcraft://commands` (catalog).

## A typical session

1. `command_line {"text": "rectang 0,0 100,60"}`
2. `execute {"command": "circle", "params": {"center": [10, 10], "radius": 4}}`
3. `command_line {"text": "dimlinear 0,0 100,0 50,-8"}`
4. `inspect_drawing {}` — verify counts and geometry.
5. `render {}` — look at it.

The end-to-end test in `crates/mcp/src/tests.rs` runs a session like this and checks the result
only through MCP.
