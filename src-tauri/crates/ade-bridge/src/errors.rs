use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use thiserror::Error;

/// Bridge-level error union. Every domain error the command surface can produce
/// is wrapped here and serialized over IPC as `{ "message": string }`, which the
/// real TS adapters surface as a rejected `Error(message)` (spec §6).
#[derive(Debug, Error)]
pub enum BridgeError {
    #[error(transparent)]
    Core(#[from] ade_core::errors::CoreError),
    #[error(transparent)]
    Fs(#[from] ade_fs::FsError),
    #[error(transparent)]
    Store(#[from] ade_store::StoreError),
    #[error(transparent)]
    Pty(#[from] ade_pty::PtyError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Message(String),
}

impl BridgeError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl Serialize for BridgeError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("BridgeError", 1)?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

/// Hand-written so the specta export can describe command errors as
/// `{ message: string }` without implementing [`specta::Type`] on the wrapped
/// domain errors. Registered through a static sentinel so repeated
/// registrations resolve to one named type.
impl specta::Type for BridgeError {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        use specta::datatype::{Field, Struct};

        specta::datatype::NamedDataType::init_with_sentinel(
            "ade_bridge::errors::BridgeError",
            &[],
            false,
            false,
            types,
            |types, ndt| {
                ndt.name = "BridgeError".into();
                ndt.module_path = "ade_bridge::errors".into();
                ndt.ty = Some(
                    Struct::named()
                        .field(
                            "message",
                            Field::new(<String as specta::Type>::definition(types)),
                        )
                        .build(),
                );
            },
            |_types| unreachable!("BridgeError is registered as a named type"),
        )
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_as_message_object() {
        let error = BridgeError::Message("boom".to_string());
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            json!({ "message": "boom" })
        );
    }

    #[test]
    fn wraps_domain_errors_with_their_display_text() {
        let error = BridgeError::Fs(ade_fs::FsError::PathAccessDenied);
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            json!({ "message": ade_fs::PATH_ACCESS_DENIED_MESSAGE })
        );

        let io = BridgeError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"));
        assert_eq!(
            serde_json::to_value(&io).unwrap(),
            json!({ "message": "missing" })
        );
    }
}
