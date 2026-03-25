//! Tool discovery: scan PATH, categorize, detect versions, generate prompt summaries.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Functional category for a discovered tool.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolCategory {
    Vcs,
    Container,
    Language,
    Build,
    Server,
    Database,
    Editor,
    Network,
    Monitoring,
    Other,
}

impl std::fmt::Display for ToolCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// A discovered tool entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEntry {
    pub name: String,
    pub path: PathBuf,
    pub category: ToolCategory,
    pub version: Option<String>,
}

/// Registry of discovered tools on this machine.
#[derive(Debug, Clone)]
pub struct ToolRegistry {
    tools: Vec<ToolEntry>,
    scanned_at: DateTime<Utc>,
}

/// Categorize a tool name by matching against known tools.
pub fn categorize(name: &str) -> ToolCategory {
    match name {
        // VCS
        "git" | "svn" | "hg" | "fossil" | "darcs" => ToolCategory::Vcs,

        // Container / orchestration
        "docker" | "podman" | "kubectl" | "helm" | "crictl" | "nerdctl" | "buildah" | "skopeo"
        | "k3s" | "minikube" | "kind" | "k9s" | "oc" | "ctr" => ToolCategory::Container,

        // Languages / runtimes
        "python3" | "python" | "node" | "ruby" | "go" | "java" | "javac" | "rustc" | "perl"
        | "php" | "lua" | "R" | "julia" | "elixir" | "erlang" | "scala" | "kotlin" | "swift"
        | "dotnet" | "deno" | "bun" => ToolCategory::Language,

        // Build tools / package managers
        "cargo" | "make" | "cmake" | "npm" | "yarn" | "pnpm" | "pip" | "pip3" | "pipx"
        | "gradle" | "maven" | "mvn" | "ant" | "meson" | "ninja" | "bazel" | "buck2" | "just"
        | "task" | "nix" | "brew" | "apt" | "apt-get" | "dnf" | "yum" | "pacman" | "zypper"
        | "snap" | "flatpak" | "gem" | "bundler" | "mix" | "cabal" | "stack" | "poetry" | "uv"
        | "rye" | "pdm" | "go-task" => ToolCategory::Build,

        // Servers
        "nginx" | "apache2" | "httpd" | "caddy" | "haproxy" | "envoy" | "traefik" => {
            ToolCategory::Server
        }

        // Databases
        "psql" | "mysql" | "mongosh" | "mongo" | "redis-cli" | "sqlite3" | "clickhouse-client"
        | "cqlsh" | "influx" => ToolCategory::Database,

        // Editors
        "vim" | "nvim" | "nano" | "emacs" | "code" | "subl" | "micro" | "helix" | "hx"
        | "kakoune" | "kak" | "ed" | "vi" => ToolCategory::Editor,

        // Network tools
        "curl" | "wget" | "ssh" | "scp" | "rsync" | "nc" | "ncat" | "dig" | "nslookup" | "host"
        | "nmap" | "tcpdump" | "traceroute" | "mtr" | "ip" | "ifconfig" | "iptables" | "nft"
        | "ss" | "netstat" | "socat" | "aria2c" | "httpie" | "xh" => ToolCategory::Network,

        // Monitoring / observability
        "htop" | "top" | "btop" | "iotop" | "nethogs" | "nvidia-smi" | "rocm-smi" | "nvtop"
        | "glances" | "dstat" | "vmstat" | "iostat" | "mpstat" | "sar" | "perf" | "strace"
        | "ltrace" | "lsof" | "fuser" => ToolCategory::Monitoring,

        _ => ToolCategory::Other,
    }
}

/// Tools for which we detect versions (keep small — each spawns a subprocess).
const KEY_TOOLS: &[(&str, &[&str])] = &[
    ("git", &["--version"]),
    ("docker", &["--version"]),
    ("python3", &["--version"]),
    ("python", &["--version"]),
    ("node", &["--version"]),
    ("cargo", &["--version"]),
    ("rustc", &["--version"]),
    ("go", &["version"]),
    ("java", &["-version"]),
    ("kubectl", &["version", "--client", "--short"]),
    ("helm", &["version", "--short"]),
    ("nginx", &["-v"]),
    ("cmake", &["--version"]),
    ("make", &["--version"]),
    ("npm", &["--version"]),
    ("pip3", &["--version"]),
    ("psql", &["--version"]),
    ("sqlite3", &["--version"]),
    ("curl", &["--version"]),
    ("brew", &["--version"]),
    (
        "nvidia-smi",
        &["--query-gpu=driver_version", "--format=csv,noheader"],
    ),
];

fn is_key_tool(name: &str) -> bool {
    KEY_TOOLS.iter().any(|(k, _)| *k == name)
}

fn version_args(name: &str) -> &[&str] {
    KEY_TOOLS
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, args)| *args)
        .unwrap_or(&["--version"])
}

/// Parse a version number from command output (first match of X.Y or X.Y.Z).
fn parse_version(output: &str) -> Option<String> {
    let re_like = output
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .find(|s| {
            let parts: Vec<&str> = s.split('.').collect();
            parts.len() >= 2
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        });
    re_like.map(String::from)
}

/// Detect version for a tool by running it with version args.
fn detect_version(name: &str, path: &Path) -> Option<String> {
    if !is_key_tool(name) {
        return None;
    }
    let args = version_args(name);
    let output = std::process::Command::new(path)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    match output {
        Ok(o) => {
            let text = if o.stdout.is_empty() {
                String::from_utf8_lossy(&o.stderr).to_string()
            } else {
                String::from_utf8_lossy(&o.stdout).to_string()
            };
            let first_line = text.lines().next().unwrap_or("");
            parse_version(first_line)
        }
        Err(_) => None,
    }
}

impl ToolRegistry {
    /// Scan PATH and build a full tool registry.
    pub fn scan() -> Self {
        Self::scan_with_versions(true)
    }

    /// Scan PATH without version detection (fast, for classification only).
    pub fn scan_names_only() -> Self {
        Self::scan_with_versions(false)
    }

    fn scan_with_versions(detect_versions: bool) -> Self {
        let mut tools = Vec::new();
        let mut seen = HashSet::new();

        if let Ok(path) = std::env::var("PATH") {
            for dir in path.split(':') {
                let dir_path = Path::new(dir);
                if let Ok(entries) = std::fs::read_dir(dir_path) {
                    for entry in entries.flatten() {
                        if let Some(name) = entry.file_name().to_str() {
                            if seen.contains(name) {
                                continue; // first in PATH wins
                            }
                            let full_path = entry.path();

                            // Skip non-executable files
                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::PermissionsExt;
                                if let Ok(meta) = entry.metadata()
                                    && meta.permissions().mode() & 0o111 == 0
                                {
                                    continue;
                                }
                            }

                            let category = categorize(name);
                            let version = if detect_versions {
                                detect_version(name, &full_path)
                            } else {
                                None
                            };

                            seen.insert(name.to_string());
                            tools.push(ToolEntry {
                                name: name.to_string(),
                                path: full_path,
                                category,
                                version,
                            });
                        }
                    }
                }
            }
        }

        // Sort: categorized first (alphabetical within category), then Other
        tools.sort_by(|a, b| {
            let a_other = a.category == ToolCategory::Other;
            let b_other = b.category == ToolCategory::Other;
            a_other
                .cmp(&b_other)
                .then_with(|| a.category.to_string().cmp(&b.category.to_string()))
                .then_with(|| a.name.cmp(&b.name))
        });

        Self {
            tools,
            scanned_at: Utc::now(),
        }
    }

    /// Build from an explicit list (for testing).
    pub fn from_entries(entries: Vec<ToolEntry>) -> Self {
        Self {
            tools: entries,
            scanned_at: Utc::now(),
        }
    }

    /// All tool names (for InputClassifier).
    pub fn names(&self) -> HashSet<String> {
        self.tools.iter().map(|t| t.name.clone()).collect()
    }

    /// All entries.
    pub fn entries(&self) -> &[ToolEntry] {
        &self.tools
    }

    /// When was this registry last scanned?
    pub fn scanned_at(&self) -> DateTime<Utc> {
        self.scanned_at
    }

    /// How many tools total?
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Rescan PATH in-place.
    pub fn rescan(&mut self) {
        *self = Self::scan();
    }

    /// Time-limited rescan: only rescan if at least `min_interval` has passed.
    pub fn rescan_if_stale(&mut self, min_interval: Duration) -> bool {
        let elapsed = Utc::now()
            .signed_duration_since(self.scanned_at)
            .to_std()
            .unwrap_or(Duration::ZERO);
        if elapsed >= min_interval {
            self.rescan();
            true
        } else {
            false
        }
    }

    /// Generate a categorized summary for the system prompt.
    /// Budget: ~1500 chars max.
    pub fn prompt_summary(&self) -> String {
        use std::collections::BTreeMap;
        let mut by_category: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut other_count = 0usize;

        for tool in &self.tools {
            if tool.category == ToolCategory::Other {
                other_count += 1;
                continue;
            }
            let label = if let Some(ref v) = tool.version {
                format!("{} ({})", tool.name, v)
            } else {
                tool.name.clone()
            };
            by_category
                .entry(tool.category.to_string().to_lowercase())
                .or_default()
                .push(label);
        }

        let mut lines = Vec::new();
        for (cat, tools) in &by_category {
            lines.push(format!("{}: {}", cat, tools.join(", ")));
        }
        if other_count > 0 {
            lines.push(format!("other: {other_count} additional tools in PATH"));
        }

        let summary = lines.join("\n");

        // Budget enforcement: truncate if too long
        if summary.len() > 1500 {
            // Keep only versioned tools + counts
            let mut short_lines = Vec::new();
            for (cat, tools) in &by_category {
                let versioned: Vec<&String> = tools.iter().filter(|t| t.contains('(')).collect();
                if versioned.is_empty() {
                    short_lines.push(format!("{cat}: {} tools", tools.len()));
                } else {
                    let rest = tools.len() - versioned.len();
                    let mut parts: Vec<String> = versioned.iter().map(|t| t.to_string()).collect();
                    if rest > 0 {
                        parts.push(format!("+{rest} more"));
                    }
                    short_lines.push(format!("{cat}: {}", parts.join(", ")));
                }
            }
            if other_count > 0 {
                short_lines.push(format!("other: {other_count} additional tools"));
            }
            short_lines.join("\n")
        } else {
            summary
        }
    }

    /// Format for daemon Capabilities RPC: (name, category, version).
    pub fn to_capability_tools(&self) -> Vec<(String, String, Option<String>)> {
        self.tools
            .iter()
            .filter(|t| t.category != ToolCategory::Other)
            .map(|t| {
                (
                    t.name.clone(),
                    t.category.to_string().to_lowercase(),
                    t.version.clone(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, cat: ToolCategory, version: Option<&str>) -> ToolEntry {
        ToolEntry {
            name: name.into(),
            path: PathBuf::from(format!("/usr/bin/{name}")),
            category: cat,
            version: version.map(String::from),
        }
    }

    #[test]
    fn categorize_known_tools() {
        assert_eq!(categorize("git"), ToolCategory::Vcs);
        assert_eq!(categorize("docker"), ToolCategory::Container);
        assert_eq!(categorize("python3"), ToolCategory::Language);
        assert_eq!(categorize("cargo"), ToolCategory::Build);
        assert_eq!(categorize("nginx"), ToolCategory::Server);
        assert_eq!(categorize("psql"), ToolCategory::Database);
        assert_eq!(categorize("vim"), ToolCategory::Editor);
        assert_eq!(categorize("curl"), ToolCategory::Network);
        assert_eq!(categorize("htop"), ToolCategory::Monitoring);
    }

    #[test]
    fn categorize_unknown_is_other() {
        assert_eq!(categorize("my-custom-script"), ToolCategory::Other);
        assert_eq!(categorize("foobarbaz"), ToolCategory::Other);
    }

    #[test]
    fn parse_version_extracts_semver() {
        assert_eq!(parse_version("git version 2.45.0"), Some("2.45.0".into()));
        assert_eq!(
            parse_version("Docker version 27.1.1, build abcdef"),
            Some("27.1.1".into())
        );
        assert_eq!(parse_version("Python 3.12.0"), Some("3.12.0".into()));
        assert_eq!(parse_version("v22.11.0"), Some("22.11.0".into()));
    }

    #[test]
    fn parse_version_handles_two_part() {
        assert_eq!(parse_version("cmake version 3.28"), Some("3.28".into()));
    }

    #[test]
    fn parse_version_returns_none_for_garbage() {
        assert_eq!(parse_version("no version here"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn registry_from_entries() {
        let reg = ToolRegistry::from_entries(vec![
            entry("git", ToolCategory::Vcs, Some("2.45.0")),
            entry("docker", ToolCategory::Container, Some("27.1.1")),
            entry("my-tool", ToolCategory::Other, None),
        ]);
        assert_eq!(reg.len(), 3);
        assert!(!reg.is_empty());
    }

    #[test]
    fn registry_names_for_classifier() {
        let reg = ToolRegistry::from_entries(vec![
            entry("git", ToolCategory::Vcs, None),
            entry("docker", ToolCategory::Container, None),
        ]);
        let names = reg.names();
        assert!(names.contains("git"));
        assert!(names.contains("docker"));
        assert!(!names.contains("helm"));
    }

    #[test]
    fn prompt_summary_groups_by_category() {
        let reg = ToolRegistry::from_entries(vec![
            entry("git", ToolCategory::Vcs, Some("2.45.0")),
            entry("docker", ToolCategory::Container, Some("27.1.1")),
            entry("kubectl", ToolCategory::Container, None),
            entry("python3", ToolCategory::Language, Some("3.12.0")),
            entry("my-script", ToolCategory::Other, None),
        ]);
        let summary = reg.prompt_summary();
        assert!(summary.contains("vcs: git (2.45.0)"));
        assert!(summary.contains("container: docker (27.1.1), kubectl"));
        assert!(summary.contains("language: python3 (3.12.0)"));
        assert!(summary.contains("other: 1 additional tools"));
    }

    #[test]
    fn prompt_summary_under_budget() {
        // Generate many tools
        let mut entries = Vec::new();
        for i in 0..500 {
            entries.push(entry(&format!("tool-{i}"), ToolCategory::Other, None));
        }
        entries.push(entry("git", ToolCategory::Vcs, Some("2.45.0")));
        let reg = ToolRegistry::from_entries(entries);
        let summary = reg.prompt_summary();
        assert!(
            summary.len() <= 1500,
            "summary should be <= 1500 chars, got {}",
            summary.len()
        );
    }

    #[test]
    fn capability_tools_excludes_other() {
        let reg = ToolRegistry::from_entries(vec![
            entry("git", ToolCategory::Vcs, Some("2.45.0")),
            entry("random-thing", ToolCategory::Other, None),
        ]);
        let caps = reg.to_capability_tools();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].0, "git");
    }

    #[test]
    fn scan_finds_real_tools() {
        // This test runs on the actual system
        let reg = ToolRegistry::scan_names_only();
        // These should exist on any Unix system
        assert!(
            reg.names().contains("ls") || reg.names().contains("echo"),
            "scan should find basic Unix tools"
        );
    }

    #[test]
    fn rescan_if_stale_respects_interval() {
        let mut reg = ToolRegistry::from_entries(vec![]);
        // Just created — should not rescan
        let rescanned = reg.rescan_if_stale(Duration::from_secs(60));
        assert!(!rescanned);
    }
}
