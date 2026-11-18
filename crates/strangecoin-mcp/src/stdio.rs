//! stdio transport (spec §4.1): newline-delimited JSON-RPC over a duplex
//! byte stream — stdin/stdout in production, `tokio::io::duplex` in tests.
//!
//! Security model is process isolation (the MCP client spawns the node as a
//! child); there is no auth on this transport by design (ADR-0029
//! Alternatives 1/2).

use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

use crate::{
    signing, EventFilter, McpError, McpServer, RequestCtx, TransportKind, MAX_MESSAGE_BYTES,
};

/// Serve MCP over `reader`/`writer` until EOF or a `shutdown` request.
///
/// `subscribe_events` acks first, then a spawned task pushes signed
/// `notifications/strangecoin/event` lines for as long as the connection
/// lives (the push stops on the first write error — i.e. client exit).
pub async fn serve_stdio<R, W>(server: Arc<McpServer>, reader: R, writer: W) -> Result<(), McpError>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let writer = Arc::new(tokio::sync::Mutex::new(writer));
    let ctx = RequestCtx::new(TransportKind::Stdio);
    let mut lines = BufReader::new(reader).lines();
    let mut subscription_spawned = false;

    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break, // EOF — client went away.
            Err(err) => return Err(McpError::Transport(format!("stdin read failed: {err}"))),
        };
        if line.len() > MAX_MESSAGE_BYTES {
            let error = server.signed_error(
                serde_json::Value::Null,
                &McpError::InvalidRequest(format!(
                    "message exceeds {MAX_MESSAGE_BYTES} byte limit"
                )),
            );
            let bytes =
                serde_json::to_vec(&error).map_err(|err| McpError::Internal(err.to_string()))?;
            write_line(&writer, &bytes).await?;
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => server.handle(request, &ctx),
            Err(_) => Some(server.parse_error_response()),
        };
        if let Some(response) = response {
            let bytes =
                serde_json::to_vec(&response).map_err(|err| McpError::Internal(err.to_string()))?;
            write_line(&writer, &bytes).await?;
        }

        // The ack line is already on the wire when the push task starts, so
        // the first event can never overtake the subscription response.
        if !subscription_spawned {
            if let Some(filter) = ctx.take_subscription() {
                subscription_spawned = true;
                let push_server = Arc::clone(&server);
                let push_writer = Arc::clone(&writer);
                tokio::spawn(async move {
                    push_events(push_server, filter, push_writer).await;
                });
            }
        }

        if server.shutdown_requested() {
            break;
        }
    }
    Ok(())
}

async fn write_line<W: AsyncWrite + Unpin>(
    writer: &Arc<tokio::sync::Mutex<W>>,
    bytes: &[u8],
) -> Result<(), McpError> {
    let mut guard = writer.lock().await;
    guard
        .write_all(bytes)
        .await
        .map_err(|err| McpError::Transport(format!("stdout write failed: {err}")))?;
    guard
        .write_all(b"\n")
        .await
        .map_err(|err| McpError::Transport(format!("stdout write failed: {err}")))?;
    guard
        .flush()
        .await
        .map_err(|err| McpError::Transport(format!("stdout flush failed: {err}")))?;
    Ok(())
}

/// Push matching events as signed JSON-RPC notifications until the backend
/// stream ends or the client disconnects.
async fn push_events<W: AsyncWrite + Unpin + Send + 'static>(
    server: Arc<McpServer>,
    filter: EventFilter,
    writer: Arc<tokio::sync::Mutex<W>>,
) {
    let mut events = server.backend().subscribe_events();
    while let Some(event) = events.recv().await {
        if !filter.matches(&event) {
            continue;
        }
        let event_value = match serde_json::to_value(&event) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let signature = signing::sign(server.signer(), &event_value);
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/strangecoin/event",
            "params": {
                "event": event_value,
                "signature": signature,
                "signer": hex::encode(server.signer().public_key()),
            }
        });
        let Ok(bytes) = serde_json::to_vec(&notification) else {
            continue;
        };
        if write_line(&writer, &bytes).await.is_err() {
            break; // Client disconnected — stop pushing.
        }
    }
}
