//! DeepSeek DSML tool-call markup parsing and display sanitizing.
//!
//! DeepSeek's OpenAI-compatible stream may return `<｜｜DSML｜｜ calls>` blocks
//! instead of OpenAI `tool_calls`. Rig's OpenAI dialect does not understand
//! those blocks, so they must be converted back into real tool calls before the
//! turn's tool-execution loop runs.

use std::collections::HashMap;

use rig_core::{
    completion::CompletionResponse,
    message::{AssistantContent, ToolCall, ToolFunction, ToolName},
};
use serde_json::{Map, Value};

const CALLS_OPEN: &str = "<｜｜DSML｜｜ calls>";
const CALLS_CLOSE: &str = "</｜｜DSML｜｜ calls>";
const INVOKE_OPEN: &str = "<｜｜DSML｜｜ invoke";
const INVOKE_CLOSE: &str = "</｜｜DSML｜｜ invoke>";
const PARAM_OPEN: &str = "<｜｜DSML｜｜ parameter";
const PARAM_CLOSE: &str = "</｜｜DSML｜｜ parameter>";

/// Remove DSML blocks from text shown to users, including an incomplete block
/// at the end of a delta. This does not affect the raw response retained for
/// model history and DSML parsing.
pub(super) fn sanitize_dsml_text(text: &str) -> String {
    let mut sanitized = String::new();
    let mut rest = text;
    while let Some(start) = rest.find(CALLS_OPEN) {
        sanitized.push_str(&rest[..start]);
        let Some(end) = rest[start + CALLS_OPEN.len()..].find(CALLS_CLOSE) else {
            // Incomplete DSML is protocol markup, not prose. Drop the tail.
            return sanitized;
        };
        rest = &rest[start + CALLS_OPEN.len() + end + CALLS_CLOSE.len()..];
    }
    sanitized.push_str(rest);
    sanitized
}

/// Convert DSML invoke blocks into real rig tool calls. Invalid blocks are
/// skipped rather than aborting the whole turn.
pub(super) fn extract_dsml_tool_calls(response: &CompletionResponse) -> Vec<ToolCall> {
    response
        .choice
        .iter()
        .filter_map(|content| match content {
            AssistantContent::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .flat_map(parse_dsml_calls)
        .collect()
}

fn parse_dsml_calls(text: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    let mut rest = text;
    let mut index = 0usize;
    while let Some(start) = rest.find(INVOKE_OPEN) {
        let Some(gt) = rest[start + INVOKE_OPEN.len()..].find('>') else {
            break;
        };
        let header = &rest[start + INVOKE_OPEN.len()..start + INVOKE_OPEN.len() + gt];
        let Some(end) = rest[start + INVOKE_OPEN.len() + gt + 1..].find(INVOKE_CLOSE) else {
            break;
        };
        let body =
            &rest[start + INVOKE_OPEN.len() + gt + 1..start + INVOKE_OPEN.len() + gt + 1 + end];
        if let Some(name) = attribute(header, "name")
            && let Ok(tool_name) = ToolName::new(name)
        {
            let function = ToolFunction::new(tool_name, parse_parameters(body));
            calls.push(ToolCall::from_wire(format!("dsml_{index}"), function));
            index += 1;
        }
        rest = &rest[start + INVOKE_OPEN.len() + gt + 1 + end + INVOKE_CLOSE.len()..];
    }
    calls
}

fn parse_parameters(body: &str) -> Value {
    let mut parameters = Map::new();
    let mut rest = body;
    while let Some(start) = rest.find(PARAM_OPEN) {
        let Some(gt) = rest[start + PARAM_OPEN.len()..].find('>') else {
            break;
        };
        let header = &rest[start + PARAM_OPEN.len()..start + PARAM_OPEN.len() + gt];
        let Some(end) = rest[start + PARAM_OPEN.len() + gt + 1..].find(PARAM_CLOSE) else {
            break;
        };
        let value =
            &rest[start + PARAM_OPEN.len() + gt + 1..start + PARAM_OPEN.len() + gt + 1 + end];
        if let Some(name) = attribute(header, "name") {
            let kind = parameter_kind(header);
            parameters.insert(name.to_string(), parse_typed_value(kind, value));
        }
        rest = &rest[start + PARAM_OPEN.len() + gt + 1 + end + PARAM_CLOSE.len()..];
    }
    Value::Object(parameters)
}

fn attribute<'a>(header: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("{key}=\"");
    let start = header.find(&needle)? + needle.len();
    let end = header[start..].find('"')?;
    Some(&header[start..start + end])
}

fn parameter_kind(header: &str) -> &'static str {
    if header.contains("number=\"true\"") {
        "number"
    } else if header.contains("boolean=\"true\"") {
        "boolean"
    } else if header.contains("object=\"true\"") {
        "object"
    } else if header.contains("array=\"true\"") {
        "array"
    } else {
        "string"
    }
}

fn parse_typed_value(kind: &str, value: &str) -> Value {
    match kind {
        "number" => value
            .parse::<f64>()
            .map_or_else(|_| Value::String(value.to_string()), Value::from),
        "boolean" => Value::Bool(matches!(
            value.trim(),
            "true" | "True" | "TRUE" | "1" | "yes"
        )),
        "object" | "array" => {
            serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_string()))
        }
        _ => Value::String(value.to_string()),
    }
}

/// Accumulate raw per-part text for DSML parsing while emitting only the
/// newly sanitized suffix to the UI.
#[derive(Default)]
pub(super) struct DsmlState {
    raw: HashMap<usize, String>,
    emitted: HashMap<usize, usize>,
}

impl DsmlState {
    pub(super) fn push_text(&mut self, part: usize, fragment: &str) -> String {
        let raw = self.raw.entry(part).or_default();
        raw.push_str(fragment);
        let sanitized = sanitize_dsml_text(raw);
        let emitted = self.emitted.entry(part).or_insert(0);
        let suffix = sanitized.chars().skip(*emitted).collect::<String>();
        *emitted = sanitized.chars().count();
        suffix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_dsml_protocol_from_display_text() {
        let input = "before <｜｜DSML｜｜ calls><｜｜DSML｜｜ invoke name=\"load_skill\"> <｜｜DSML｜｜ parameter name=\"skill_name\" string=\"true\">higher-education-writing</｜｜DSML｜｜ parameter> </｜｜DSML｜｜ invoke> </｜｜DSML｜｜ calls> after";
        assert_eq!(sanitize_dsml_text(input), "before  after");
    }

    #[test]
    fn extracts_real_tool_calls_with_typed_parameters() {
        let input = "<｜｜DSML｜｜ calls><｜｜DSML｜｜ invoke name=\"load_skill\"> <｜｜DSML｜｜ parameter name=\"skill_name\" string=\"true\">higher-education-writing</｜｜DSML｜｜ parameter> <｜｜DSML｜｜ parameter name=\"priority\" number=\"true\">2</｜｜DSML｜｜ parameter> <｜｜DSML｜｜ parameter name=\"enabled\" boolean=\"true\">true</｜｜DSML｜｜ parameter> </｜｜DSML｜｜ invoke> </｜｜DSML｜｜ calls>";
        let calls = parse_dsml_calls(input);
        assert_eq!(calls.len(), 1);
        let function = calls[0].function.clone();
        assert_eq!(function.name.as_str(), "load_skill");
        let arguments = function.arguments_value();
        assert_eq!(arguments["skill_name"], "higher-education-writing");
        assert_eq!(arguments["priority"].as_f64(), Some(2.0));
        assert_eq!(arguments["enabled"].as_bool(), Some(true));
    }

    #[test]
    fn split_deltas_emit_only_sanitized_suffixes() {
        let mut state = DsmlState::default();
        let first = state.push_text(
            0,
            "hello <｜｜DSML｜｜ calls><｜｜DSML｜｜ invoke name=\"load_skill\">",
        );
        assert_eq!(first, "hello ");
        let second = state.push_text(0, " </｜｜DSML｜｜ invoke> </｜｜DSML｜｜ calls> world");
        assert_eq!(second, " world");
    }

    #[test]
    fn incomplete_protocol_is_not_emitted() {
        let mut state = DsmlState::default();
        let text = state.push_text(
            0,
            "<｜｜DSML｜｜ calls><｜｜DSML｜｜ invoke name=\"load_skill\">",
        );
        assert_eq!(text, "");
    }

    #[test]
    fn completion_response_tool_calls_are_extracted() {
        let response = CompletionResponse::new(
            vec![AssistantContent::Text(rig_core::message::Text::from(
                "<｜｜DSML｜｜ calls><｜｜DSML｜｜ invoke name=\"create_report\"> <｜｜DSML｜｜ parameter name=\"title\" string=\"true\">Report</｜｜DSML｜｜ parameter> </｜｜DSML｜｜ invoke> </｜｜DSML｜｜ calls>",
            ))],
            rig_core::completion::Usage::new(),
            rig_core::message::Origin::new("openai.responses", "openai", "stub"),
            serde_json::json!({}),
        );
        let calls = extract_dsml_tool_calls(&response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name.as_str(), "create_report");
        assert_eq!(calls[0].function.arguments_value()["title"], "Report");
    }
}
