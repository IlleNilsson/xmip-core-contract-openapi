#![forbid(unsafe_code)]

//! The `OpenAPI` content contract — a technology of `xmip-core-contract`.
//!
//! Two claims, decided 2026-09-07 (ADR-0042): **well-formedness is a given**
//! and **conformance is a given once a contract is named**.
//!
//! Well-formed here is a *sound description*: a JSON object that says which
//! `OpenAPI` it is (`openapi` 3.x, or `swagger` 2.0), names its `info.title`
//! and `info.version`, keeps its `paths` as an object of templates that
//! begin with `/`, gives every operation under them its `responses` where
//! the version requires it, and refers with `$ref` only to what the
//! document itself holds. A description that points at a component it does
//! not have is the one every generated client breaks on.
//!
//! Conformance is the *operation*: a Location that names this contract with
//! `POST /orders` bound has every description held to defining that method
//! on that path, and `placeOrder` to an operation of that id. A description
//! in YAML is the same document in another notation and is the next layer
//! here; the schemas inside are `xmip-core-contract-json-schema`'s.

use contract::{
    Contract, ContractDescriptor, ContractError, ContractFactory, ContractId, ValidationIssue,
    ValidationResult,
};
use serde_json::Value;
use stream::Stream;

const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// The bound operation: a method and path, or an operation id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    Route { method: String, path: String },
    Id(String),
}

impl Operation {
    /// `POST /orders` or `placeOrder`.
    ///
    /// # Errors
    /// An empty reference, or a method with no path.
    pub fn parse(reference: &str) -> Result<Self, ContractError> {
        let reference = reference.trim();
        if reference.is_empty() {
            return Err(ContractError {
                message: "an empty operation reference".to_string(),
            });
        }
        match reference.split_once(' ') {
            Some((method, path)) if path.trim().starts_with('/') => Ok(Self::Route {
                method: method.to_ascii_lowercase(),
                path: path.trim().to_string(),
            }),
            Some(_) => Err(ContractError {
                message: format!("{reference:?} is not METHOD /path or an operationId"),
            }),
            None => Ok(Self::Id(reference.to_string())),
        }
    }

    fn reference(&self) -> String {
        match self {
            Self::Route { method, path } => format!("{} {path}", method.to_ascii_uppercase()),
            Self::Id(id) => id.clone(),
        }
    }
}

/// The `OpenAPI` contract, bare or bound to an operation.
pub struct OpenApi {
    descriptor: ContractDescriptor,
    operation: Option<Operation>,
}

impl OpenApi {
    /// A sound description, of any operations.
    #[must_use]
    pub fn new() -> Self {
        Self {
            descriptor: descriptor("openapi"),
            operation: None,
        }
    }

    /// A sound description that defines `operation`.
    #[must_use]
    pub fn of(operation: Operation) -> Self {
        Self {
            descriptor: descriptor(&format!("openapi:{}", operation.reference())),
            operation: Some(operation),
        }
    }

    /// Whether an operation is bound.
    #[must_use]
    pub fn is_bound(&self) -> bool {
        self.operation.is_some()
    }
}

impl Default for OpenApi {
    fn default() -> Self {
        Self::new()
    }
}

fn descriptor(id: &str) -> ContractDescriptor {
    ContractDescriptor {
        id: ContractId(id.to_string()),
        version: "1".to_string(),
        representation: "application/vnd.oai.openapi+json".to_string(),
    }
}

/// Every departure of `document` from a sound description.
fn soundness(document: &Value) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let Some(object) = document.as_object() else {
        return vec![issue(
            "malformed",
            "the document is not a JSON object",
            None,
        )];
    };
    let version = object
        .get("openapi")
        .or_else(|| object.get("swagger"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let responses_required = !version.starts_with("3.1");
    if !(version.starts_with("3.") || version == "2.0") {
        issues.push(issue(
            "structure",
            "neither openapi 3.x nor swagger 2.0 is declared",
            Some("openapi".into()),
        ));
    }
    for field in ["title", "version"] {
        if object
            .get("info")
            .and_then(|info| info.get(field))
            .and_then(Value::as_str)
            .is_none()
        {
            issues.push(issue(
                "structure",
                &format!("info.{field} is missing"),
                Some("info".into()),
            ));
        }
    }
    match object.get("paths") {
        Some(Value::Object(paths)) => {
            for (template, item) in paths {
                if !template.starts_with('/') {
                    issues.push(issue(
                        "structure",
                        "a path template that does not begin with /",
                        Some(format!("paths.{template}")),
                    ));
                }
                let Some(item) = item.as_object() else {
                    continue;
                };
                for method in METHODS {
                    if let Some(operation) = item.get(method)
                        && responses_required
                        && operation
                            .get("responses")
                            .and_then(Value::as_object)
                            .is_none()
                    {
                        issues.push(issue(
                            "structure",
                            "an operation without responses",
                            Some(format!("paths.{template}.{method}")),
                        ));
                    }
                }
            }
        }
        Some(_) => issues.push(issue(
            "structure",
            "paths is not an object",
            Some("paths".into()),
        )),
        None if responses_required => {
            issues.push(issue("structure", "paths is missing", Some("paths".into())));
        }
        None => {}
    }
    references(document, document, "", &mut issues);
    issues
}

/// Every `$ref` under `value` that begins with `#` and does not land.
fn references(root: &Value, value: &Value, path: &str, issues: &mut Vec<ValidationIssue>) {
    match value {
        Value::Object(object) => {
            if let Some(Value::String(target)) = object.get("$ref")
                && let Some(pointer) = target.strip_prefix('#')
                && root.pointer(pointer).is_none()
            {
                issues.push(issue(
                    "reference",
                    &format!("$ref {target} does not land"),
                    Some(path.trim_start_matches('.').to_string()),
                ));
            }
            for (key, child) in object {
                references(root, child, &format!("{path}.{key}"), issues);
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                references(root, child, &format!("{path}[{i}]"), issues);
            }
        }
        _ => {}
    }
}

/// Whether `document` defines `operation`.
fn defines(document: &Value, operation: &Operation) -> bool {
    let Some(paths) = document.get("paths").and_then(Value::as_object) else {
        return false;
    };
    match operation {
        Operation::Route { method, path } => {
            paths.get(path).and_then(|item| item.get(method)).is_some()
        }
        Operation::Id(id) => paths.values().any(|item| {
            METHODS.iter().any(|method| {
                item.get(method)
                    .and_then(|operation| operation.get("operationId"))
                    .and_then(Value::as_str)
                    == Some(id)
            })
        }),
    }
}

impl Contract for OpenApi {
    fn descriptor(&self) -> &ContractDescriptor {
        &self.descriptor
    }

    fn identify(&self, stream: &Stream) -> Result<bool, ContractError> {
        if stream.media_type().is_some_and(|m| {
            let base = m.split(';').next().unwrap_or("").trim();
            base.eq_ignore_ascii_case("application/vnd.oai.openapi+json")
                || base.eq_ignore_ascii_case("application/vnd.oai.openapi")
        }) {
            return Ok(true);
        }
        let Ok(text) = std::str::from_utf8(stream.bytes()) else {
            return Ok(false);
        };
        Ok(text.contains("\"openapi\"") || text.contains("\"swagger\""))
    }

    fn validate(&self, stream: &Stream) -> Result<ValidationResult, ContractError> {
        let document: Value = match serde_json::from_slice(stream.bytes()) {
            Ok(document) => document,
            Err(error) => {
                return Ok(result(vec![issue(
                    "malformed",
                    &format!("not JSON: {error}"),
                    None,
                )]));
            }
        };
        let mut issues = soundness(&document);
        if let Some(operation) = &self.operation
            && !defines(&document, operation)
        {
            issues.push(issue(
                "operation",
                &format!("does not define {}", operation.reference()),
                Some("paths".into()),
            ));
        }
        Ok(result(issues))
    }
}

fn issue(code: &str, message: &str, path: Option<String>) -> ValidationIssue {
    ValidationIssue {
        code: code.to_string(),
        message: message.to_string(),
        path,
    }
}

fn result(issues: Vec<ValidationIssue>) -> ValidationResult {
    ValidationResult {
        valid: issues.is_empty(),
        issues,
    }
}

/// Loads the contract a Location names: an empty reference is the bare
/// contract, anything else an operation, `POST /orders` or `placeOrder`.
pub struct OpenApiFactory;

impl ContractFactory for OpenApiFactory {
    fn technology(&self) -> &'static str {
        "openapi"
    }

    fn load(&self, reference: &str) -> Result<Box<dyn Contract>, ContractError> {
        if reference.trim().is_empty() {
            return Ok(Box::new(OpenApi::new()));
        }
        Ok(Box::new(OpenApi::of(Operation::parse(reference)?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xcore::StreamId;

    const ORDERS: &str = r##"{
        "openapi": "3.0.3",
        "info": {"title": "Orders", "version": "1"},
        "paths": {
            "/orders": {
                "post": {
                    "operationId": "placeOrder",
                    "requestBody": {"content": {"application/json": {"schema":
                        {"$ref": "#/components/schemas/Order"}}}},
                    "responses": {"201": {"description": "placed"}}
                },
                "get": {"responses": {"200": {"$ref": "#/components/responses/List"}}}
            }
        },
        "components": {
            "schemas": {"Order": {"type": "object"}},
            "responses": {"List": {"description": "the orders"}}
        }
    }"##;

    fn stream(text: &str, media_type: Option<&str>) -> Stream {
        Stream::new(
            StreamId::new(1),
            text.as_bytes().to_vec(),
            media_type.map(str::to_string),
        )
    }

    #[test]
    fn a_sound_description_holds_bare_and_bound() {
        let bare = OpenApi::new();
        assert!(bare.identify(&stream(ORDERS, None)).expect("identify"));
        assert!(
            bare.identify(&stream("{}", Some("application/vnd.oai.openapi+json")))
                .expect("identify")
        );
        assert!(!bare.identify(&stream("{\"a\":1}", None)).expect("identify"));
        let result = bare.validate(&stream(ORDERS, None)).expect("validate");
        assert!(result.valid, "{:?}", result.issues);
        for reference in ["POST /orders", "get /orders", "placeOrder"] {
            let bound = OpenApiFactory.load(reference).expect("load");
            assert!(
                bound
                    .validate(&stream(ORDERS, None))
                    .expect("validate")
                    .valid,
                "{reference}"
            );
        }
        assert_eq!(
            OpenApiFactory
                .load("post /orders")
                .expect("load")
                .descriptor()
                .id
                .0,
            "openapi:POST /orders"
        );
        assert!(OpenApi::of(Operation::parse("x").expect("parse")).is_bound());
        let swagger = r#"{"swagger":"2.0","info":{"title":"t","version":"1"},"paths":{}}"#;
        assert!(
            bare.validate(&stream(swagger, None))
                .expect("validate")
                .valid
        );
        let webhooks = r#"{"openapi":"3.1.0","info":{"title":"t","version":"1"},"webhooks":{}}"#;
        assert!(
            bare.validate(&stream(webhooks, None))
                .expect("validate")
                .valid
        );
    }

    #[test]
    fn departures_are_named_with_their_paths() {
        let broken = ORDERS
            .replace("#/components/schemas/Order", "#/components/schemas/Nope")
            .replace("\"/orders\"", "\"orders\"")
            .replace("\"version\": \"1\"", "\"v\": \"1\"");
        let result = OpenApi::new()
            .validate(&stream(&broken, None))
            .expect("validate");
        let messages: Vec<&str> = result.issues.iter().map(|i| i.message.as_str()).collect();
        assert_eq!(result.issues.len(), 3, "{messages:?}");
        assert_eq!(messages[0], "info.version is missing");
        assert_eq!(messages[1], "a path template that does not begin with /");
        assert!(messages[2].contains("$ref #/components/schemas/Nope does not land"));
        assert_eq!(
            result.issues[2].path.as_deref(),
            Some("paths.orders.post.requestBody.content.application/json.schema")
        );
        let no_responses = ORDERS.replace(
            r#""responses": {"201": {"description": "placed"}}"#,
            "\"x\": 1",
        );
        let result = OpenApi::new()
            .validate(&stream(&no_responses, None))
            .expect("validate");
        assert_eq!(result.issues[0].message, "an operation without responses");
        let bound = OpenApi::of(Operation::parse("DELETE /orders").expect("parse"));
        let result = bound.validate(&stream(ORDERS, None)).expect("validate");
        assert_eq!(result.issues[0].code, "operation");
        assert_eq!(result.issues[0].message, "does not define DELETE /orders");
        let bound = OpenApi::of(Operation::parse("cancelOrder").expect("parse"));
        assert!(
            !bound
                .validate(&stream(ORDERS, None))
                .expect("validate")
                .valid
        );
    }

    #[test]
    fn what_is_not_a_description_does_not_hold() {
        let result = OpenApi::new()
            .validate(&stream("[1]", None))
            .expect("validate");
        assert_eq!(result.issues[0].code, "malformed");
        let result = OpenApi::new()
            .validate(&stream("{nope", None))
            .expect("validate");
        assert_eq!(result.issues[0].code, "malformed");
        let result = OpenApi::new()
            .validate(&stream(
                r#"{"info":{"title":"t","version":"1"},"paths":[]}"#,
                None,
            ))
            .expect("validate");
        assert_eq!(result.issues.len(), 2);
        assert!(Operation::parse("").is_err());
        assert!(Operation::parse("POST orders").is_err());
    }
}
