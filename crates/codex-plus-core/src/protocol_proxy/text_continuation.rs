use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::{Value, json};

const MAX_TEXT_BYTES: usize = 64_000;
const MAX_RECOVERY_BYTES: usize = 2 * 1024 * 1024;

pub(super) fn request_body(body: &Value) -> Option<Value> {
    for key in ["previous_response_id", "conversation", "prompt"] {
        if body.get(key).is_some_and(|value| !value.is_null()) {
            return None;
        }
    }
    if body.get("background").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    if body
        .pointer("/text/format/type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind != "text")
    {
        return None;
    }
    let input = match body.get("input")? {
        Value::String(text) if !text.is_empty() => vec![json!({"role": "user", "content": text})],
        Value::Array(items) if !items.is_empty() => {
            if items.iter().any(|item| item["type"] == "item_reference") {
                return None;
            }
            items.clone()
        }
        _ => return None,
    };
    let mut body = body.clone();
    body["input"] = Value::Array(input);
    body["tools"] = json!([]);
    body["tool_choice"] = json!("none");
    body["parallel_tool_calls"] = json!(false);
    body["store"] = json!(false);
    body.as_object_mut()?.remove("text");
    Some(body)
}

#[derive(Clone)]
pub(crate) struct Pending(Arc<Mutex<Option<Continuation>>>);

impl Pending {
    pub(super) fn new(builder: reqwest::RequestBuilder, body: Value, timeout: Duration) -> Self {
        Self(Arc::new(Mutex::new(Some(Continuation {
            builder,
            body,
            timeout,
        }))))
    }

    pub(crate) fn take(response: &mut reqwest::Response) -> Option<Continuation> {
        response
            .extensions_mut()
            .remove::<Self>()?
            .0
            .lock()
            .ok()?
            .take()
    }
}

pub(crate) struct Continuation {
    builder: reqwest::RequestBuilder,
    body: Value,
    timeout: Duration,
}

#[derive(Default)]
pub(crate) struct TextPrefix {
    response: Option<Value>,
    reasoning: Option<String>,
    items: Vec<Value>,
    message: Option<Value>,
    part: bool,
    text: String,
    sequence: u64,
}

impl TextPrefix {
    pub(super) fn recoverable(&self) -> bool {
        self.response.is_some() && self.message.is_some() && self.part && !self.text.is_empty()
    }

    fn matching_position(&self, event: &Value) -> bool {
        self.message.as_ref().is_some_and(|message| {
            message["id"] == event["item_id"]
                && event["output_index"].as_u64() == Some(self.items.len() as u64)
                && event["content_index"].as_u64() == Some(0)
        })
    }

    pub(super) fn observe(&mut self, kind: &str, event: &Value) -> bool {
        if let Some(sequence) = event["sequence_number"].as_u64() {
            self.sequence = self.sequence.max(sequence);
        }
        match kind {
            "response.created"
                if self.response.is_none()
                    && event["response"]["id"]
                        .as_str()
                        .is_some_and(|id| !id.is_empty())
                    && event["response"]["output"]
                        .as_array()
                        .is_some_and(Vec::is_empty) =>
            {
                self.response = Some(event["response"].clone());
                true
            }
            "response.in_progress" => self.response.as_ref().is_some_and(|response| {
                response["id"] == event["response"]["id"]
                    && event["response"]["output"]
                        .as_array()
                        .is_some_and(Vec::is_empty)
            }),
            "response.output_item.added"
                if self.response.is_some()
                    && self.message.is_none()
                    && self.reasoning.is_none()
                    && event["output_index"].as_u64() == Some(self.items.len() as u64) =>
            {
                let item = &event["item"];
                let Some(id) = item["id"].as_str().filter(|id| !id.is_empty()) else {
                    return false;
                };
                match item["type"].as_str() {
                    Some("reasoning") => self.reasoning = Some(id.to_owned()),
                    Some("message")
                        if item["role"] == "assistant"
                            && item["phase"] != "commentary"
                            && item["content"].as_array().is_some_and(Vec::is_empty) =>
                    {
                        self.message = Some(item.clone());
                    }
                    _ => return false,
                }
                true
            }
            "response.output_item.done" if self.reasoning.is_some() => {
                let item = &event["item"];
                if item["type"] != "reasoning"
                    || item["id"].as_str() != self.reasoning.as_deref()
                    || event["output_index"].as_u64() != Some(self.items.len() as u64)
                {
                    return false;
                }
                self.reasoning = None;
                self.items.push(item.clone());
                self.items.len() <= 32
            }
            "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_summary_text.done"
            | "response.reasoning_text.delta"
            | "response.reasoning_text.done" => {
                self.reasoning.is_some() && event["item_id"].as_str() == self.reasoning.as_deref()
            }
            "response.content_part.added"
                if !self.part
                    && self.matching_position(event)
                    && event["part"]["type"] == "output_text"
                    && event["part"]["text"].as_str() == Some("")
                    && empty_annotations(&event["part"]) =>
            {
                self.part = true;
                true
            }
            "response.output_text.delta" if self.part && self.matching_position(event) => {
                let Some(delta) = event["delta"].as_str() else {
                    return false;
                };
                if self.text.len() + delta.len() > MAX_TEXT_BYTES {
                    return false;
                }
                self.text.push_str(delta);
                true
            }
            _ => false,
        }
    }
}

fn empty_annotations(part: &Value) -> bool {
    part.get("annotations")
        .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty))
}

impl Continuation {
    pub(crate) async fn complete(
        mut self,
        prefix: TextPrefix,
        key_slot: &crate::gateway::key_pool::KeySlot,
        attempted: &mut bool,
    ) -> anyhow::Result<Vec<u8>> {
        let input = self.body["input"]
            .as_array_mut()
            .expect("prepared input array");
        input.push(json!({"role": "assistant", "content": [
            {"type": "output_text", "text": prefix.text, "annotations": []}
        ]}));
        input.push(json!({"role": "user", "content":
            [{"type": "input_text", "text": "Continue the interrupted assistant response from its last visible words. Do not repeat any of the preceding response. Respond only with new plain text. Do not call tools."}]
        }));

        // This is one new generation. Never route or retry this POST.
        let (client, request) = self.builder.json(&self.body).build_split();
        let request = request?;
        anyhow::ensure!(
            key_slot.try_record_additional_send(),
            "continuation rate limit reached"
        );
        *attempted = true;
        let mut response = tokio::time::timeout(self.timeout, client.execute(request)).await??;
        anyhow::ensure!(
            response.status() == reqwest::StatusCode::OK
                && response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .is_some_and(|value| value.starts_with("text/event-stream")),
            "unexpected continuation response"
        );
        tokio::time::timeout(Duration::from_secs(240), read_suffix(&mut response, prefix)).await?
    }
}

async fn read_suffix(
    response: &mut reqwest::Response,
    prefix: TextPrefix,
) -> anyhow::Result<Vec<u8>> {
    let mut stream = TextPrefix::default();
    let mut closed_part = false;
    let mut closed_item = false;
    let mut buffer = Vec::new();
    let mut received = 0;
    while let Some(chunk) = response.chunk().await? {
        received += chunk.len();
        anyhow::ensure!(received <= MAX_RECOVERY_BYTES, "continuation too large");
        buffer.extend_from_slice(&chunk);
        let mut start = 0;
        while let Some(end) = super::next_sse_boundary(&buffer, start) {
            let block = std::str::from_utf8(&buffer[start..end])?;
            start = end;
            if block
                .lines()
                .all(|line| line.is_empty() || line.starts_with(':'))
            {
                continue;
            }
            let event = super::sse_block_json(block)
                .ok_or_else(|| anyhow::anyhow!("invalid continuation event"))?;
            let kind = event["type"].as_str().unwrap_or_default();
            match kind {
                "response.completed" => {
                    let completed = &event["response"];
                    let output = completed["output"].as_array();
                    anyhow::ensure!(
                        stream.recoverable()
                            && closed_item
                            && completed["status"] == "completed"
                            && stream
                                .response
                                .as_ref()
                                .is_some_and(|created| created["id"] == completed["id"])
                            && output.is_some_and(|items| {
                                items.len() == stream.items.len() + 1
                                    && items[..stream.items.len()]
                                        .iter()
                                        .all(|item| item["type"] == "reasoning")
                                    && valid_message(&items[stream.items.len()], &stream)
                            }),
                        "invalid continuation completion"
                    );
                    return splice(prefix, &stream.text);
                }
                "response.output_text.done"
                    if !closed_part
                        && !closed_item
                        && stream.matching_position(&event)
                        && event["text"].as_str() == Some(stream.text.as_str()) => {}
                "response.content_part.done"
                    if !closed_part
                        && !closed_item
                        && stream.matching_position(&event)
                        && valid_part(&event["part"], &stream.text) =>
                {
                    closed_part = true;
                }
                "response.output_item.done"
                    if closed_part
                        && !closed_item
                        && event["output_index"].as_u64() == Some(stream.items.len() as u64)
                        && valid_message(&event["item"], &stream) =>
                {
                    closed_item = true;
                }
                _ if !closed_part && !closed_item && stream.observe(kind, &event) => {}
                _ => anyhow::bail!("unexpected continuation event"),
            }
        }
        buffer.drain(..start);
    }
    anyhow::bail!("continuation ended before completion")
}

fn valid_part(part: &Value, text: &str) -> bool {
    part["type"] == "output_text" && part["text"].as_str() == Some(text) && empty_annotations(part)
}

fn valid_message(item: &Value, stream: &TextPrefix) -> bool {
    stream
        .message
        .as_ref()
        .is_some_and(|message| message["id"] == item["id"])
        && item["type"] == "message"
        && item["role"] == "assistant"
        && item["phase"] != "commentary"
        && item["content"]
            .as_array()
            .is_some_and(|parts| parts.len() == 1 && valid_part(&parts[0], &stream.text))
}

fn without_overlap<'a>(prefix: &str, suffix: &'a str) -> &'a str {
    if let Some(rest) = suffix.strip_prefix(prefix) {
        return rest;
    }
    let mut starts = prefix
        .char_indices()
        .rev()
        .take(512)
        .filter(|(start, _)| prefix.len() - start >= 12)
        .collect::<Vec<_>>();
    starts.reverse();
    starts
        .into_iter()
        .find_map(|(start, _)| suffix.strip_prefix(&prefix[start..]))
        .unwrap_or(suffix)
}

fn splice(mut prefix: TextPrefix, suffix: &str) -> anyhow::Result<Vec<u8>> {
    let suffix = without_overlap(&prefix.text, suffix);
    anyhow::ensure!(
        !suffix.trim().is_empty(),
        "continuation supplied no new text"
    );
    let mut message = prefix.message.take().expect("validated message");
    let mut response = prefix.response.take().expect("validated response");
    let mut wire = Vec::new();
    let position = json!({
        "item_id": message["id"],
        "output_index": prefix.items.len(),
        "content_index": 0,
    });
    let mut sequence = prefix.sequence;
    let mut send = |kind: &str, mut event: Value| {
        sequence = sequence.saturating_add(1);
        event["type"] = json!(kind);
        event["sequence_number"] = json!(sequence);
        wire.extend_from_slice(format!("event: {kind}\ndata: {event}\n\n").as_bytes());
    };
    let mut delta = position.clone();
    delta["delta"] = json!(suffix);
    delta["logprobs"] = json!([]);
    send("response.output_text.delta", delta);
    prefix.text.push_str(suffix);
    let part = json!({"type": "output_text", "text": prefix.text, "annotations": []});
    let mut done = position.clone();
    done["text"] = part["text"].clone();
    done["logprobs"] = json!([]);
    send("response.output_text.done", done);
    let mut content = position;
    content["part"] = part.clone();
    send("response.content_part.done", content);
    message["status"] = json!("completed");
    message["content"] = json!([part]);
    send(
        "response.output_item.done",
        json!({"output_index": prefix.items.len(), "item": message}),
    );
    prefix.items.push(message);
    response["status"] = json!("completed");
    response["output"] = json!(prefix.items);
    // The interrupted generation did not report usage for the whole reply.
    response["usage"] = Value::Null;
    send("response.completed", json!({"response": response}));
    Ok(wire)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_prefix(transformer: &mut super::super::ResponsesSseTransformer) {
        for event in [
            json!({"type":"response.created","response":{"id":"resp_first","output":[]}}),
            json!({"type":"response.output_item.added","output_index":0,"item":{
                "id":"msg_first","type":"message","role":"assistant","content":[]}}),
            json!({"type":"response.content_part.added","output_index":0,"content_index":0,
                "item_id":"msg_first","part":{"type":"output_text","text":"","annotations":[]}}),
            json!({"type":"response.output_text.delta","output_index":0,"content_index":0,
                "item_id":"msg_first","delta":"The first sentence."}),
        ] {
            transformer.push_bytes(format!("data: {event}\n\n").as_bytes());
        }
    }

    #[test]
    fn continuation_retains_ids_and_deduplicates_the_visible_prefix() {
        let mut transformer =
            super::super::ResponsesSseTransformer::default().tracking_text_prefix(true);
        feed_prefix(&mut transformer);
        let events = splice(
            transformer.take_text_prefix().unwrap(),
            "The first sentence. And a second one.",
        )
        .unwrap();
        let events: Vec<_> = std::str::from_utf8(&events)
            .unwrap()
            .split("\n\n")
            .filter_map(super::super::sse_block_json)
            .collect();
        assert_eq!(events.len(), 5);
        assert_eq!(events[0]["delta"], " And a second one.");
        assert_eq!(events[0]["item_id"], "msg_first");
        let response = &events[4]["response"];
        assert_eq!(events[4]["type"], "response.completed");
        assert_eq!(response["id"], "resp_first");
        assert_eq!(response["output"][0]["id"], "msg_first");
        assert_eq!(
            response["output"][0]["content"][0]["text"],
            "The first sentence. And a second one."
        );
        assert!(response["usage"].is_null());
    }

    #[test]
    fn incomplete_frame_and_tool_item_prevent_continuation() {
        for tail in [
            b"data: {\"type\":\"response.output_text.delta\"".as_slice(),
            b"data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\"}}\n\n",
            b"data: [DONE]\n\n",
        ] {
            let mut transformer =
                super::super::ResponsesSseTransformer::default().tracking_text_prefix(true);
            feed_prefix(&mut transformer);
            transformer.push_bytes(tail);
            assert!(transformer.take_text_prefix().is_none());
        }
    }

    #[test]
    fn continuation_disables_tools_and_requires_inline_context() {
        let mut body = json!({"model":"test","input":"hello","tools":[{"type":"web_search"}],
            "tool_choice":"required","parallel_tool_calls":true});
        let prepared = request_body(&body).unwrap();
        assert_eq!(prepared["tools"], json!([]));
        assert_eq!(prepared["tool_choice"], "none");
        assert_eq!(prepared["parallel_tool_calls"], false);
        assert_eq!(prepared["input"][0]["content"], "hello");
        body["previous_response_id"] = json!("resp_prior");
        assert!(request_body(&body).is_none());
    }
}
