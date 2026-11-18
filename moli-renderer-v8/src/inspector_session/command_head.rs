//! pilot: the facts the renderer reads from an Inspector command before V8
//! dispatches it.

use std::fmt;

use serde::de::{Deserialize, Deserializer, IgnoredAny, MapAccess, Visitor};
use serde_json::Value;

use crate::script_vm::WEBDRIVER_BIDI_FILE_PROMPT_HANDLER_PARAM;

/// pilot: the `id`, `method` and the few `params` that steer how the renderer
/// dispatches an Inspector command, read in one pass that skips every other
/// value without copying it. The renderer used to parse the whole command
/// into a `serde_json::Value` once for each of these facts, about seven times
/// between the Page owner and V8 for one Runtime.evaluate, which for
/// Playwright's 346 KB injected script cost milliseconds per navigation.
///
/// The facts are those `serde_json::Value::get` reads from the parsed
/// command: a later duplicate key wins, `params` that are not an object have
/// no fields, and a command that is not JSON has none at all. A command whose
/// head is not an object holding an object (`params` that are null or an
/// array, say) is read through a `Value`, so its facts are exactly those.
#[derive(Debug, Default)]
pub(crate) struct InspectorCommandHead {
    /// Whether the command is JSON at all.
    json: bool,
    id: Option<Value>,
    method: Option<Value>,
    /// `None` without `params`, `Some(None)` when `params` is not an object.
    params: Option<Option<InspectorCommandParamsHead>>,
}

#[derive(Debug, Default)]
struct InspectorCommandParamsHead {
    context_id: Option<Value>,
    execution_context_id: Option<Value>,
    object_id: Option<Value>,
    unique_context_id: Option<Value>,
    user_gesture: Option<Value>,
    webdriver_bidi_file_prompt_handler: Option<Value>,
}

#[cfg(test)]
thread_local! {
    static INSPECTOR_COMMAND_HEAD_READS: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
}

/// How many command heads this thread has read, for tests that pin a command
/// to one read on its way to V8.
#[cfg(test)]
pub(crate) fn inspector_command_head_reads_for_test() -> usize {
    INSPECTOR_COMMAND_HEAD_READS.with(std::cell::Cell::get)
}

impl InspectorCommandHead {
    pub(crate) fn read(raw_json: &str) -> Self {
        #[cfg(test)]
        INSPECTOR_COMMAND_HEAD_READS.with(|reads| reads.set(reads.get() + 1));
        serde_json::from_str::<Self>(raw_json).unwrap_or_else(|_| {
            serde_json::from_str::<Value>(raw_json)
                .map(|message| Self::from_message(&message))
                .unwrap_or_default()
        })
    }

    fn from_message(message: &Value) -> Self {
        let params = message.get("params").map(|params| {
            params.as_object().map(|params| InspectorCommandParamsHead {
                context_id: params.get("contextId").cloned(),
                execution_context_id: params.get("executionContextId").cloned(),
                object_id: params.get("objectId").cloned(),
                unique_context_id: params.get("uniqueContextId").cloned(),
                user_gesture: params.get("userGesture").cloned(),
                webdriver_bidi_file_prompt_handler: params
                    .get(WEBDRIVER_BIDI_FILE_PROMPT_HANDLER_PARAM)
                    .cloned(),
            })
        });
        Self {
            json: true,
            id: message.get("id").cloned(),
            method: message.get("method").cloned(),
            params,
        }
    }

    pub(crate) fn is_json(&self) -> bool {
        self.json
    }

    pub(crate) fn id(&self) -> Option<&Value> {
        self.id.as_ref()
    }

    pub(crate) fn method(&self) -> Option<&str> {
        self.method.as_ref().and_then(Value::as_str)
    }

    /// Whether the command has `params` at all, an object or not.
    pub(crate) fn has_params(&self) -> bool {
        self.params.is_some()
    }

    pub(crate) fn has_object_params(&self) -> bool {
        matches!(self.params, Some(Some(_)))
    }

    pub(crate) fn context_id(&self) -> Option<&Value> {
        self.params()?.context_id.as_ref()
    }

    pub(crate) fn execution_context_id(&self) -> Option<&Value> {
        self.params()?.execution_context_id.as_ref()
    }

    pub(crate) fn object_id(&self) -> Option<&Value> {
        self.params()?.object_id.as_ref()
    }

    pub(crate) fn unique_context_id(&self) -> Option<&Value> {
        self.params()?.unique_context_id.as_ref()
    }

    pub(crate) fn user_gesture(&self) -> Option<&Value> {
        self.params()?.user_gesture.as_ref()
    }

    pub(crate) fn webdriver_bidi_file_prompt_handler(&self) -> Option<&Value> {
        self.params()?.webdriver_bidi_file_prompt_handler.as_ref()
    }

    fn params(&self) -> Option<&InspectorCommandParamsHead> {
        self.params.as_ref()?.as_ref()
    }
}

impl<'de> Deserialize<'de> for InspectorCommandHead {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(InspectorCommandHeadVisitor)
    }
}

struct InspectorCommandHeadVisitor;

impl<'de> Visitor<'de> for InspectorCommandHeadVisitor {
    type Value = InspectorCommandHead;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an Inspector command object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut head = InspectorCommandHead {
            json: true,
            ..InspectorCommandHead::default()
        };
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "id" => head.id = Some(map.next_value()?),
                "method" => head.method = Some(map.next_value()?),
                "params" => {
                    head.params = Some(Some(map.next_value::<InspectorCommandParamsHead>()?));
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(head)
    }
}

impl<'de> Deserialize<'de> for InspectorCommandParamsHead {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(InspectorCommandParamsHeadVisitor)
    }
}

struct InspectorCommandParamsHeadVisitor;

impl<'de> Visitor<'de> for InspectorCommandParamsHeadVisitor {
    type Value = InspectorCommandParamsHead;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Inspector command params")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut params = InspectorCommandParamsHead::default();
        while let Some(key) = map.next_key::<String>()? {
            let field = match key.as_str() {
                "contextId" => &mut params.context_id,
                "executionContextId" => &mut params.execution_context_id,
                "objectId" => &mut params.object_id,
                "uniqueContextId" => &mut params.unique_context_id,
                "userGesture" => &mut params.user_gesture,
                WEBDRIVER_BIDI_FILE_PROMPT_HANDLER_PARAM => {
                    &mut params.webdriver_bidi_file_prompt_handler
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                    continue;
                }
            };
            *field = Some(map.next_value()?);
        }
        Ok(params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// What the renderer reads from a command, through a head or a `Value`.
    #[derive(Debug, PartialEq)]
    struct Facts {
        json: bool,
        id: Option<Value>,
        method: Option<String>,
        has_params: bool,
        has_object_params: bool,
        params: Vec<Option<Value>>,
    }

    const PARAMS: [&str; 6] = [
        "contextId",
        "executionContextId",
        "objectId",
        "uniqueContextId",
        "userGesture",
        WEBDRIVER_BIDI_FILE_PROMPT_HANDLER_PARAM,
    ];

    fn head_facts(raw_json: &str) -> Facts {
        let head = InspectorCommandHead::read(raw_json);
        Facts {
            json: head.is_json(),
            id: head.id().cloned(),
            method: head.method().map(str::to_owned),
            has_params: head.has_params(),
            has_object_params: head.has_object_params(),
            params: vec![
                head.context_id().cloned(),
                head.execution_context_id().cloned(),
                head.object_id().cloned(),
                head.unique_context_id().cloned(),
                head.user_gesture().cloned(),
                head.webdriver_bidi_file_prompt_handler().cloned(),
            ],
        }
    }

    fn value_facts(raw_json: &str) -> Facts {
        let message = serde_json::from_str::<Value>(raw_json).ok();
        let params = message.as_ref().and_then(|message| message.get("params"));
        Facts {
            json: message.is_some(),
            id: message
                .as_ref()
                .and_then(|message| message.get("id"))
                .cloned(),
            method: message
                .as_ref()
                .and_then(|message| message.get("method"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            has_params: params.is_some(),
            has_object_params: params.is_some_and(Value::is_object),
            params: PARAMS
                .iter()
                .map(|name| params.and_then(|params| params.get(name)).cloned())
                .collect(),
        }
    }

    /// A Runtime.evaluate the size of Playwright's injected script, full of
    /// the quotes, backslashes and newlines that JSON escapes.
    fn injected_script_sized_evaluate() -> String {
        let expression = "if (a) {\n  b(\"c\\\\d\", 'e');\n}\n".repeat(13_000);
        assert!(expression.len() > 340_000, "{}", expression.len());
        let mut params = serde_json::Map::new();
        params.insert("expression".to_owned(), json!(expression));
        params.insert("contextId".to_owned(), json!(3));
        params.insert("userGesture".to_owned(), json!(true));
        params.insert(
            WEBDRIVER_BIDI_FILE_PROMPT_HANDLER_PARAM.to_owned(),
            json!("accept"),
        );
        params.insert("returnByValue".to_owned(), json!(true));
        json!({"id": 7, "method": "Runtime.evaluate", "params": params, "sessionId": "SID-1"})
            .to_string()
    }

    // 2026-11-18: the renderer parsed a whole Runtime.evaluate into a Value
    // for each fact it read; the head must read the same facts, odd commands
    // included, so the dispatch decisions do not change.
    #[test]
    fn inspector_command_head_reads_the_facts_a_parsed_value_gives() {
        let commands = [
            injected_script_sized_evaluate(),
            json!({"id": 8, "method": "Runtime.callFunctionOn", "params": {"functionDeclaration": "() => 1", "objectId": "1.2.3", "executionContextId": null}}).to_string(),
            json!({"id": 9, "method": "Runtime.enable"}).to_string(),
            json!({"id": 10, "method": "Debugger.resume", "params": {}}).to_string(),
            json!({"id": 11, "method": "Runtime.evaluate", "params": null}).to_string(),
            json!({"id": 12, "method": "Runtime.evaluate", "params": [{"userGesture": true}]}).to_string(),
            json!({"id": 13, "method": "Runtime.evaluate", "params": "contextId"}).to_string(),
            json!({"id": -14, "method": 5, "params": {"uniqueContextId": "T:1", "userGesture": "yes"}}).to_string(),
            r#"{"id":15,"method":"Runtime.evaluate","method":"Runtime.enable","params":{"contextId":1,"contextId":2}}"#.to_owned(),
            r#"[{"id":16,"method":"Runtime.enable"}]"#.to_owned(),
            r#"{"id":17,"method":"Runtime.evaluate","params":{"expression":"unterminated}"#.to_owned(),
            String::new(),
        ];
        for command in &commands {
            let shown = &command[..command.len().min(120)];
            assert_eq!(head_facts(command), value_facts(command), "{shown}");
        }
    }

    // 2026-11-18: the head of a 346 KB Runtime.evaluate carries the steering
    // params, the WebDriver BiDi file prompt handler under its real name too.
    #[test]
    fn inspector_command_head_of_a_large_evaluate_holds_its_steering_params() {
        let head = InspectorCommandHead::read(&injected_script_sized_evaluate());
        assert_eq!(head.method(), Some("Runtime.evaluate"));
        assert_eq!(head.id(), Some(&json!(7)));
        assert_eq!(head.context_id(), Some(&json!(3)));
        assert_eq!(head.user_gesture(), Some(&json!(true)));
        assert_eq!(
            head.webdriver_bidi_file_prompt_handler(),
            Some(&json!("accept"))
        );
        assert!(head.has_object_params());
    }
}
