use std::collections::HashMap;

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

use crate::protocol_proxy::{UpstreamProxyResponse, UpstreamWireApi};
use crate::settings::{GatewayHeaderBackup, RelayProfile};

use super::route;
use super::store::GatewaySettings;

pub(super) const RESPONSE_BUDGET_SECS: u64 = 120;
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

pub fn target_available(gateway: &GatewaySettings, target: &GatewayHeaderBackup) -> bool {
    gateway.providers.iter().any(|provider| {
        provider.enabled
            && provider.relay.id == target.provider_id
            && route::chat_models(&provider.relay).contains(&target.model)
    })
}

pub(crate) fn portable_request(request: &Value) -> Option<Value> {
    if [
        "previous_response_id",
        "conversation",
        "background",
        "prompt",
    ]
    .iter()
    .any(|key| {
        request
            .get(*key)
            .is_some_and(|value| !value.is_null() && value != false)
    }) {
        return None;
    }
    match request.get("input")? {
        Value::String(text) if !text.trim().is_empty() => {}
        Value::Array(items) if !items.is_empty() => {
            let mut pending = HashMap::new();
            let mut saw_user = false;
            for item in items {
                match item["type"].as_str().unwrap_or("message") {
                    "message" => {
                        if !matches!(
                            item["role"].as_str()?,
                            "system" | "developer" | "user" | "assistant"
                        ) {
                            return None;
                        }
                        if item["role"] == "user" {
                            saw_user = true;
                        }
                        match item.get("content")? {
                            Value::String(_) => {}
                            Value::Array(parts)
                                if parts.iter().all(|part| {
                                    matches!(
                                        part["type"].as_str(),
                                        Some("input_text" | "output_text" | "refusal")
                                    ) && part.get("file_id").is_none()
                                }) => {}
                            _ => return None,
                        }
                    }
                    kind @ ("function_call" | "custom_tool_call") => {
                        let id = item["call_id"].as_str().filter(|id| !id.is_empty())?;
                        let payload = if kind == "function_call" {
                            "arguments"
                        } else {
                            "input"
                        };
                        item["name"].as_str()?;
                        item[payload].as_str()?;
                        if pending.insert(id, (kind, false)).is_some() {
                            return None;
                        }
                    }
                    kind @ ("function_call_output" | "custom_tool_call_output") => {
                        let (call, done) = pending.get_mut(item["call_id"].as_str()?)?;
                        let expected = if *call == "function_call" {
                            "function_call_output"
                        } else {
                            "custom_tool_call_output"
                        };
                        if *done || kind != expected || !item["output"].is_string() {
                            return None;
                        }
                        *done = true;
                    }
                    _ => return None,
                }
            }
            if !saw_user || pending.values().any(|(_, done)| !done) {
                return None;
            }
        }
        _ => return None,
    }
    let mut body = request.clone();
    let object = body.as_object_mut()?;
    for key in [
        "previous_response_id",
        "conversation",
        "background",
        "prompt",
        "prompt_cache_key",
        "include",
        "service_tier",
    ] {
        object.remove(key);
    }
    // 消息和调用内容已经完整内联。移掉 Responses 上游签发的项 ID，保留配对所需的 call_id。
    if let Some(items) = object.get_mut("input").and_then(Value::as_array_mut) {
        for item in items {
            item.as_object_mut()?.remove("id");
        }
    }
    body["tools"] = json!([]);
    body["tool_choice"] = json!("none");
    body["parallel_tool_calls"] = json!(false);
    body["store"] = json!(false);
    Some(body)
}

pub(crate) fn guard_wire_request(
    wire: UpstreamWireApi,
    body: &mut Value,
    relay: &RelayProfile,
    model: Option<&str>,
) -> Result<()> {
    let target = super::http_wait::HTTP_ATTEMPT.with(|scope| scope.backup_target());
    ensure!(
        target.as_ref().is_some_and(|(provider, target_model)| {
            &relay.id == provider
                && model == Some(target_model.as_str())
                && body["model"] == target_model.as_str()
        }),
        "备用请求的供应商或模型被改写"
    );
    if wire == UpstreamWireApi::Responses {
        *body = portable_request(body).context("备用请求包含不可移植的上下文")?;
    }
    let fields = body.as_object_mut().context("备用请求体无效")?;
    fields.remove("functions");
    fields.remove("function_call");
    fields.remove("mcp_servers");
    match wire {
        UpstreamWireApi::Responses | UpstreamWireApi::ChatCompletions => {
            fields.insert("tools".into(), json!([]));
            fields.insert("tool_choice".into(), json!("none"));
            fields.insert("parallel_tool_calls".into(), json!(false));
        }
        UpstreamWireApi::AnthropicMessages => {
            // Messages 省略 tools 和 tool_choice 即不会授权工具。
            fields.remove("tools");
            fields.remove("tool_choice");
            fields.remove("parallel_tool_calls");
        }
        _ => bail!("备用请求协议不支持"),
    }
    fields.insert("stream".into(), json!(true));
    Ok(())
}

pub(super) async fn validate_response(
    response: &mut UpstreamProxyResponse,
    request: &Value,
) -> Result<()> {
    ensure!(
        response.is_stream && !response.exec_adapted && !response.compaction,
        "备用供应商未返回支持的流式响应"
    );
    let mut source = std::mem::take(&mut response.stream_prefix);
    if let Some(body) = response.buffered_body.take() {
        ensure!(
            source.len().saturating_add(body.len()) <= MAX_RESPONSE_BYTES,
            "备用响应过大"
        );
        source.extend(body);
    } else {
        let mut upstream = response.take_response()?;
        while let Some(chunk) = upstream.chunk().await? {
            ensure!(
                source.len().saturating_add(chunk.len()) <= MAX_RESPONSE_BYTES,
                "备用响应过大"
            );
            source.extend_from_slice(&chunk);
        }
    }
    let body = match response.wire_api {
        UpstreamWireApi::Responses => source,
        UpstreamWireApi::ChatCompletions => {
            let mut converter =
                crate::protocol_proxy::ChatSseToResponsesConverter::with_request(request);
            let mut body = converter.push_bytes(&source);
            ensure!(converter.has_terminal_event(), "备用 Chat 响应缺少终态");
            body.extend(converter.finish());
            body
        }
        UpstreamWireApi::AnthropicMessages => {
            let mut converter =
                crate::anthropic_bridge::AnthropicSseToResponsesConverter::with_request(request);
            let mut body = converter.push_bytes(&source);
            ensure!(converter.has_terminal_event(), "备用 Messages 响应缺少终态");
            body.extend(converter.finish());
            body
        }
        _ => bail!("备用响应协议不支持"),
    };
    validate_events(&body)?;
    response.wire_api = UpstreamWireApi::Responses;
    response.buffered_body = Some(body);
    Ok(())
}

fn validate_events(bytes: &[u8]) -> Result<()> {
    let text = std::str::from_utf8(bytes)?.replace("\r\n", "\n");
    let mut completed = false;
    for frame in text.split("\n\n") {
        let data = frame
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(|line| line.strip_prefix(' ').unwrap_or(line))
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let event: Value = serde_json::from_str(&data)?;
        ensure!(!completed, "备用响应在终态后继续输出");
        let kind = event["type"].as_str().context("备用响应事件缺少类型")?;
        match kind {
            "response.created" | "response.in_progress" | "response.completed" => {
                ensure!(
                    event["response"]["output"]
                        .as_array()
                        .is_some_and(|items| items.iter().all(assistant_output)),
                    "备用响应包含工具或不支持的输出项"
                );
                if kind == "response.completed" {
                    ensure!(
                        event["response"]["status"] == "completed"
                            && event["response"]["output"].as_array().is_some_and(|items| {
                                items.iter().any(|item| {
                                    item["type"] == "message"
                                        && item["content"]
                                            .as_array()
                                            .is_some_and(|parts| !parts.is_empty())
                                })
                            }),
                        "备用响应缺少完整回答"
                    );
                    completed = true;
                }
            }
            "response.output_item.added" | "response.output_item.done" => {
                ensure!(assistant_output(&event["item"]), "备用响应尝试调用工具");
            }
            "response.content_part.added" | "response.content_part.done" => {
                ensure!(
                    matches!(
                        event["part"]["type"].as_str(),
                        Some("output_text" | "refusal")
                    ),
                    "备用响应包含不支持的内容"
                );
            }
            "response.output_text.delta"
            | "response.output_text.done"
            | "response.refusal.delta"
            | "response.refusal.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_summary_text.done"
            | "response.reasoning_text.delta"
            | "response.reasoning_text.done" => {}
            _ => bail!("备用响应包含失败事件或不支持的事件类型"),
        }
    }
    ensure!(completed, "备用响应缺少完成事件");
    Ok(())
}

fn assistant_output(item: &Value) -> bool {
    match item["type"].as_str() {
        Some("message") => {
            item["role"] == "assistant"
                && item["content"].as_array().is_some_and(|parts| {
                    parts.iter().all(|part| {
                        matches!(part["type"].as_str(), Some("output_text" | "refusal"))
                    })
                })
        }
        Some("reasoning") => true,
        _ => false,
    }
}
