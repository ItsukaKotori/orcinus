//! NDJSON frame protocol between the Rust host process and the Node plugin
//! worker. Field names, discriminants, and bounds mirror
//! `orca:src/shared/plugins/plugin-host-protocol.ts` verbatim; only the
//! transport is adapted (the oracle uses Node `fork` IPC with advanced
//! serialization, this fork uses one JSON object per line over stdio).
//!
//! Frames are untrusted input: the child runs third-party code, so every
//! read is a fallible parse. Text fields the oracle bounds at 8192 bytes
//! (`commandResult.error`, `log.message`, `fatal.error`, and — so a host
//! error can never blow the frame cap — `hostResult.error`) are truncated on
//! the way in and on the way out instead of being rejected.

use std::io::{BufRead, Write};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

use crate::types::{CONTENT_TOO_LARGE, INVALID_PARAMS, IO_ERROR};
use crate::PluginError;

/// Maximum serialized frame size, excluding the trailing newline.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Bound on error/message text carried inside frames (oracle zod `.max(8192)`).
pub const TEXT_MAX_BYTES: usize = 8192;

/// Log severity carried by [`ChildFrame::Log`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// Host → worker frames.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ParentFrame {
    #[serde(rename_all = "camelCase")]
    Init {
        plugin_id: String,
        plugin_root: String,
        main_entry: String,
        granted_capabilities: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    InvokeCommand {
        call_id: u64,
        command_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args: Option<Value>,
    },
    #[serde(rename_all = "camelCase")]
    DeliverEvent {
        event_id: u64,
        event: String,
        payload: Value,
    },
    #[serde(rename_all = "camelCase")]
    HostResult {
        call_id: u64,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error_code: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            serialize_with = "bounded_text::serialize_option",
            deserialize_with = "bounded_text::deserialize_option"
        )]
        error: Option<String>,
    },
    Shutdown,
}

/// Worker → host frames.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ChildFrame {
    Ready {
        commands: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    CommandResult {
        call_id: u64,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<Value>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            serialize_with = "bounded_text::serialize_option",
            deserialize_with = "bounded_text::deserialize_option"
        )]
        error: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    EventAck {
        event_id: u64,
    },
    #[serde(rename_all = "camelCase")]
    HostCall {
        call_id: u64,
        method: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        params: Option<Value>,
    },
    Log {
        level: LogLevel,
        #[serde(
            serialize_with = "bounded_text::serialize",
            deserialize_with = "bounded_text::deserialize"
        )]
        message: String,
    },
    Fatal {
        #[serde(
            serialize_with = "bounded_text::serialize",
            deserialize_with = "bounded_text::deserialize"
        )]
        error: String,
    },
}

/// Truncate a string to at most [`TEXT_MAX_BYTES`] bytes on a char boundary.
pub fn truncate_text(value: &str) -> String {
    if value.len() <= TEXT_MAX_BYTES {
        return value.to_owned();
    }
    let mut end = TEXT_MAX_BYTES;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn too_large(limit: usize, actual: usize) -> PluginError {
    PluginError::new(
        CONTENT_TOO_LARGE,
        format!("frame exceeds {limit} bytes (got {actual})"),
    )
}

fn malformed(error: impl std::fmt::Display) -> PluginError {
    PluginError::new(INVALID_PARAMS, format!("malformed frame: {error}"))
}

/// Write one frame as a single NDJSON line. The 1 MiB cap and the 8192 byte
/// text bound are enforced here (the latter via the `bounded_text` serde
/// attributes).
pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, frame: &T) -> Result<(), PluginError> {
    let mut line = serde_json::to_vec(frame)
        .map_err(|error| PluginError::new(IO_ERROR, format!("failed to encode frame: {error}")))?;
    if line.len() > MAX_FRAME_BYTES {
        return Err(too_large(MAX_FRAME_BYTES, line.len()));
    }
    line.push(b'\n');
    writer
        .write_all(&line)
        .and_then(|()| writer.flush())
        .map_err(|error| PluginError::new(IO_ERROR, format!("failed to write frame: {error}")))
}

/// Read one NDJSON frame. Returns `Ok(None)` on clean EOF. An over-long line
/// is rejected as soon as it crosses the cap, before it is buffered whole.
pub fn read_frame<R: BufRead, T: DeserializeOwned>(
    reader: &mut R,
) -> Result<Option<T>, PluginError> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().map_err(|error| {
            PluginError::new(IO_ERROR, format!("failed to read frame: {error}"))
        })?;
        if available.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            return Err(PluginError::new(
                IO_ERROR,
                "unexpected end of stream inside a frame",
            ));
        }
        match available.iter().position(|byte| *byte == b'\n') {
            Some(index) => {
                if line.len() + index > MAX_FRAME_BYTES {
                    return Err(too_large(MAX_FRAME_BYTES, line.len() + index));
                }
                line.extend_from_slice(&available[..index]);
                reader.consume(index + 1);
                break;
            }
            None => {
                if line.len() + available.len() > MAX_FRAME_BYTES {
                    return Err(too_large(MAX_FRAME_BYTES, line.len() + available.len()));
                }
                line.extend_from_slice(available);
                let consumed = available.len();
                reader.consume(consumed);
            }
        }
    }
    let frame = serde_json::from_slice(&line).map_err(malformed)?;
    Ok(Some(frame))
}

mod bounded_text {
    use super::truncate_text;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &str, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&truncate_text(value))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
        let value = String::deserialize(deserializer)?;
        Ok(truncate_text(&value))
    }

    pub fn serialize_option<S: Serializer>(
        value: &Option<String>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(value) => serializer.serialize_some(&truncate_text(value)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize_option<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<String>, D::Error> {
        let value = Option::<String>::deserialize(deserializer)?;
        Ok(value.map(|value| truncate_text(&value)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::BufReader;

    fn parent_frames() -> Vec<ParentFrame> {
        vec![
            ParentFrame::Init {
                plugin_id: "pub.demo".to_owned(),
                plugin_root: "/tmp/plugins/pub.demo".to_owned(),
                main_entry: "dist/main.mjs".to_owned(),
                granted_capabilities: vec!["storage".to_owned(), "workspace:read".to_owned()],
            },
            ParentFrame::InvokeCommand {
                call_id: 7,
                command_id: "echo".to_owned(),
                args: Some(json!({ "key": "greeting" })),
            },
            ParentFrame::InvokeCommand {
                call_id: 8,
                command_id: "no-args".to_owned(),
                args: None,
            },
            ParentFrame::DeliverEvent {
                event_id: 3,
                event: "worktree.created".to_owned(),
                payload: json!({ "worktreeId": "wt-1" }),
            },
            ParentFrame::HostResult {
                call_id: 4,
                ok: true,
                value: Some(json!({ "stored": "hi" })),
                error_code: None,
                error: None,
            },
            ParentFrame::HostResult {
                call_id: 5,
                ok: false,
                value: None,
                error_code: Some("capability_denied".to_owned()),
                error: Some("denied by gate".to_owned()),
            },
            ParentFrame::Shutdown,
        ]
    }

    fn child_frames() -> Vec<ChildFrame> {
        vec![
            ChildFrame::Ready {
                commands: vec!["echo".to_owned(), "no-args".to_owned()],
            },
            ChildFrame::CommandResult {
                call_id: 7,
                ok: true,
                value: Some(json!({ "echoed": "hi" })),
                error: None,
            },
            ChildFrame::CommandResult {
                call_id: 8,
                ok: false,
                value: None,
                error: Some("boom".to_owned()),
            },
            ChildFrame::EventAck { event_id: 3 },
            ChildFrame::HostCall {
                call_id: 0,
                method: "storage.get".to_owned(),
                params: Some(json!({ "key": "k" })),
            },
            ChildFrame::HostCall {
                call_id: 1,
                method: "secrets.get".to_owned(),
                params: None,
            },
            ChildFrame::Log {
                level: LogLevel::Warn,
                message: "careful".to_owned(),
            },
            ChildFrame::Fatal {
                error: "boom".to_owned(),
            },
        ]
    }

    #[test]
    fn parent_frame_shapes_match_oracle_fields() {
        assert_eq!(
            serde_json::to_value(&parent_frames()[0]).unwrap(),
            json!({
                "type": "init",
                "pluginId": "pub.demo",
                "pluginRoot": "/tmp/plugins/pub.demo",
                "mainEntry": "dist/main.mjs",
                "grantedCapabilities": ["storage", "workspace:read"]
            })
        );
        assert_eq!(
            serde_json::to_value(&parent_frames()[1]).unwrap(),
            json!({
                "type": "invokeCommand",
                "callId": 7,
                "commandId": "echo",
                "args": { "key": "greeting" }
            })
        );
        assert_eq!(
            serde_json::to_value(&parent_frames()[3]).unwrap(),
            json!({
                "type": "deliverEvent",
                "eventId": 3,
                "event": "worktree.created",
                "payload": { "worktreeId": "wt-1" }
            })
        );
        assert_eq!(
            serde_json::to_value(&parent_frames()[4]).unwrap(),
            json!({
                "type": "hostResult",
                "callId": 4,
                "ok": true,
                "value": { "stored": "hi" }
            })
        );
        assert_eq!(
            serde_json::to_value(ParentFrame::Shutdown).unwrap(),
            json!({ "type": "shutdown" })
        );
    }

    #[test]
    fn child_frame_shapes_match_oracle_fields() {
        assert_eq!(
            serde_json::to_value(&child_frames()[0]).unwrap(),
            json!({ "type": "ready", "commands": ["echo", "no-args"] })
        );
        assert_eq!(
            serde_json::to_value(&child_frames()[2]).unwrap(),
            json!({ "type": "commandResult", "callId": 8, "ok": false, "error": "boom" })
        );
        assert_eq!(
            serde_json::to_value(&child_frames()[3]).unwrap(),
            json!({ "type": "eventAck", "eventId": 3 })
        );
        assert_eq!(
            serde_json::to_value(&child_frames()[4]).unwrap(),
            json!({
                "type": "hostCall",
                "callId": 0,
                "method": "storage.get",
                "params": { "key": "k" }
            })
        );
        assert_eq!(
            serde_json::to_value(&child_frames()[6]).unwrap(),
            json!({ "type": "log", "level": "warn", "message": "careful" })
        );
        assert_eq!(
            serde_json::to_value(&child_frames()[7]).unwrap(),
            json!({ "type": "fatal", "error": "boom" })
        );
    }

    #[test]
    fn optional_fields_are_omitted_not_null() {
        let invoke = serde_json::to_value(&parent_frames()[2]).unwrap();
        assert!(invoke.get("args").is_none());
        let host_call = serde_json::to_value(&child_frames()[5]).unwrap();
        assert!(host_call.get("params").is_none());
        let result = serde_json::to_value(&child_frames()[1]).unwrap();
        assert!(result.get("error").is_none());
    }

    #[test]
    fn parent_frames_round_trip() {
        for frame in parent_frames() {
            let encoded = serde_json::to_string(&frame).unwrap();
            let decoded: ParentFrame = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, frame);
        }
    }

    #[test]
    fn child_frames_round_trip() {
        for frame in child_frames() {
            let encoded = serde_json::to_string(&frame).unwrap();
            let decoded: ChildFrame = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, frame);
        }
    }

    #[test]
    fn write_frame_appends_ndjson_newline() {
        let mut out = Vec::new();
        write_frame(&mut out, &ChildFrame::EventAck { event_id: 9 }).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "{\"type\":\"eventAck\",\"eventId\":9}\n"
        );
    }

    #[test]
    fn read_frame_reads_sequence_then_eof() {
        let input = "{\"type\":\"ready\",\"commands\":[]}\n\
                     {\"type\":\"eventAck\",\"eventId\":3}\n";
        let mut reader = BufReader::new(input.as_bytes());
        assert_eq!(
            read_frame::<_, ChildFrame>(&mut reader).unwrap(),
            Some(ChildFrame::Ready { commands: vec![] })
        );
        assert_eq!(
            read_frame::<_, ChildFrame>(&mut reader).unwrap(),
            Some(ChildFrame::EventAck { event_id: 3 })
        );
        assert_eq!(read_frame::<_, ChildFrame>(&mut reader).unwrap(), None);
    }

    #[test]
    fn read_frame_rejects_malformed_frames() {
        let cases = [
            "not json\n",
            "[]\n",
            "{\"type\":\"eventAck\"}\n",
            "{\"type\":\"bogus\"}\n",
            "{\"type\":\"log\",\"level\":\"debug\",\"message\":\"x\"}\n",
            "{\"type\":\"hostCall\",\"callId\":-1,\"method\":\"m\"}\n",
            "{\"type\":\"commandResult\",\"callId\":1.5,\"ok\":true}\n",
        ];
        for case in cases {
            let mut reader = BufReader::new(case.as_bytes());
            let error = read_frame::<_, ChildFrame>(&mut reader).unwrap_err();
            assert_eq!(error.code(), INVALID_PARAMS, "case: {case}");
        }
    }

    #[test]
    fn read_frame_rejects_oversized_line() {
        let line = format!(
            "{{\"type\":\"log\",\"level\":\"info\",\"message\":\"{}\"}}\n",
            "a".repeat(MAX_FRAME_BYTES)
        );
        let mut reader = BufReader::new(line.as_bytes());
        let error = read_frame::<_, ChildFrame>(&mut reader).unwrap_err();
        assert_eq!(error.code(), CONTENT_TOO_LARGE);
    }

    #[test]
    fn read_frame_accepts_frame_at_exactly_the_cap() {
        let prefix = "{\"type\":\"log\",\"level\":\"info\",\"message\":\"";
        let suffix = "\"}";
        let padding = MAX_FRAME_BYTES - prefix.len() - suffix.len();
        let line = format!("{prefix}{}{suffix}\n", "a".repeat(padding));
        let mut reader = BufReader::new(line.as_bytes());
        let frame = read_frame::<_, ChildFrame>(&mut reader).unwrap().unwrap();
        assert_eq!(
            frame,
            ChildFrame::Log {
                level: LogLevel::Info,
                message: truncate_text(&"a".repeat(padding)),
            }
        );
    }

    #[test]
    fn write_frame_rejects_oversized_frame() {
        let frame = ChildFrame::CommandResult {
            call_id: 1,
            ok: true,
            value: Some(Value::String("a".repeat(MAX_FRAME_BYTES))),
            error: None,
        };
        let error = write_frame(&mut Vec::new(), &frame).unwrap_err();
        assert_eq!(error.code(), CONTENT_TOO_LARGE);
    }

    #[test]
    fn write_frame_truncates_bounded_error_text() {
        let long = "x".repeat(TEXT_MAX_BYTES + 1000);
        let child_frames = [
            ChildFrame::CommandResult {
                call_id: 1,
                ok: false,
                value: None,
                error: Some(long.clone()),
            },
            ChildFrame::Log {
                level: LogLevel::Error,
                message: long.clone(),
            },
            ChildFrame::Fatal {
                error: long.clone(),
            },
        ];
        for frame in child_frames {
            let mut out = Vec::new();
            write_frame(&mut out, &frame).unwrap();
            assert!(out.len() < TEXT_MAX_BYTES + 512);
            let value: Value = serde_json::from_slice(&out[..out.len() - 1]).unwrap();
            let text = value
                .get("error")
                .or_else(|| value.get("message"))
                .and_then(Value::as_str)
                .unwrap();
            assert_eq!(text.len(), TEXT_MAX_BYTES);
        }

        let host_result = ParentFrame::HostResult {
            call_id: 2,
            ok: false,
            value: None,
            error_code: Some("action_failed".to_owned()),
            error: Some(long),
        };
        let mut out = Vec::new();
        write_frame(&mut out, &host_result).unwrap();
        let value: Value = serde_json::from_slice(&out[..out.len() - 1]).unwrap();
        assert_eq!(value["error"].as_str().unwrap().len(), TEXT_MAX_BYTES);
    }

    #[test]
    fn read_frame_truncates_bounded_error_text() {
        let line = format!(
            "{{\"type\":\"log\",\"level\":\"info\",\"message\":\"{}\"}}\n",
            "y".repeat(TEXT_MAX_BYTES + 1000)
        );
        let mut reader = BufReader::new(line.as_bytes());
        let frame = read_frame::<_, ChildFrame>(&mut reader).unwrap().unwrap();
        match frame {
            ChildFrame::Log { message, .. } => assert_eq!(message.len(), TEXT_MAX_BYTES),
            other => panic!("unexpected frame: {other:?}"),
        }

        let line = format!(
            "{{\"type\":\"fatal\",\"error\":\"{}\"}}\n",
            "z".repeat(TEXT_MAX_BYTES + 1)
        );
        let mut reader = BufReader::new(line.as_bytes());
        let frame = read_frame::<_, ChildFrame>(&mut reader).unwrap().unwrap();
        match frame {
            ChildFrame::Fatal { error } => assert_eq!(error.len(), TEXT_MAX_BYTES),
            other => panic!("unexpected frame: {other:?}"),
        }
    }

    #[test]
    fn truncation_is_utf8_boundary_safe() {
        let long = "€".repeat(4000);
        let truncated = truncate_text(&long);
        assert_eq!(truncated.len(), 8190);
        assert!(truncated.chars().all(|character| character == '€'));

        let frame = ChildFrame::Fatal { error: long };
        let mut out = Vec::new();
        write_frame(&mut out, &frame).unwrap();
        let value: Value = serde_json::from_slice(&out[..out.len() - 1]).unwrap();
        assert_eq!(value["error"].as_str().unwrap().len(), 8190);
    }

    #[test]
    fn read_frame_reports_eof_inside_a_frame() {
        let mut reader = BufReader::new(&b"{\"type\":\"ready\""[..]);
        let error = read_frame::<_, ChildFrame>(&mut reader).unwrap_err();
        assert_eq!(error.code(), IO_ERROR);
    }

    #[test]
    fn init_requires_granted_capabilities() {
        let error = serde_json::from_value::<ParentFrame>(json!({
            "type": "init",
            "pluginId": "pub.demo",
            "pluginRoot": "/tmp/plugins/pub.demo",
            "mainEntry": "main.mjs"
        }))
        .unwrap_err();
        assert!(error.to_string().contains("grantedCapabilities"));
    }
}
