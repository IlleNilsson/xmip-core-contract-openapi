//! What an `OpenAPI` description declares, read once: its title and every
//! operation under `paths`. The contract holds a bound operation to it, and
//! the HTTP API logic technology binds a method and path to an `operationId`
//! through it — what a technology needs of a description it takes from the
//! contract that reads it, rather than reading the document a second way
//! (ADR-0044).

use contract::ContractError;
use serde_json::Value;

/// The eight HTTP methods a path item may carry.
pub const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// One operation a description declares: the method and path template it is
/// reached by, and its `operationId` when it has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declared {
    pub method: String,
    pub template: String,
    pub operation_id: Option<String>,
}

/// A description's title and operations.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Description {
    title: String,
    operations: Vec<Declared>,
}

impl Description {
    /// Read `text`, a description in JSON.
    ///
    /// # Errors
    /// The text is not JSON.
    pub fn parse(text: &str) -> Result<Self, ContractError> {
        let document: Value = serde_json::from_str(text)
            .map_err(|error| ContractError::new(format!("not valid JSON: {error}")))?;
        Ok(Self::of(&document))
    }

    /// What `document` declares. A document with no `paths` — a 3.1
    /// description of webhooks alone — declares no operation.
    #[must_use]
    pub fn of(document: &Value) -> Self {
        let title = document
            .pointer("/info/title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut operations = Vec::new();
        if let Some(paths) = document.get("paths").and_then(Value::as_object) {
            for (template, item) in paths {
                let Some(item) = item.as_object() else {
                    continue;
                };
                for method in METHODS {
                    if let Some(operation) = item.get(method) {
                        operations.push(Declared {
                            method: method.to_string(),
                            template: template.clone(),
                            operation_id: operation
                                .get("operationId")
                                .and_then(Value::as_str)
                                .map(str::to_string),
                        });
                    }
                }
            }
        }
        Self { title, operations }
    }

    /// `info.title`, or empty when the description has none.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Every operation, in the order of the paths and then of [`METHODS`].
    #[must_use]
    pub fn operations(&self) -> &[Declared] {
        &self.operations
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDERS: &str = r#"{"openapi":"3.1.0","info":{"title":"Orders"},"paths":{
      "/orders":{"post":{"operationId":"placeOrder"},"get":{"summary":"list"}},
      "/orders/{id}":{"parameters":[],"delete":{"operationId":"cancelOrder"}}}}"#;

    #[test]
    fn a_description_lists_its_title_and_every_operation_by_method_and_path() {
        let description = Description::parse(ORDERS).expect("json");
        assert_eq!(description.title(), "Orders");
        let ids: Vec<(&str, &str, Option<&str>)> = description
            .operations()
            .iter()
            .map(|o| {
                (
                    o.method.as_str(),
                    o.template.as_str(),
                    o.operation_id.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            ids,
            [
                ("get", "/orders", None),
                ("post", "/orders", Some("placeOrder")),
                ("delete", "/orders/{id}", Some("cancelOrder")),
            ]
        );
    }

    #[test]
    fn a_description_without_paths_or_title_declares_nothing() {
        let webhooks = Description::parse(r#"{"openapi":"3.1.0","webhooks":{}}"#).expect("json");
        assert_eq!(webhooks.title(), "");
        assert!(webhooks.operations().is_empty());
        assert!(Description::parse("{nope").is_err());
    }
}
