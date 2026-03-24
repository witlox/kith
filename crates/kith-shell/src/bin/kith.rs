//! kith — intent-driven distributed shell.
//!
//! Usage:
//!   kith                          # interactive mode
//!   kith "deploy to staging"      # single command mode
//!   kith --backend anthropic      # override backend
//!   kith --daemon host:port       # connect to remote daemon

use std::io::{self, BufRead, Write};

use kith_common::credential::Keypair;
use kith_common::inference::InferenceBackend;
use kith_shell::agent::{Agent, AgentOutput};
use kith_shell::inference::{AnthropicBackend, OpenAiCompatBackend};
use kith_shell::prompt::build_system_prompt;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();

    // Parse basic flags
    let backend_type = find_flag(&args, "--backend")
        .or_else(|| std::env::var("KITH_BACKEND").ok())
        .unwrap_or_else(|| "openai-compatible".into());

    let endpoint = find_flag(&args, "--endpoint")
        .or_else(|| std::env::var("KITH_ENDPOINT").ok())
        .unwrap_or_else(|| "http://localhost:8000/v1".into());

    let model = find_flag(&args, "--model")
        .or_else(|| std::env::var("KITH_MODEL").ok())
        .unwrap_or_else(|| "default".into());

    let daemon_addr = find_flag(&args, "--daemon")
        .or_else(|| std::env::var("KITH_DAEMON").ok());

    // Build inference backend
    let backend: Box<dyn InferenceBackend> = match backend_type.as_str() {
        "anthropic" => {
            let api_key = std::env::var("ANTHROPIC_API_KEY")
                .map_err(|_| "ANTHROPIC_API_KEY env var not set")?;
            let model = if model == "default" { "claude-sonnet-4-20250514".into() } else { model };
            Box::new(AnthropicBackend::new(api_key, model))
        }
        _ => {
            let api_key = std::env::var("OPENAI_API_KEY").ok();
            Box::new(OpenAiCompatBackend::new(endpoint, model, api_key))
        }
    };

    // Build system prompt
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "unknown".into());
    let os_info = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    let prompt = build_system_prompt(&hostname, &os_info, "", None);

    let mut agent = Agent::new(backend, prompt);

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
    let non_flag_args: Vec<&str> = args.iter()
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

    // Interactive mode
    eprintln!("kith shell — {} on {hostname}", agent.backend_name());
    eprintln!("type naturally or use commands directly. run: prefix for escape hatch.\n");

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("kith> ");
        stdout.flush()?;

        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            break; // EOF
        }

        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        if input == "exit" || input == "quit" {
            break;
        }

        let output = agent.process(input).await;
        print_output(&output);
    }

    Ok(())
}

fn print_output(output: &AgentOutput) {
    match output {
        AgentOutput::PassThrough { stdout, stderr, exit_code: _, .. } => {
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
        let bytes = std::fs::read(&key_path)?;
        let secret: [u8; 32] = bytes.as_slice().try_into()
            .map_err(|_| "invalid key file: expected 32 bytes")?;
        Ok(Keypair::from_secret(&secret))
    } else {
        let kp = Keypair::generate();
        if let Some(parent) = key_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&key_path, kp.secret_bytes())?;
        let pubkey = kith_common::credential::pubkey_to_hex(&kp.public_key_bytes());
        eprintln!("generated new identity: {pubkey}");
        eprintln!("key stored at: {}", key_path.display());
        Ok(kp)
    }
}
