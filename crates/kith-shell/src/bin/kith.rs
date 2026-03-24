//! kith — intent-driven distributed shell.
//!
//! Usage:
//!   kith                          # interactive mode
//!   kith "deploy to staging"      # single command mode
//!   kith --backend anthropic      # override backend
//!   kith --daemon host:port       # connect to remote daemon

use kith_common::credential::Keypair;
use kith_common::inference::InferenceBackend;
use kith_shell::agent::{Agent, AgentOutput};
use kith_shell::inference::{AnthropicBackend, OpenAiCompatBackend};
use kith_shell::prompt::build_system_prompt;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();

    // Handle --init: generate keypair + default config
    if args.iter().any(|a| a == "--init") {
        return run_init();
    }

    // Handle --status: show system state
    if args.iter().any(|a| a == "--status" || a == "status") {
        return run_status().await;
    }

    // Load config file (if exists): ~/.config/kith/config.toml
    let config_path = find_flag(&args, "--config");
    let config =
        kith_common::config::KithConfig::load(config_path.as_deref().map(std::path::Path::new))
            .unwrap_or_else(|e| {
                eprintln!("warning: config load failed: {e}");
                None
            });

    // Resolve settings: CLI flags > env vars > config file > defaults
    let cfg_inference = config.as_ref().and_then(|c| c.inference.as_ref());

    let backend_type = find_flag(&args, "--backend")
        .or_else(|| std::env::var("KITH_BACKEND").ok())
        .or_else(|| cfg_inference.map(|i| i.backend.clone()))
        .unwrap_or_else(|| "openai-compatible".into());

    let endpoint = find_flag(&args, "--endpoint")
        .or_else(|| std::env::var("KITH_ENDPOINT").ok())
        .or_else(|| cfg_inference.and_then(|i| i.endpoint.clone()))
        .unwrap_or_else(|| "http://localhost:8000/v1".into());

    let model = find_flag(&args, "--model")
        .or_else(|| std::env::var("KITH_MODEL").ok())
        .or_else(|| cfg_inference.map(|i| i.model.clone()))
        .unwrap_or_else(|| "default".into());

    let daemon_addr = find_flag(&args, "--daemon").or_else(|| std::env::var("KITH_DAEMON").ok());

    // Resolve API key: env var name from config, then try common env vars
    let api_key_env = cfg_inference.and_then(|i| i.api_key_env.clone());

    // Build inference backend
    let backend: Box<dyn InferenceBackend> = match backend_type.as_str() {
        "anthropic" => {
            let key_var = api_key_env.as_deref().unwrap_or("ANTHROPIC_API_KEY");
            let api_key =
                std::env::var(key_var).map_err(|_| format!("{key_var} env var not set"))?;
            let model = if model == "default" {
                "claude-sonnet-4-20250514".into()
            } else {
                model
            };
            Box::new(AnthropicBackend::new(api_key, model))
        }
        _ => {
            let key_var = api_key_env.as_deref().unwrap_or("OPENAI_API_KEY");
            let api_key = std::env::var(key_var).ok();
            Box::new(OpenAiCompatBackend::new(endpoint, model, api_key))
        }
    };

    // Build system prompt
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "unknown".into());
    let os_info = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    let prompt = build_system_prompt(&hostname, &os_info, "", None);

    // Build embedding backend from config
    let embedder: Box<dyn kith_state::embedding::EmbeddingBackend> =
        match config.as_ref().and_then(|c| c.embedding.as_ref()) {
            Some(emb_cfg) if emb_cfg.backend == "api" => {
                let endpoint = emb_cfg
                    .endpoint
                    .as_deref()
                    .unwrap_or("http://localhost:11434/v1");
                let model = emb_cfg.model.as_deref().unwrap_or("all-minilm");
                let dims = emb_cfg.dimensions.unwrap_or(384);
                Box::new(kith_state::api_embedding::ApiEmbeddingBackend::new(
                    endpoint.into(),
                    model.into(),
                    None,
                    dims,
                ))
            }
            _ => Box::new(kith_state::embedding::BagOfWordsEmbedder::new(1000)),
        };

    let mut agent = Agent::with_embedder(backend, prompt, embedder);

    // Connect to daemon if specified
    if let Some(ref addr) = daemon_addr {
        let keypair = load_or_create_keypair()?;
        match kith_shell::daemon_client::DaemonClient::connect(addr, keypair).await {
            Ok(client) => {
                info!(addr = %addr, "connected to daemon");
                agent.set_daemon(client);
            }
            Err(e) => {
                eprintln!("warning: could not connect to daemon at {addr}: {e}");
            }
        }
    }

    // Single command mode
    let non_flag_args: Vec<&str> = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(|a| a.as_str())
        .collect();

    if !non_flag_args.is_empty() {
        let input = non_flag_args.join(" ");
        let output = agent.process(&input).await;
        print_output(&output);
        std::process::exit(match &output {
            AgentOutput::PassThrough { exit_code, .. } => *exit_code,
            AgentOutput::Error(_) => 1,
            _ => 0,
        });
    }

    // Interactive mode with PTY + rustyline
    eprintln!("kith shell — {} on {hostname}", agent.backend_name());
    eprintln!("type naturally or use commands directly. run: prefix for escape hatch.\n");

    // Spawn PTY bash for pass-through commands
    let pty = match kith_shell::pty::PtyShell::spawn() {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("warning: PTY unavailable ({e}), using direct exec fallback");
            None
        }
    };

    // rustyline for line editing with history
    let history_path = dirs_next::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("kith")
        .join("history.txt");
    let mut rl = rustyline::DefaultEditor::new()
        .unwrap_or_else(|_| rustyline::DefaultEditor::new().expect("rustyline init"));
    let _ = rl.load_history(&history_path);

    loop {
        let readline = rl.readline("kith> ");
        match readline {
            Ok(line) => {
                let input = line.trim();
                if input.is_empty() {
                    continue;
                }
                if input == "exit" || input == "quit" {
                    break;
                }
                rl.add_history_entry(input).ok();

                // Classify and route
                use kith_shell::classify::InputClass;
                match agent.classifier().classify(input) {
                    InputClass::PassThrough(cmd) => {
                        if cmd.is_empty() {
                            continue;
                        }
                        if let Some(ref pty_shell) = pty {
                            // Execute via PTY bash
                            match pty_shell
                                .exec_and_capture(&cmd, std::time::Duration::from_secs(30))
                            {
                                Ok(output) => print!("{output}"),
                                Err(e) => eprintln!("error: {e}"),
                            }
                        } else {
                            // Fallback: direct exec
                            let output = agent.process(input).await;
                            print_output(&output);
                        }
                    }
                    InputClass::Intent(_) => {
                        // Route to LLM agent
                        let output = agent.process(input).await;
                        print_output(&output);
                    }
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted) => {
                // Ctrl-C: clear line, continue
                continue;
            }
            Err(rustyline::error::ReadlineError::Eof) => {
                // Ctrl-D: exit
                break;
            }
            Err(e) => {
                eprintln!("error: {e}");
                break;
            }
        }
    }

    let _ = rl.save_history(&history_path);

    Ok(())
}

fn print_output(output: &AgentOutput) {
    match output {
        AgentOutput::PassThrough {
            stdout,
            stderr,
            exit_code: _,
            ..
        } => {
            if !stdout.is_empty() {
                print!("{stdout}");
            }
            if !stderr.is_empty() {
                eprint!("{stderr}");
            }
        }
        AgentOutput::Text(text) => {
            println!("{text}");
        }
        AgentOutput::ToolResults(results) => {
            for r in results {
                println!("[{}] {}", r.tool_name, r.output);
            }
        }
        AgentOutput::Degraded { input: _ } => {
            eprintln!("inference unavailable — pass-through mode");
            // Could exec the input as bash here
        }
        AgentOutput::Error(msg) => {
            eprintln!("error: {msg}");
        }
    }
}

fn find_flag(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn load_or_create_keypair() -> Result<Keypair, Box<dyn std::error::Error>> {
    let key_path = dirs_next::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("kith")
        .join("identity.key");

    if key_path.exists() {
        // Warn if key file has loose permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::metadata(&key_path)?.permissions();
            let mode = perms.mode() & 0o777;
            if mode & 0o077 != 0 {
                eprintln!(
                    "warning: identity key at {} has permissions {:o}, should be 600",
                    key_path.display(),
                    mode
                );
            }
        }
        let bytes = std::fs::read(&key_path)?;
        let secret: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| "invalid key file: expected 32 bytes")?;
        Ok(Keypair::from_secret(&secret))
    } else {
        let kp = Keypair::generate();
        if let Some(parent) = key_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&key_path, kp.secret_bytes())?;
        // Set restrictive permissions (FS-01)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
        }
        let pubkey = kith_common::credential::pubkey_to_hex(&kp.public_key_bytes());
        eprintln!("generated new identity: {pubkey}");
        eprintln!("key stored at: {}", key_path.display());
        Ok(kp)
    }
}

fn run_init() -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = dirs_next::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("kith");
    std::fs::create_dir_all(&config_dir)?;

    // Generate keypair
    let kp = load_or_create_keypair()?;
    let pubkey = kith_common::credential::pubkey_to_hex(&kp.public_key_bytes());

    // Generate mesh identifier
    let mesh_id = format!("kith-{}", &uuid::Uuid::new_v4().to_string()[..8]);

    // Write default config if it doesn't exist
    let config_path = config_dir.join("config.toml");
    if config_path.exists() {
        eprintln!("config already exists: {}", config_path.display());
    } else {
        let config_content = format!(
            r#"# kith configuration
# Generated by kith --init

[inference]
backend = "openai-compatible"
endpoint = "http://localhost:8000/v1"
model = "default"
# api_key_env = "OPENAI_API_KEY"

# For Anthropic:
# backend = "anthropic"
# model = "claude-sonnet-4-20250514"
# api_key_env = "ANTHROPIC_API_KEY"

[mesh]
identifier = "{mesh_id}"
wireguard_interface = "kith0"
listen_port = 51820
mesh_cidr = "{mesh_id}"
nostr_relays = ["wss://relay.damus.io", "wss://nos.lol", "wss://relay.nostr.band"]
"#
        );
        std::fs::write(&config_path, config_content)?;
        eprintln!("config written: {}", config_path.display());
    }

    eprintln!();
    eprintln!("kith initialized:");
    eprintln!("  identity:  {pubkey}");
    eprintln!("  mesh id:   {mesh_id}");
    eprintln!("  config:    {}", config_path.display());
    eprintln!();
    eprintln!("To add this identity to a daemon, add to the daemon's config or env:");
    eprintln!("  KITH_USERS=\"{pubkey}:ops\"");
    eprintln!();
    eprintln!("Start the shell:");
    eprintln!("  kith");

    Ok(())
}

async fn run_status() -> Result<(), Box<dyn std::error::Error>> {
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "unknown".into());

    println!("kith status");
    println!("===========");
    println!("  machine:  {hostname}");
    println!(
        "  os:       {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );

    // Identity
    let key_path = dirs_next::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("kith")
        .join("identity.key");
    if key_path.exists() {
        let kp = load_or_create_keypair()?;
        let pubkey = kith_common::credential::pubkey_to_hex(&kp.public_key_bytes());
        println!("  identity: {}", &pubkey[..16]);
    } else {
        println!("  identity: not initialized (run kith --init)");
    }

    // Config
    let config = kith_common::config::KithConfig::load(None).unwrap_or(None);
    if let Some(ref cfg) = config {
        if let Some(ref inf) = cfg.inference {
            println!("  backend:  {} ({})", inf.backend, inf.model);
        }
        println!("  mesh id:  {}", cfg.mesh.identifier);
        println!("  relays:   {}", cfg.mesh.nostr_relays.len());
    } else {
        println!("  config:   none (run kith --init)");
    }

    // Try connecting to daemon
    let daemon_addr = std::env::var("KITH_DAEMON").ok();
    if let Some(ref addr) = daemon_addr {
        let kp = load_or_create_keypair()?;
        match kith_shell::daemon_client::DaemonClient::connect(addr, kp).await {
            Ok(mut client) => match client.query().await {
                Ok(state) => println!("  daemon:   connected ({addr}) — {state}"),
                Err(e) => println!("  daemon:   connected ({addr}) — query failed: {e}"),
            },
            Err(e) => println!("  daemon:   unreachable ({addr}): {e}"),
        }
    } else {
        println!("  daemon:   not configured (set KITH_DAEMON=host:port)");
    }

    // Sync peers
    let peers = std::env::var("KITH_SYNC_PEERS").unwrap_or_default();
    if peers.is_empty() {
        println!("  sync:     no peers configured");
    } else {
        let count = peers.split(',').filter(|s| !s.trim().is_empty()).count();
        println!("  sync:     {count} peer(s) configured");
    }

    Ok(())
}
