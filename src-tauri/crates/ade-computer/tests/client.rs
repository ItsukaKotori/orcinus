//! Client anchors (task brief step 1): protocol gate, capabilities cache,
//! single-flight serialization, validation gating and verification
//! normalization. Every test is driven by the scripted fake helper in
//! `tests/common`; no Swift helper or real app is involved.

mod common;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use ade_computer::client::ProviderClient;
use ade_computer::protocol::ComputerErrorCode;
use ade_computer::provider::ComputerProvider;
use common::{FakeHelper, FakeRequest, FakeResponse};
use serde_json::json;

const TEST_TIMEOUT: Duration = Duration::from_secs(5);
const SILENCE: Duration = Duration::from_millis(100);

fn test_client(helper: &FakeHelper) -> ProviderClient {
    ProviderClient::new(helper.helper_app().to_path_buf(), TEST_TIMEOUT)
}

fn handshake_ok() -> FakeResponse {
    FakeResponse::Result(json!({"protocolVersion": 1, "supports": {"actions": {}}}))
}

fn expect_method(helper: &FakeHelper, method: &str) -> FakeRequest {
    let request = helper.next_request(TEST_TIMEOUT);
    assert_eq!(request.method, method, "raw: {}", request.raw);
    request
}

fn expect_no_requests(helper: &FakeHelper) {
    let extra = helper.requests_rx().recv_timeout(SILENCE);
    assert!(
        extra.is_err(),
        "unexpected request: {}",
        extra.expect("checked").raw
    );
}

#[test]
fn handshake_protocol_version_two_is_provider_incompatible() {
    let helper = FakeHelper::start();
    helper.respond_with([FakeResponse::Result(
        json!({"protocolVersion": 2, "supports": {}}),
    )]);
    let mut client = test_client(&helper);

    let error = client.ensure_ready().unwrap_err();
    assert_eq!(error.code, ComputerErrorCode::ProviderIncompatible);
    assert!(
        error.message.contains("required protocol 1"),
        "message was: {}",
        error.message
    );
    assert!(
        error.message.contains('2'),
        "message was: {}",
        error.message
    );

    expect_method(&helper, "handshake");
    expect_no_requests(&helper);
    assert_eq!(helper.spawns().len(), 1, "incompatibility must not respawn");
}

#[test]
fn incompatible_handshake_is_not_cached_and_the_next_attempt_retries() {
    let helper = FakeHelper::start();
    helper.respond_with([
        FakeResponse::Result(json!({"protocolVersion": 2})),
        handshake_ok(),
    ]);
    let mut client = test_client(&helper);

    assert_eq!(
        client.ensure_ready().unwrap_err().code,
        ComputerErrorCode::ProviderIncompatible
    );
    let capabilities = client.ensure_ready().expect("second handshake succeeds");
    assert_eq!(capabilities["protocolVersion"], json!(1));

    expect_method(&helper, "handshake");
    expect_method(&helper, "handshake");
    expect_no_requests(&helper);
}

#[test]
fn handshake_without_protocol_version_is_provider_incompatible() {
    let helper = FakeHelper::start();
    helper.respond_with([FakeResponse::Result(json!({"supports": {}}))]);
    let mut client = test_client(&helper);

    let error = client.ensure_ready().unwrap_err();
    assert_eq!(error.code, ComputerErrorCode::ProviderIncompatible);
    assert!(
        error.message.contains("protocol"),
        "message was: {}",
        error.message
    );
}

#[test]
fn ensure_ready_caches_the_handshake_and_call_normalizes_get_app_state() {
    let helper = FakeHelper::start();
    helper.respond_with([
        handshake_ok(),
        FakeResponse::Result(json!({
            "app": "Finder",
            "action": {"path": "synthetic", "elementIndex": 2},
        })),
    ]);
    let mut client = test_client(&helper);

    let capabilities = client.ensure_ready().expect("handshake succeeds");
    assert_eq!(capabilities["protocolVersion"], json!(1));
    assert_eq!(
        client.ensure_ready().expect("cached handshake"),
        capabilities,
        "handshake result is cached"
    );

    let result = client
        .call("getAppState", json!({"app": "Finder", "windowIndex": 0}))
        .expect("getAppState succeeds");
    assert_eq!(
        result["action"]["verification"],
        json!({"state": "unverified", "reason": "synthetic_input"})
    );
    assert_eq!(result["action"]["path"], json!("synthetic"));

    expect_method(&helper, "handshake");
    let request = expect_method(&helper, "getAppState");
    assert_eq!(request.params, json!({"app": "Finder", "windowIndex": 0}));
    expect_no_requests(&helper);
}

#[test]
fn capabilities_trait_method_returns_the_cached_handshake_result() {
    let helper = FakeHelper::start();
    helper.respond_with([handshake_ok()]);
    let mut client = test_client(&helper);
    let provider: &mut dyn ComputerProvider = &mut client;

    let first = provider.capabilities().expect("capabilities");
    let second = provider.capabilities().expect("cached capabilities");
    assert_eq!(first, second);
    assert_eq!(first["protocolVersion"], json!(1));

    expect_method(&helper, "handshake");
    expect_no_requests(&helper);
}

#[test]
fn invalid_click_params_fail_before_the_helper_sees_a_request() {
    let helper = FakeHelper::start();
    helper.respond_with([handshake_ok()]);
    let mut client = test_client(&helper);
    client.ensure_ready().expect("handshake succeeds");

    let error = client.call("click", json!({"app": "Finder"})).unwrap_err();
    assert_eq!(error.code, ComputerErrorCode::InvalidArgument);
    assert_eq!(error.message, "Click requires elementIndex or both x and y");

    expect_method(&helper, "handshake");
    expect_no_requests(&helper);
}

#[test]
fn helper_error_responses_pass_through_without_restarting_the_helper() {
    let helper = FakeHelper::start();
    helper.respond_with([
        handshake_ok(),
        FakeResponse::error("element_not_found", "no such element"),
        FakeResponse::Result(json!({"action": {"path": "clipboard"}})),
    ]);
    let mut client = test_client(&helper);
    client.ensure_ready().expect("handshake succeeds");

    let error = client
        .call("click", json!({"app": "Finder", "elementIndex": 9}))
        .unwrap_err();
    assert_eq!(error.code, ComputerErrorCode::ElementNotFound);
    assert_eq!(error.message, "no such element");

    let result = client
        .call("click", json!({"app": "Finder", "elementIndex": 9}))
        .expect("second click succeeds");
    assert_eq!(
        result["action"]["verification"],
        json!({"state": "unverified", "reason": "clipboard_paste"})
    );

    let handshake = expect_method(&helper, "handshake");
    let first_click = expect_method(&helper, "click");
    let second_click = expect_method(&helper, "click");
    assert_eq!(first_click.connection_id, handshake.connection_id);
    assert_eq!(second_click.connection_id, handshake.connection_id);
    assert_eq!(helper.spawns().len(), 1, "error envelope must not restart");
}

#[test]
fn concurrent_calls_are_serialized_on_a_single_connection() {
    let helper = FakeHelper::start();
    helper.respond_with([
        handshake_ok(),
        FakeResponse::Delay(Duration::from_millis(300)),
    ]);
    let client = Arc::new(Mutex::new(test_client(&helper)));
    client.lock().unwrap().ensure_ready().expect("handshake");

    let first = {
        let client = Arc::clone(&client);
        thread::spawn(move || {
            client
                .lock()
                .unwrap()
                .call("click", json!({"app": "Finder", "elementIndex": 1}))
        })
    };
    thread::sleep(Duration::from_millis(50));
    let second = {
        let client = Arc::clone(&client);
        thread::spawn(move || {
            client
                .lock()
                .unwrap()
                .call("click", json!({"app": "Finder", "elementIndex": 2}))
        })
    };

    let first_result = first.join().expect("first thread").expect("first click");
    let second_result = second.join().expect("second thread").expect("second click");
    assert_eq!(first_result["params"]["elementIndex"], json!(1));
    assert_eq!(second_result["params"]["elementIndex"], json!(2));

    let handshake = expect_method(&helper, "handshake");
    let first_click = expect_method(&helper, "click");
    let second_click = expect_method(&helper, "click");
    assert_eq!(first_click.params["elementIndex"], json!(1));
    assert_eq!(second_click.params["elementIndex"], json!(2));
    assert!(
        first_click.id < second_click.id,
        "request ids must increase"
    );
    assert_eq!(first_click.connection_id, second_click.connection_id);
    assert_eq!(first_click.connection_id, handshake.connection_id);
    let gap = second_click
        .received_at
        .duration_since(first_click.received_at);
    assert!(
        gap >= Duration::from_millis(250),
        "second request was handled during the first: {gap:?}"
    );
    assert_eq!(
        helper.spawns().len(),
        1,
        "serialized calls must not respawn"
    );
}

#[test]
fn shutdown_terminates_the_helper_and_clears_the_capabilities_cache() {
    let helper = FakeHelper::start();
    helper.respond_with([handshake_ok(), handshake_ok()]);
    let mut client = test_client(&helper);
    client.ensure_ready().expect("handshake succeeds");
    let first_spawn = helper.last_spawn();

    client.shutdown();
    assert!(
        helper.wait_for_termination(first_spawn.pid, TEST_TIMEOUT),
        "helper was not terminated"
    );
    assert!(
        FakeHelper::wait_for_removed(&first_spawn.directory, TEST_TIMEOUT),
        "socket directory was not removed"
    );

    client
        .ensure_ready()
        .expect("fresh handshake after shutdown");
    expect_method(&helper, "handshake");
    expect_method(&helper, "terminate");
    expect_method(&helper, "handshake");
    assert_eq!(helper.spawns().len(), 2, "shutdown cleared the cache");

    client.shutdown();
    client.shutdown();
}
