//! Transport anchors (task brief step 1): spawn/token deletion, NDJSON round
//! trip, timeout + restart, mid-request disconnect + restart, error mapping,
//! and shutdown. Every test is driven by the scripted fake helper in
//! `tests/common`; no Swift helper or real app is involved and all injected
//! timeouts are short.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

use ade_computer::protocol::ComputerErrorCode;
use ade_computer::transport::{helper_request_timeout, HelperTransport};
use common::{FakeHelper, FakeResponse};
use serde_json::json;

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777
}

#[test]
fn helper_request_timeout_defaults_to_sixty_seconds() {
    assert_eq!(helper_request_timeout(), Duration::from_secs(60));
}

#[test]
fn start_spawns_helper_with_expected_argv_and_deletes_token_after_connect() {
    let fake = FakeHelper::start();
    let transport = HelperTransport::start(fake.helper_app(), Duration::from_secs(2))
        .expect("transport starts");
    let spawn = fake.wait_for_spawn(0);

    assert_eq!(spawn.args.len(), 4, "args: {:?}", spawn.args);
    assert_eq!(spawn.args[0], "--agent");
    assert_eq!(spawn.args[1], spawn.socket_path.to_string_lossy());
    assert_eq!(spawn.args[2], "--token-file");
    assert_eq!(spawn.args[3], spawn.token_path.to_string_lossy());

    let directory_name = spawn
        .directory
        .file_name()
        .expect("socket directory name")
        .to_string_lossy();
    assert!(
        directory_name.starts_with("orcinus-computer-use-"),
        "unexpected socket directory: {directory_name}"
    );
    assert_eq!(mode(&spawn.directory), 0o700, "socket directory mode");
    assert_eq!(mode(&spawn.socket_path), 0o600, "socket mode");

    assert_eq!(spawn.token.len(), 48, "token is 48 hex chars");
    assert!(
        spawn.token.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "token must be hex: {}",
        spawn.token
    );
    assert!(
        !spawn.token_path.exists(),
        "token file must be deleted once the socket connection is established"
    );

    drop(transport);
}

#[test]
fn request_round_trips_and_envelope_carries_token() {
    let fake = FakeHelper::start();
    let mut transport = HelperTransport::start(fake.helper_app(), Duration::from_secs(2))
        .expect("transport starts");
    let spawn = fake.wait_for_spawn(0);

    let result = transport
        .request("handshake", &json!({}))
        .expect("handshake round trip");
    assert_eq!(result, json!({"method": "handshake", "params": {}}));

    let request = fake.next_request(Duration::from_secs(2));
    assert_eq!(request.id, 1);
    assert_eq!(request.method, "handshake");
    assert_eq!(request.params, json!({}));
    assert_eq!(request.token.as_deref(), Some(spawn.token.as_str()));
    assert!(
        request.raw.ends_with('\n'),
        "request must be a single NDJSON line: {:?}",
        request.raw
    );
    assert_eq!(request.raw.matches('\n').count(), 1);

    let envelope: serde_json::Value = serde_json::from_str(request.raw.trim_end()).expect("json");
    assert_eq!(envelope["id"], 1);
    assert_eq!(envelope["method"], "handshake");
    assert_eq!(envelope["params"], json!({}));
    assert_eq!(envelope["token"], spawn.token);

    transport.shutdown();
}

#[test]
fn request_timeout_kills_helper_and_next_request_uses_a_new_process() {
    let fake = FakeHelper::start();
    fake.respond_with([FakeResponse::Delay(Duration::from_millis(400))]);
    let mut transport = HelperTransport::start(fake.helper_app(), Duration::from_millis(100))
        .expect("transport starts");
    let first = fake.wait_for_spawn(0);

    let error = transport
        .request("click", &json!({"app": "Finder"}))
        .expect_err("request must time out");
    assert_eq!(error.code, ComputerErrorCode::ActionTimeout, "{error:?}");

    assert!(
        fake.wait_for_termination(first.pid, Duration::from_secs(3)),
        "helper process must receive SIGTERM after a timeout"
    );
    assert!(
        fake.wait_for_process_exit(first.pid, Duration::from_secs(3)),
        "helper process must exit after a timeout"
    );
    assert!(
        FakeHelper::wait_for_removed(&first.directory, Duration::from_secs(3)),
        "socket directory must be removed on restart"
    );

    let result = transport
        .request("handshake", &json!({}))
        .expect("second request succeeds on a fresh helper");
    assert_eq!(result, json!({"method": "handshake", "params": {}}));
    let second = fake.wait_for_spawn(1);
    assert_ne!(
        second.pid, first.pid,
        "restart must use a new helper process"
    );

    transport.shutdown();
}

#[test]
fn helper_disconnect_fails_the_request_and_restart_recovers() {
    let fake = FakeHelper::start();
    fake.respond_with([FakeResponse::Disconnect]);
    let mut transport = HelperTransport::start(fake.helper_app(), Duration::from_secs(2))
        .expect("transport starts");
    let first = fake.wait_for_spawn(0);

    let error = transport
        .request("handshake", &json!({}))
        .expect_err("closed connection must fail the request");
    assert_eq!(
        error.code,
        ComputerErrorCode::AccessibilityError,
        "{error:?}"
    );
    assert!(
        fake.wait_for_termination(first.pid, Duration::from_secs(3)),
        "helper process must be terminated after a transport failure"
    );

    let result = transport
        .request("handshake", &json!({}))
        .expect("next request must restart the helper and succeed");
    assert_eq!(result, json!({"method": "handshake", "params": {}}));
    let second = fake.wait_for_spawn(1);
    assert_ne!(
        second.pid, first.pid,
        "restart must use a new helper process"
    );

    transport.shutdown();
}

#[test]
fn helper_error_envelope_maps_to_computer_error_and_keeps_the_helper_alive() {
    let fake = FakeHelper::start();
    fake.respond_with([FakeResponse::error(
        "permission_denied",
        "no accessibility permission",
    )]);
    let mut transport = HelperTransport::start(fake.helper_app(), Duration::from_secs(2))
        .expect("transport starts");
    let spawn = fake.wait_for_spawn(0);

    let error = transport
        .request("click", &json!({"app": "Finder"}))
        .expect_err("error response must be surfaced");
    assert_eq!(error.code, ComputerErrorCode::PermissionDenied);
    assert_eq!(error.message, "no accessibility permission");

    // A well-formed error envelope is not a transport failure: the next
    // request must be served by the same process over the same connection.
    let result = transport
        .request("handshake", &json!({}))
        .expect("same helper keeps serving after an error envelope");
    assert_eq!(result, json!({"method": "handshake", "params": {}}));

    let first = fake.next_request(Duration::from_secs(2));
    let second = fake.next_request(Duration::from_secs(2));
    assert_eq!(first.method, "click");
    assert_eq!(second.method, "handshake");
    assert_eq!(
        first.connection_id, second.connection_id,
        "an error envelope must not reconnect"
    );
    assert_eq!(fake.spawns().len(), 1, "an error envelope must not respawn");
    assert!(
        fake.terminated_pids().is_empty(),
        "an error envelope must not SIGTERM the helper: {:?}",
        fake.terminated_pids()
    );
    assert!(
        common::process_alive(spawn.pid),
        "helper process must stay alive after an error envelope"
    );

    transport.shutdown();
}

#[test]
fn stale_responses_with_other_ids_are_ignored() {
    let fake = FakeHelper::start();
    fake.respond_with([FakeResponse::StaleThen(json!("fresh"))]);
    let mut transport = HelperTransport::start(fake.helper_app(), Duration::from_secs(2))
        .expect("transport starts");

    let result = transport
        .request("handshake", &json!({}))
        .expect("request resolves with the matching response");
    assert_eq!(result, json!("fresh"));

    transport.shutdown();
}

#[test]
fn helper_rejects_a_mismatched_token() {
    let fake = FakeHelper::start();
    let mut transport = HelperTransport::start(fake.helper_app(), Duration::from_secs(2))
        .expect("transport starts");
    fake.wait_for_spawn(0);
    fake.set_expected_token("not-the-real-token");

    let error = transport
        .request("handshake", &json!({}))
        .expect_err("mismatched token must be rejected");
    assert_eq!(error.code, ComputerErrorCode::PermissionDenied, "{error:?}");

    drop(transport);
}

#[test]
fn shutdown_terminates_helper_and_removes_socket_directory() {
    let fake = FakeHelper::start();
    let mut transport = HelperTransport::start(fake.helper_app(), Duration::from_secs(2))
        .expect("transport starts");
    let spawn = fake.wait_for_spawn(0);

    transport.shutdown();

    let terminate = fake.next_request(Duration::from_secs(2));
    assert_eq!(terminate.method, "terminate");
    assert_eq!(terminate.token.as_deref(), Some(spawn.token.as_str()));
    assert!(
        fake.wait_for_termination(spawn.pid, Duration::from_secs(3)),
        "helper process must receive SIGTERM on shutdown"
    );
    assert!(
        fake.wait_for_process_exit(spawn.pid, Duration::from_secs(3)),
        "helper process must exit on shutdown"
    );
    assert!(
        FakeHelper::wait_for_removed(&spawn.directory, Duration::from_secs(3)),
        "socket directory must be removed on shutdown"
    );

    // Shutdown is idempotent.
    transport.shutdown();
}
