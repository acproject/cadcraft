//! Onboarding AI IDEs: build stdio MCP config snippets for the common clients and merge a
//! `cadcraft` server entry into an existing config file without touching anything else.
//!
//! The wire protocol itself lives in [`crate::server`]; this module only produces the JSON a
//! user pastes into (or lets the CLI write into) their IDE's MCP settings.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

/// How an IDE should launch the CADCraft MCP server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// Executable path; callers normally pass the absolute path of the running `cadcraft-cli`.
    pub command: String,
    /// Arguments after the command, e.g. `["mcp"]` or `["mcp", "--connect", "127.0.0.1:7979"]`.
    pub args: Vec<String>,
    /// Extra environment variables for the spawned process.
    pub env: Vec<(String, String)>,
}

impl Launch {
    pub fn new(command: impl Into<String>) -> Self {
        Self { command: command.into(), args: vec!["mcp".into()], env: Vec::new() }
    }
    /// Bridge to a running CADCraft app's control port instead of a headless session.
    pub fn connect(mut self, addr: &str) -> Self {
        self.args.push("--connect".into());
        self.args.push(addr.to_string());
        self
    }
    /// Open a drawing file when the headless server starts.
    pub fn open(mut self, path: &str) -> Self {
        self.args.push("--open".into());
        self.args.push(path.to_string());
        self
    }
    /// Add an environment variable for the spawned server.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

/// A recognised AI IDE / MCP host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ide {
    /// The base `{"mcpServers": {...}}` shape: Claude Code, Continue and most other hosts.
    Generic,
    /// Claude Desktop application.
    ClaudeDesktop,
    /// Cursor editor (`~/.cursor/mcp.json`).
    Cursor,
    /// Trae / Trae CN editor.
    Trae,
    /// Windsurf editor (`~/.codeium/windsurf/mcp_config.json`).
    Windsurf,
    /// Cline extension for VS Code-style editors.
    Cline,
    /// VS Code GitHub Copilot MCP (`.vscode/mcp.json`, project-scoped, key `servers`).
    Vscode,
}

impl Ide {
    pub const fn all() -> &'static [Ide] {
        &[Ide::Generic, Ide::ClaudeDesktop, Ide::Cursor, Ide::Trae, Ide::Windsurf, Ide::Cline, Ide::Vscode]
    }
    /// Parse the id used on the command line and in docs.
    pub fn parse(id: &str) -> Option<Ide> {
        Ide::all().iter().copied().find(|i| i.id() == id.to_ascii_lowercase())
    }
    pub const fn id(&self) -> &'static str {
        match self {
            Ide::Generic => "generic",
            Ide::ClaudeDesktop => "claude-desktop",
            Ide::Cursor => "cursor",
            Ide::Trae => "trae",
            Ide::Windsurf => "windsurf",
            Ide::Cline => "cline",
            Ide::Vscode => "vscode",
        }
    }
    pub const fn label(&self) -> &'static str {
        match self {
            Ide::Generic => "Generic MCP host (mcpServers JSON)",
            Ide::ClaudeDesktop => "Claude Desktop",
            Ide::Cursor => "Cursor",
            Ide::Trae => "Trae / Trae CN",
            Ide::Windsurf => "Windsurf",
            Ide::Cline => "Cline (VS Code extension)",
            Ide::Vscode => "VS Code (GitHub Copilot, .vscode/mcp.json)",
        }
    }
    /// Top-level JSON key the host keeps its server table under.
    const fn servers_key(&self) -> &'static str {
        match self {
            Ide::Vscode => "servers",
            _ => "mcpServers",
        }
    }
    /// Whether entries require an explicit `"type": "stdio"` field.
    const fn requires_type(&self) -> bool {
        matches!(self, Ide::Vscode)
    }
    /// Conventional config file location for this host, if one exists.
    ///
    /// `home` is the user's home directory; `cwd` is used for project-scoped configs.
    pub fn config_path(&self, home: &Path, cwd: &Path) -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        let app_support = home.join("Library/Application Support");
        #[cfg(windows)]
        let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
        Some(match self {
            Ide::Generic => return None,
            Ide::Vscode => cwd.join(".vscode/mcp.json"),
            Ide::Cursor => home.join(".cursor/mcp.json"),
            Ide::Windsurf => home.join(".codeium/windsurf/mcp_config.json"),
            Ide::Trae => {
                // Trae CN keeps its data in ~/.trae-cn; the international build uses ~/.trae.
                if home.join(".trae-cn").is_dir() { home.join(".trae-cn/mcp.json") } else { home.join(".trae/mcp.json") }
            }
            Ide::ClaudeDesktop => {
                #[cfg(target_os = "macos")]
                {
                    app_support.join("Claude/claude_desktop_config.json")
                }
                #[cfg(windows)]
                {
                    appdata?.join("Claude/claude_desktop_config.json")
                }
                #[cfg(not(any(target_os = "macos", windows)))]
                {
                    home.join(".config/Claude/claude_desktop_config.json")
                }
            }
            Ide::Cline => {
                let tail = "User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json";
                #[cfg(target_os = "macos")]
                {
                    app_support.join("Code").join(tail)
                }
                #[cfg(windows)]
                {
                    appdata?.join("Code").join(tail)
                }
                #[cfg(not(any(target_os = "macos", windows)))]
                {
                    home.join(".config/Code").join(tail)
                }
            }
        })
    }
}

/// The `cadcraft` server entry object.
pub fn server_entry(launch: &Launch, ide: Ide) -> Value {
    let mut m = Map::new();
    if ide.requires_type() {
        m.insert("type".into(), json!("stdio"));
    }
    m.insert("command".into(), json!(launch.command));
    m.insert("args".into(), Value::Array(launch.args.iter().map(|a| json!(a)).collect()));
    if !launch.env.is_empty() {
        let mut e = Map::new();
        for (k, v) in &launch.env {
            e.insert(k.clone(), json!(v));
        }
        m.insert("env".into(), Value::Object(e));
    }
    Value::Object(m)
}

/// A complete, paste-ready config document for one host.
pub fn snippet(ide: Ide, launch: &Launch) -> Value {
    json!({ide.servers_key(): {"cadcraft": server_entry(launch, ide)}})
}

/// Merge a `cadcraft` entry into an existing config document (or an empty one), preserving
/// every other server and setting. Returns pretty-printed JSON.
pub fn merge(existing: Option<&str>, ide: Ide, launch: &Launch) -> Result<String, String> {
    let mut doc: Map<String, Value> = match existing.map(str::trim).filter(|s| !s.is_empty()) {
        Some(text) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(o)) => o,
            Ok(_) => return Err("existing config is not a JSON object".into()),
            Err(e) => return Err(format!("existing config is not valid JSON: {e}")),
        },
        None => Map::new(),
    };
    let table = doc
        .entry(ide.servers_key().to_string())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("`{}` is already present but is not a JSON object", ide.servers_key()))?;
    table.insert("cadcraft".into(), server_entry(launch, ide));
    serde_json::to_string_pretty(&Value::Object(doc)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch() -> Launch {
        Launch::new("/usr/local/bin/cadcraft-cli")
    }

    #[test]
    fn ids_round_trip() {
        for ide in Ide::all() {
            assert_eq!(Ide::parse(ide.id()), Some(*ide));
        }
        assert_eq!(Ide::parse("CURSOR"), Some(Ide::Cursor));
        assert_eq!(Ide::parse("nope"), None);
    }

    #[test]
    fn generic_snippet_is_mcpservers_shape() {
        let v = snippet(Ide::Generic, &launch());
        let e = &v["mcpServers"]["cadcraft"];
        assert_eq!(e["command"], "/usr/local/bin/cadcraft-cli");
        assert_eq!(e["args"][0], "mcp");
        assert!(e.get("type").is_none());
        assert!(e.get("env").is_none());
    }

    #[test]
    fn vscode_uses_servers_key_and_stdio_type() {
        let v = snippet(Ide::Vscode, &launch().connect("127.0.0.1:7979"));
        let e = &v["servers"]["cadcraft"];
        assert_eq!(e["type"], "stdio");
        assert_eq!(e["args"][1], "--connect");
        assert_eq!(e["args"][2], "127.0.0.1:7979");
    }

    #[test]
    fn env_is_included_only_when_set() {
        let v = snippet(Ide::Generic, &launch().env("RUST_LOG", "info"));
        assert_eq!(v["mcpServers"]["cadcraft"]["env"]["RUST_LOG"], "info");
    }

    #[test]
    fn merge_into_empty_equals_snippet() {
        let out = merge(None, Ide::Cursor, &launch()).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["cadcraft"]["command"], "/usr/local/bin/cadcraft-cli");
    }

    #[test]
    fn merge_preserves_other_servers_and_is_idempotent() {
        let before = r#"{
  "mcpServers": {
    "other": {"command": "other-tool", "args": []}
  }
}"#;
        let once = merge(Some(before), Ide::Cursor, &launch()).unwrap();
        let v: Value = serde_json::from_str(&once).unwrap();
        assert_eq!(v["mcpServers"]["other"]["command"], "other-tool");
        assert_eq!(v["mcpServers"]["cadcraft"]["args"][0], "mcp");
        let twice = merge(Some(&once), Ide::Cursor, &launch()).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn merge_rejects_non_object_and_bad_json() {
        assert!(merge(Some("[1,2]"), Ide::Cursor, &launch()).is_err());
        assert!(merge(Some("{not json"), Ide::Cursor, &launch()).is_err());
        // A servers table of the wrong JSON type must not be overwritten silently.
        assert!(merge(Some(r#"{"mcpServers": [1]}"#), Ide::Cursor, &launch()).is_err());
    }

    #[test]
    fn config_paths_follow_conventions() {
        let home = Path::new("/home/alice");
        let cwd = Path::new("/home/alice/proj");
        assert_eq!(Ide::Cursor.config_path(home, cwd), Some(PathBuf::from("/home/alice/.cursor/mcp.json")));
        assert_eq!(Ide::Windsurf.config_path(home, cwd), Some(PathBuf::from("/home/alice/.codeium/windsurf/mcp_config.json")));
        assert_eq!(Ide::Vscode.config_path(home, cwd), Some(cwd.join(".vscode/mcp.json")));
        assert_eq!(Ide::Generic.config_path(home, cwd), None);
    }
}
