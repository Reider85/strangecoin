//! `subscribe_events(filter)` — spec §2.7. Transport split: on **stdio** the
//! filter is handed to the serve loop, which pushes signed notifications on
//! stdout; on **HTTP+SSE** the ack points at the SSE endpoint (events flow
//! on `GET /mcp/events`, filter travels as query params there).

use serde_json::{json, Value};

use crate::{EventFilter, McpBackend, McpError, RequestCtx, TransportKind};

pub fn schema() -> Value {
    json!({
        "name": "subscribe_events",
        "description": "Subscribe to chain events via SSE stream.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "filter": {
                    "type": "object",
                    "properties": {
                        "event_types": {"type": "array", "items": {"type": "string", "enum": ["BlockApplied", "TxConfirmed", "EventLog"]}},
                        "addresses": {"type": "array", "items": {"type": "string"}},
                        "event_sigs": {"type": "array", "items": {"type": "string"}}
                    }
                }
            }
        },
        "outputSchema": {
            "type": "object",
            "description": "SSE stream — each event is one JSON object",
            "properties": {
                "event_type": {"type": "string"},
                "data": {"type": "object"}
            }
        }
    })
}

fn parse_filter(args: &Value) -> Result<EventFilter, McpError> {
    let Some(filter) = args.get("filter") else {
        return Ok(EventFilter::default());
    };
    let list = |key: &str| -> Result<Option<Vec<String>>, McpError> {
        match filter.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| {
                    item.as_str().map(str::to_string).ok_or_else(|| {
                        McpError::InvalidParams(format!("`filter.{key}` items must be strings"))
                    })
                })
                .collect::<Result<Vec<String>, McpError>>()
                .map(Some),
            Some(_) => Err(McpError::InvalidParams(format!(
                "`filter.{key}` must be an array of strings"
            ))),
        }
    };
    EventFilter::from_parts(
        list("event_types")?,
        list("addresses")?,
        list("event_sigs")?,
    )
}

pub fn call(backend: &dyn McpBackend, ctx: &RequestCtx, args: &Value) -> Result<Value, McpError> {
    let filter = parse_filter(args)?;
    match ctx.transport {
        TransportKind::Stdio => {
            // The serve loop drains this after writing the ack, so the
            // subscription never races the response (spec §4.1).
            ctx.set_subscription(filter);
            let _ = backend; // subscription is created by the serve loop
            Ok(json!({ "subscribed": true }))
        }
        TransportKind::HttpSse => Ok(json!({
            "subscribed": true,
            "stream": "/mcp/events",
            "note": "connect GET /mcp/events with your SSE session id; \
                     filter via ?types=&addresses=&event_sigs= query params"
        })),
    }
}
