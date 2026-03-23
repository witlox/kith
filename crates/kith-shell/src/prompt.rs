//! System prompt assembly. Thin by design — safety is in the daemon, not the prompt.

/// Build the system prompt for the agent.
/// ~2K tokens target (not 27K like Claude Code — safety is in infrastructure).
pub fn build_system_prompt(
    hostname: &str,
    os_info: &str,
    fleet_summary: &str,
    project_context: Option<&str>,
) -> String {
    let mut prompt = format!(
        "You are a shell agent. You operate on a mesh of machines.\n\
        \n\
        Local operations: use standard Unix commands. You are on {hostname}, \
        running {os_info}. Available tools in your PATH are discoverable.\n\
        \n\
        Remote operations: use remote(host, command) to execute on other \
        machines. Use fleet_query() to check machine state without executing. \
        All remote operations are authenticated with your user's identity \
        and scoped by policy.\n\
        \n\
        Changes: local file changes and remote state changes enter a pending \
        state. Present the diff to the user. They commit or rollback. \
        Don't assume approval.\n\
        \n\
        Context: use retrieve(query) to search operational history when you \
        need background on what happened previously. The history spans all \
        machines in the mesh.\n"
    );

    if !fleet_summary.is_empty() {
        prompt.push_str(&format!("\nFleet state:\n{fleet_summary}\n"));
    }

    if let Some(ctx) = project_context {
        prompt.push_str(&format!("\nProject context:\n{ctx}\n"));
    }

    prompt.push_str(
        "\nYou are not a coding assistant. You are a shell. Users may ask you \
        to do anything they'd do in a terminal — code, deploy, debug, \
        configure, investigate, automate. Use Unix tools directly. \
        Don't explain unless asked.\n",
    );

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_contains_hostname() {
        let p = build_system_prompt("dev-mac", "Darwin 25.3.0", "", None);
        assert!(p.contains("dev-mac"));
    }

    #[test]
    fn prompt_contains_os_info() {
        let p = build_system_prompt("dev-mac", "Darwin 25.3.0", "", None);
        assert!(p.contains("Darwin 25.3.0"));
    }

    #[test]
    fn prompt_includes_fleet_summary() {
        let p = build_system_prompt(
            "dev-mac",
            "Darwin",
            "staging-1: healthy, 12GB free",
            None,
        );
        assert!(p.contains("staging-1: healthy"));
    }

    #[test]
    fn prompt_includes_project_context() {
        let p = build_system_prompt("dev-mac", "Darwin", "", Some("FastAPI service, Docker deploy"));
        assert!(p.contains("FastAPI service"));
    }

    #[test]
    fn prompt_omits_fleet_when_empty() {
        let p = build_system_prompt("dev-mac", "Darwin", "", None);
        assert!(!p.contains("Fleet state:"));
    }

    #[test]
    fn prompt_is_reasonably_short() {
        let p = build_system_prompt("dev-mac", "Darwin 25.3.0", "staging-1: ok", Some("project ctx"));
        // Should be well under 2K tokens (~500 words max)
        assert!(p.len() < 3000, "prompt should be <3000 chars, got {}", p.len());
    }

    #[test]
    fn prompt_instructs_unix_usage() {
        let p = build_system_prompt("dev-mac", "Darwin", "", None);
        assert!(p.contains("Unix tools directly"));
        assert!(p.contains("remote(host, command)"));
        assert!(p.contains("retrieve(query)"));
    }

    #[test]
    fn prompt_mentions_commit_rollback() {
        let p = build_system_prompt("dev-mac", "Darwin", "", None);
        assert!(p.contains("commit or rollback"));
    }
}
