use jsonc_parser::cst::{CstInputValue, CstObject};
use serde_json::Value;

pub(super) fn input(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(value) => CstInputValue::Bool(*value),
        Value::Number(value) => CstInputValue::Number(value.to_string()),
        Value::String(value) => CstInputValue::String(value.clone()),
        Value::Array(values) => CstInputValue::Array(values.iter().map(input).collect()),
        Value::Object(values) => CstInputValue::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), input(value)))
                .collect(),
        ),
    }
}

pub(super) fn set(object: &CstObject, key: &str, value: &Value) {
    if let Some(property) = object.get(key) {
        property.set_value(input(value));
    } else {
        object.append(key, input(value));
    }
}
