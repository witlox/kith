//! Input classification: pass-through vs intent.
//! Rule: if first token matches a known command (from PATH), pass-through.
//! "run:" prefix forces pass-through. Otherwise, route to InferenceBackend.

use std::collections::HashSet;

/// Classification result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputClass {
    /// Send directly to bash, no LLM.
    PassThrough(String),
    /// Route to InferenceBackend for reasoning.
    Intent(String),
}

/// Classifies user input as pass-through or intent.
pub struct InputClassifier {
    known_commands: HashSet<String>,
}

impl InputClassifier {
    pub fn new(known_commands: HashSet<String>) -> Self {
        Self { known_commands }
    }

    /// Build from PATH environment variable.
    pub fn from_path_env() -> Self {
        let mut commands = HashSet::new();
        if let Ok(path) = std::env::var("PATH") {
            for dir in path.split(':') {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        if let Some(name) = entry.file_name().to_str() {
                            commands.insert(name.to_string());
                        }
                    }
                }
            }
        }
        Self::new(commands)
    }

    /// Classify user input.
    pub fn classify(&self, input: &str) -> InputClass {
        let trimmed = input.trim();

        if trimmed.is_empty() {
            return InputClass::PassThrough(String::new());
        }

        // Escape hatch: "run:" prefix forces pass-through
        if let Some(rest) = trimmed.strip_prefix("run:") {
            return InputClass::PassThrough(rest.trim().to_string());
        }

        // First token check: if it matches a known command, pass-through
        let first_token = trimmed.split_whitespace().next().unwrap_or("");

        if self.known_commands.contains(first_token) {
            return InputClass::PassThrough(trimmed.to_string());
        }

        // If first token looks like an absolute path to an executable
        if first_token.starts_with('/') || first_token.starts_with("./") {
            return InputClass::PassThrough(trimmed.to_string());
        }

        // Otherwise, it's natural language → intent
        InputClass::Intent(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classifier_with(commands: &[&str]) -> InputClassifier {
        let known: HashSet<String> = commands.iter().map(|s| (*s).to_string()).collect();
        InputClassifier::new(known)
    }

    #[test]
    fn empty_input_is_passthrough() {
        let c = classifier_with(&["ls"]);
        assert_eq!(c.classify(""), InputClass::PassThrough(String::new()));
        assert_eq!(c.classify("  "), InputClass::PassThrough(String::new()));
    }

    #[test]
    fn escape_hatch_forces_passthrough() {
        let c = classifier_with(&[]);
        assert_eq!(
            c.classify("run: rm -rf /tmp/test"),
            InputClass::PassThrough("rm -rf /tmp/test".into())
        );
    }

    #[test]
    fn escape_hatch_trims_whitespace() {
        let c = classifier_with(&[]);
        assert_eq!(
            c.classify("run:   docker ps"),
            InputClass::PassThrough("docker ps".into())
        );
    }

    #[test]
    fn known_command_is_passthrough() {
        let c = classifier_with(&["ls", "git", "docker", "cargo"]);
        assert_eq!(
            c.classify("ls -la"),
            InputClass::PassThrough("ls -la".into())
        );
        assert_eq!(
            c.classify("git push origin main"),
            InputClass::PassThrough("git push origin main".into())
        );
        assert_eq!(
            c.classify("docker compose up -d"),
            InputClass::PassThrough("docker compose up -d".into())
        );
    }

    #[test]
    fn absolute_path_is_passthrough() {
        let c = classifier_with(&[]);
        assert_eq!(
            c.classify("/usr/bin/python3 script.py"),
            InputClass::PassThrough("/usr/bin/python3 script.py".into())
        );
        assert_eq!(
            c.classify("./run.sh"),
            InputClass::PassThrough("./run.sh".into())
        );
    }

    #[test]
    fn natural_language_is_intent() {
        let c = classifier_with(&["ls", "git", "docker"]);
        assert_eq!(
            c.classify("what's using port 3000?"),
            InputClass::Intent("what's using port 3000?".into())
        );
        assert_eq!(
            c.classify("find all Python files that import requests"),
            InputClass::Intent("find all Python files that import requests".into())
        );
        assert_eq!(
            c.classify("deploy the app to staging"),
            InputClass::Intent("deploy the app to staging".into())
        );
    }

    #[test]
    fn unknown_command_is_intent() {
        let c = classifier_with(&["ls", "git"]);
        // "check" is not in the known commands
        assert_eq!(
            c.classify("check disk usage on staging"),
            InputClass::Intent("check disk usage on staging".into())
        );
    }

    #[test]
    fn from_path_env_finds_real_commands() {
        let c = InputClassifier::from_path_env();
        // These should exist on any Unix system
        assert_eq!(
            c.classify("ls -la"),
            InputClass::PassThrough("ls -la".into())
        );
        assert_eq!(
            c.classify("echo hello"),
            InputClass::PassThrough("echo hello".into())
        );
    }
}
