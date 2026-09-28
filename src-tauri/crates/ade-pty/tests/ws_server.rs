use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

const TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn connects_with_valid_token_and_echoes() {
    let (port, token, _server) = ade_pty::test_support::start_echo_server();
    let (mut ws, _resp) = connect_async(format!("ws://127.0.0.1:{port}/pty/test-id?token={token}"))
        .await
        .expect("handshake with valid token");
    ws.send(Message::binary(b"hello".as_slice()))
        .await
        .expect("send binary");
    let echoed = tokio::time::timeout(TIMEOUT, async {
        loop {
            match ws.next().await {
                None => panic!("ws stream ended without echo"),
                Some(Err(e)) => panic!("ws stream error: {e}"),
                Some(Ok(Message::Binary(bytes))) => break bytes,
                Some(Ok(Message::Close(frame))) => panic!("server closed early: {frame:?}"),
                Some(Ok(_)) => {}
            }
        }
    })
    .await
    .expect("timed out waiting for echo");
    assert_eq!(echoed.as_ref(), b"hello");
}

#[tokio::test]
async fn rejects_bad_token() {
    let (port, _token, _server) = ade_pty::test_support::start_echo_server();
    let connection = connect_async(format!(
        "ws://127.0.0.1:{port}/pty/test-id?token=wrong-token"
    ))
    .await;
    let mut ws = match connection {
        Ok((ws, _resp)) => ws,
        // 握手层面直接被拒也算拒绝；本实现预期是握手成功后 close(1008)。
        Err(_) => return,
    };
    let closed = tokio::time::timeout(TIMEOUT, async {
        while let Some(msg) = ws.next().await {
            match msg {
                Ok(Message::Close(_)) | Err(_) => return,
                Ok(Message::Binary(_)) => panic!("received echo despite bad token"),
                Ok(_) => {}
            }
        }
    })
    .await;
    assert!(
        closed.is_ok(),
        "server did not close the bad-token connection within {TIMEOUT:?}"
    );
}
