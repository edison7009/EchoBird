use std::collections::BTreeMap;

use axum::http::StatusCode;
use serde_json::{json, Value};

use super::ProxyError;

pub(crate) const RESPONSES_REQUEST: &str = "_echobird_responses";

pub(crate) fn alias(namespace: &str, name: &str) -> String {
    if namespace.is_empty() || namespace == "functions" {
        name.into()
    } else {
        format!("{namespace}__{name}")
    }
}

/// Retain each original declaration so grammar and namespace survive a native hop.
pub(crate) fn catalog(request: &Value) -> Result<BTreeMap<String, Value>, ProxyError> {
    let mut catalog = BTreeMap::new();
    for tool in request["tools"].as_array().into_iter().flatten().chain(
        request["input"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| {
                matches!(
                    item["type"].as_str(),
                    Some("additional_tools" | "tool_search_output")
                )
            })
            .flat_map(|item| item["tools"].as_array().into_iter().flatten()),
    ) {
        collect(tool, "", &mut catalog)?;
    }
    Ok(catalog)
}

/// Tools the Responses host runs itself; a Chat model has nothing to run them with.
pub(crate) fn is_hosted(kind: &str) -> bool {
    kind.starts_with("web_search")
        || matches!(
            kind,
            "file_search" | "code_interpreter" | "image_generation" | "mcp" | "tool_search"
        )
}

fn collect(
    tool: &Value,
    namespace: &str,
    catalog: &mut BTreeMap<String, Value>,
) -> Result<(), ProxyError> {
    if tool["type"] == "namespace" {
        if !namespace.is_empty() {
            return Err(ProxyError::new(
                StatusCode::BAD_REQUEST,
                "Nested tool namespaces are unsupported",
            ));
        }
        let namespace = tool["name"].as_str().unwrap_or_default();
        for child in tool["tools"].as_array().into_iter().flatten() {
            collect(child, namespace, catalog)?;
        }
        return Ok(());
    }
    if is_hosted(tool["type"].as_str().unwrap_or_default()) {
        return Ok(());
    }
    if tool["type"] != "function" && tool["type"] != "custom" {
        return Err(ProxyError::new(StatusCode::BAD_REQUEST, "The router accepts client function and custom tools only; hosted tools require their existing client setting to be disabled"));
    }
    let name = tool["name"]
        .as_str()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| ProxyError::new(StatusCode::BAD_REQUEST, "Client tool name is required"))?;
    let mut original = tool.clone();
    if !namespace.is_empty() {
        original["namespace"] = json!(namespace);
    }
    let name = alias(namespace, name);
    if let Some(previous) = catalog.insert(name, original.clone()) {
        if previous != original {
            return Err(ProxyError::new(
                StatusCode::BAD_REQUEST,
                "Client tool names collide after namespace conversion",
            ));
        }
    }
    Ok(())
}

pub(crate) fn chat_tools(catalog: &BTreeMap<String, Value>) -> Vec<Value> {
    catalog.iter().map(|(name, tool)| {
        let mut function = tool.clone();
        function.as_object_mut().unwrap().remove("type");
        function.as_object_mut().unwrap().remove("namespace");
        function["name"] = json!(name);
        if tool["type"] == "custom" {
            function.as_object_mut().unwrap().remove("format");
            let mut description = tool["description"].as_str().unwrap_or_default().to_string();
            if tool["format"]["type"] == "grammar" {
                description.push_str(&format!("\nInput must follow this {} grammar:\n{}", tool["format"]["syntax"].as_str().unwrap_or("provided"), tool["format"]["definition"].as_str().unwrap_or_default()));
            }
            function["description"] = json!(description);
            function["parameters"] = json!({"type":"object","properties":{"input":{"type":"string"}},"required":["input"],"additionalProperties":false});
        }
        json!({"type":"function","function":function})
    }).collect()
}

pub(crate) fn custom_input(arguments: &str) -> String {
    serde_json::from_str::<Value>(arguments)
        .ok()
        .and_then(|value| value["input"].as_str().map(str::to_owned))
        .unwrap_or_else(|| arguments.into())
}

/// Some account gateways require a type beside object-only oneOf schemas.
pub(super) fn type_object_unions(schema: &mut Value) {
    fn object_branch(root: &Value, schema: &Value, depth: usize) -> bool {
        if depth > 64 {
            return false;
        }
        if schema["type"] == "object" {
            return true;
        }
        schema["$ref"]
            .as_str()
            .and_then(|reference| reference.strip_prefix('#'))
            .and_then(|pointer| root.pointer(pointer))
            .is_some_and(|target| object_branch(root, target, depth + 1))
    }
    fn visit(schema: &mut Value, root: &Value, depth: usize) {
        if depth > 64 {
            return;
        }
        let Some(object) = schema.as_object_mut() else {
            return;
        };
        if !object.contains_key("type")
            && object
                .get("oneOf")
                .and_then(Value::as_array)
                .is_some_and(|branches| {
                    !branches.is_empty()
                        && branches.iter().all(|branch| object_branch(root, branch, 0))
                })
        {
            object.insert("type".into(), json!("object"));
        }
        for key in [
            "items",
            "additionalItems",
            "unevaluatedItems",
            "contains",
            "not",
            "if",
            "then",
            "else",
            "propertyNames",
            "additionalProperties",
            "unevaluatedProperties",
        ] {
            if let Some(child) = object.get_mut(key) {
                if let Some(children) = child.as_array_mut() {
                    for child in children {
                        visit(child, root, depth + 1);
                    }
                } else {
                    visit(child, root, depth + 1);
                }
            }
        }
        for key in ["anyOf", "oneOf", "allOf", "prefixItems"] {
            if let Some(children) = object.get_mut(key).and_then(Value::as_array_mut) {
                for child in children {
                    visit(child, root, depth + 1);
                }
            }
        }
        for key in [
            "properties",
            "patternProperties",
            "$defs",
            "definitions",
            "dependentSchemas",
        ] {
            if let Some(children) = object.get_mut(key).and_then(Value::as_object_mut) {
                for child in children.values_mut() {
                    visit(child, root, depth + 1);
                }
            }
        }
    }
    visit(schema, &schema.clone(), 0);
}

#[cfg(test)]
mod schema_tests {
    use super::*;
    #[test]
    fn local_ref_object_unions_are_typed_without_changing_examples_or_mixed_unions() {
        let union = json!({"oneOf":[{"$ref":"#/$defs/A"},{"$ref":"#/$defs/B"}]});
        let mut schema = json!({"type":"object","$defs":{"A":{"type":"object"},"B":{"$ref":"#/$defs/A"}},"properties":{"choice":union},"examples":[union],"default":union,"additionalProperties":false});
        type_object_unions(&mut schema);
        assert_eq!(schema["properties"]["choice"]["type"], "object");
        assert_eq!(schema["examples"], json!([union]));
        assert_eq!(schema["default"], union);
        assert_eq!(schema["additionalProperties"], false);
        let original = schema.clone();
        type_object_unions(&mut schema);
        assert_eq!(schema, original);
        for branches in [
            json!([]),
            json!([{"type":"object"},{"type":"string"}]),
            json!([{"$ref":"#/$defs/Missing"}]),
            json!([{"$ref":"https://example.invalid/schema"}]),
            json!([{"$ref":"#"}]),
        ] {
            let mut schema = json!({"oneOf":branches});
            let original = schema.clone();
            type_object_unions(&mut schema);
            assert_eq!(schema, original);
        }
        let mut any = json!({"anyOf":[{"type":"object"},{"type":"object"}]});
        let original = any.clone();
        type_object_unions(&mut any);
        assert_eq!(any, original);
    }
}
