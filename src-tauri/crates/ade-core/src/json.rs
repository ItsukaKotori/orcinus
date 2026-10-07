use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A JSON value with a specta definition that exports as a recursive *named*
/// type.
///
/// `specta`'s built-in `serde_json::Value` impl registers as an inline type,
/// which `specta-typescript` refuses (or overflows on) when it appears in a
/// command signature. The bridge command surface is free-form JSON, so it uses
/// this transparent wrapper for its args/results; payload types in this crate
/// project JSON fields through the same named type with
/// `#[specta(type = Option<Json>)]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Json(pub Value);

impl Json {
    pub fn new(value: Value) -> Self {
        Self(value)
    }

    pub fn into_inner(self) -> Value {
        self.0
    }
}

impl From<Value> for Json {
    fn from(value: Value) -> Self {
        Self(value)
    }
}

impl From<Json> for Value {
    fn from(json: Json) -> Self {
        json.0
    }
}

#[cfg(feature = "specta")]
mod specta_impl {
    use super::Json;

    const JSON_SENTINEL: &str = "ade_core::json::Json";

    fn json_datatype(types: &mut specta::Types) -> specta::datatype::DataType {
        use specta::datatype::{DataType, Enum, Field, List, Map, Variant};

        let bool_ty = <bool as specta::Type>::definition(types);
        let number_ty = <f64 as specta::Type>::definition(types);
        let string_ty = <String as specta::Type>::definition(types);
        // Recursive use site: the sentinel registration is already in place, so
        // this resolves to a named reference instead of expanding again.
        let self_ty = <Json as specta::Type>::definition(types);

        let mut json = Enum::default();
        json.variants = vec![
            ("Null".into(), Variant::unit()),
            (
                "Bool".into(),
                Variant::unnamed().field(Field::new(bool_ty)).build(),
            ),
            (
                "Number".into(),
                Variant::unnamed().field(Field::new(number_ty)).build(),
            ),
            (
                "String".into(),
                Variant::unnamed()
                    .field(Field::new(string_ty.clone()))
                    .build(),
            ),
            (
                "Array".into(),
                Variant::unnamed()
                    .field(Field::new(DataType::List(List::new(self_ty.clone()))))
                    .build(),
            ),
            (
                "Object".into(),
                Variant::unnamed()
                    .field(Field::new(DataType::Map(Map::new(string_ty, self_ty))))
                    .build(),
            ),
        ];
        DataType::Enum(json)
    }

    impl specta::Type for Json {
        fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
            specta::datatype::NamedDataType::init_with_sentinel(
                JSON_SENTINEL,
                &[],
                false,
                false,
                types,
                |types, ndt| {
                    ndt.name = "Json".into();
                    ndt.module_path = "ade_core::json".into();
                    ndt.ty = Some(json_datatype(types));
                },
                json_datatype,
            )
            .into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_transparently() {
        let value = json!({ "theme": "dark", "nested": [1, 2, { "a": null }] });
        let wrapped = Json::new(value.clone());
        assert_eq!(serde_json::to_value(&wrapped).unwrap(), value);
        assert_eq!(
            serde_json::from_value::<Json>(value.clone()).unwrap().0,
            value
        );
    }

    #[cfg(feature = "specta")]
    #[test]
    fn specta_definition_registers_named_recursive_type() {
        let mut types = specta::Types::default();
        let datatype = <Json as specta::Type>::definition(&mut types);
        assert!(matches!(datatype, specta::datatype::DataType::Reference(_)));
        assert!(types
            .into_unsorted_iter()
            .any(|ndt| ndt.name == "Json" && ndt.ty.is_some()));
    }
}
