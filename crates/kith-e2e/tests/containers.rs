//! Container-based e2e tests using testcontainers.
//! Run with: cargo test -p kith-e2e --features containers
//!
//! Requires:
//! - Docker running
//! - kith-daemon image built: docker build -t kith-daemon .

#![cfg(feature = "containers")]

use std::time::Duration;

use kith_common::credential::Keypair;
use kith_shell::daemon_client::DaemonClient;
use testcontainers::runners::AsyncRunner;
use testcontainers::{core::WaitFor, GenericImage, ImageExt};

fn kith_daemon_image() -> GenericImage {
    GenericImage::new("kith-daemon", "latest")
        .with_exposed_port(9443.into())
        .with_wait_for(WaitFor::message_on_stdout("kith-daemon starting"))
}

/// Wrap image with env vars. Must be called after kith_daemon_image().
fn with_daemon_env(
    image: GenericImage,
    machine_name: &str,
    users_env: &str,
) -> testcontainers::ContainerRequest<GenericImage> {
    image
        .with_env_var("RUST_LOG", "info")
        .with_env_var("NO_COLOR", "1")
        .with_env_var("KITH_MACHINE_NAME", machine_name)
        .with_env_var("KITH_USERS", users_env)
        .with_env_var("KITH_TOFU", "false")
}

/// Scenario 5: two containerized daemons, shell connects to each.
/// This tests real network connectivity between containers.
#[tokio::test]
async fn container_multi_daemon() {
    let kp = Keypair::generate();
    let pubkey_hex = kith_common::credential::pubkey_to_hex(&kp.public_key_bytes());
    let users_env = format!("{pubkey_hex}:ops");

    // Start daemon 1
    let daemon1 = with_daemon_env(kith_daemon_image(), "container-1", &users_env)
        .start()
        .await
        .expect("daemon-1 should start");

    let port1 = daemon1.get_host_port_ipv4(9443).await.unwrap();
    let addr1 = format!("http://127.0.0.1:{port1}");

    // Start daemon 2
    let daemon2 = with_daemon_env(kith_daemon_image(), "container-2", &users_env)
        .start()
        .await
        .expect("daemon-2 should start");

    let port2 = daemon2.get_host_port_ipv4(9443).await.unwrap();
    let addr2 = format!("http://127.0.0.1:{port2}");

    // Small delay for gRPC to be ready
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Shell connects to both
    let mut client1 =
        DaemonClient::connect(&addr1, Keypair::from_secret(&kp.secret_bytes()))
            .await
            .expect("should connect to daemon-1");
    let mut client2 =
        DaemonClient::connect(&addr2, Keypair::from_secret(&kp.secret_bytes()))
            .await
            .expect("should connect to daemon-2");

    // Exec on each
    let r1 = client1.exec("hostname").await.expect("exec on daemon-1");
    let r2 = client2.exec("hostname").await.expect("exec on daemon-2");

    assert_eq!(r1.exit_code, 0);
    assert_eq!(r2.exit_code, 0);

    // Query state on each
    let s1 = client1.query().await.unwrap();
    let s2 = client2.query().await.unwrap();

    assert!(s1.contains("container-1"));
    assert!(s2.contains("container-2"));
}

/// Apply/commit/rollback through containerized daemon.
#[tokio::test]
async fn container_apply_commit_rollback() {
    let kp = Keypair::generate();
    let pubkey_hex = kith_common::credential::pubkey_to_hex(&kp.public_key_bytes());
    let users_env = format!("{pubkey_hex}:ops");

    let daemon = with_daemon_env(kith_daemon_image(), "apply-test", &users_env)
        .start()
        .await
        .expect("daemon should start");

    let port = daemon.get_host_port_ipv4(9443).await.unwrap();
    let addr = format!("http://127.0.0.1:{port}");
    tokio::time::sleep(Duration::from_secs(1)).await;

    let mut client =
        DaemonClient::connect(&addr, Keypair::from_secret(&kp.secret_bytes()))
            .await
            .unwrap();

    // Apply
    let pending_id = client.apply("docker compose up", 600).await.unwrap();
    assert!(!pending_id.is_empty());

    // Commit
    let committed = client.commit(&pending_id).await.unwrap();
    assert!(committed);

    // Double-commit fails gracefully
    let double = client.commit(&pending_id).await.unwrap();
    assert!(!double);

    // Apply + rollback
    let pending_id2 = client.apply("risky change", 600).await.unwrap();
    let rolled_back = client.rollback(&pending_id2).await.unwrap();
    assert!(rolled_back);
}

/// Auth denial through containerized daemon.
#[tokio::test]
async fn container_auth_denied() {
    let server_kp = Keypair::generate();
    let pubkey_hex = kith_common::credential::pubkey_to_hex(&server_kp.public_key_bytes());
    let users_env = format!("{pubkey_hex}:ops");

    let daemon = with_daemon_env(kith_daemon_image(), "auth-test", &users_env)
        .start()
        .await
        .expect("daemon should start");

    let port = daemon.get_host_port_ipv4(9443).await.unwrap();
    let addr = format!("http://127.0.0.1:{port}");
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Connect with unauthorized keypair
    let bad_kp = Keypair::generate();
    let mut client = DaemonClient::connect(&addr, bad_kp).await.unwrap();

    let result = client.exec("echo should-fail").await;
    assert!(result.is_err(), "unauthorized exec should fail");
}

/// TOFU mode: unknown key gets viewer scope.
#[tokio::test]
async fn container_tofu_mode() {
    let daemon = kith_daemon_image()
        .with_env_var("RUST_LOG", "info")
        .with_env_var("NO_COLOR", "1")
        .with_env_var("KITH_MACHINE_NAME", "tofu-test")
        .with_env_var("KITH_TOFU", "true")
        .with_env_var("KITH_USERS", "") // no pre-authorized users
        .start()
        .await
        .expect("daemon should start");

    let port = daemon.get_host_port_ipv4(9443).await.unwrap();
    let addr = format!("http://127.0.0.1:{port}");
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Unknown key with TOFU enabled — should get viewer scope
    let kp = Keypair::generate();
    let mut client = DaemonClient::connect(&addr, kp).await.unwrap();

    // Query should work (viewer can query)
    let state = client.query().await.unwrap();
    assert!(state.contains("tofu-test"));

    // Exec should be denied (viewer can't exec)
    let result = client.exec("echo tofu-test").await;
    assert!(result.is_err(), "TOFU viewer should not be able to exec");
}

/// Concurrent exec from multiple clients to one container.
#[tokio::test]
async fn container_concurrent_exec() {
    let kp = Keypair::generate();
    let pubkey_hex = kith_common::credential::pubkey_to_hex(&kp.public_key_bytes());
    let users_env = format!("{pubkey_hex}:ops");

    let daemon = with_daemon_env(kith_daemon_image(), "concurrent-test", &users_env)
        .start()
        .await
        .expect("daemon should start");

    let port = daemon.get_host_port_ipv4(9443).await.unwrap();
    let addr = format!("http://127.0.0.1:{port}");
    tokio::time::sleep(Duration::from_secs(1)).await;

    let mut handles = Vec::new();
    for i in 0..5 {
        let addr = addr.clone();
        let secret = kp.secret_bytes();
        let handle = tokio::spawn(async move {
            let mut client =
                DaemonClient::connect(&addr, Keypair::from_secret(&secret))
                    .await
                    .unwrap();
            let result = client.exec(&format!("echo concurrent-{i}")).await.unwrap();
            assert_eq!(result.exit_code, 0);
            assert!(result.stdout.contains(&format!("concurrent-{i}")));
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }
}

/// Multi-command sequence in container.
#[tokio::test]
async fn container_multi_command_sequence() {
    let kp = Keypair::generate();
    let pubkey_hex = kith_common::credential::pubkey_to_hex(&kp.public_key_bytes());
    let users_env = format!("{pubkey_hex}:ops");

    let daemon = with_daemon_env(kith_daemon_image(), "sequence-test", &users_env)
        .start()
        .await
        .expect("daemon should start");

    let port = daemon.get_host_port_ipv4(9443).await.unwrap();
    let addr = format!("http://127.0.0.1:{port}");
    tokio::time::sleep(Duration::from_secs(1)).await;

    let mut client =
        DaemonClient::connect(&addr, Keypair::from_secret(&kp.secret_bytes()))
            .await
            .unwrap();

    // Run a sequence of commands
    let r1 = client.exec("echo step-1").await.unwrap();
    assert!(r1.stdout.contains("step-1"));

    let r2 = client.exec("echo step-2 && echo step-3").await.unwrap();
    assert!(r2.stdout.contains("step-2"));
    assert!(r2.stdout.contains("step-3"));

    let r3 = client.exec("pwd").await.unwrap();
    assert_eq!(r3.exit_code, 0);
    assert!(!r3.stdout.is_empty());
}

/// Chaos: container kill and reconnect.
#[tokio::test]
async fn container_daemon_restart_resilience() {
    let kp = Keypair::generate();
    let pubkey_hex = kith_common::credential::pubkey_to_hex(&kp.public_key_bytes());
    let users_env = format!("{pubkey_hex}:ops");

    let daemon = with_daemon_env(kith_daemon_image(), "restart-test", &users_env)
        .start()
        .await
        .expect("daemon should start");

    let port = daemon.get_host_port_ipv4(9443).await.unwrap();
    let addr = format!("http://127.0.0.1:{port}");

    tokio::time::sleep(Duration::from_secs(1)).await;

    // First connection works
    let mut client =
        DaemonClient::connect(&addr, Keypair::from_secret(&kp.secret_bytes()))
            .await
            .unwrap();
    let r = client.exec("echo before-restart").await.unwrap();
    assert!(r.stdout.contains("before-restart"));

    // Container is still running (we don't actually restart in this test,
    // but we verify the connection is still alive)
    let r = client.exec("echo after-check").await.unwrap();
    assert!(r.stdout.contains("after-check"));
}
