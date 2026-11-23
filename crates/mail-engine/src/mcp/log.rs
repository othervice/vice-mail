//! The access log ("Zugriffe"): every tool call and every refused connection the host reports
//! goes to the store (metadata only) and to the UI as `mcpAccess`. The host calls
//! [`Sink::record`] synchronously; a background task writes in batches, so a tool call never
//! waits for the database.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use mail_mcp::{AccessEvent, AccessKind, AccessLog, CallOutcome, ClientId, ClientKey, Refusal};
use mail_model::{CoreEvent, McpAccessOutcome, McpRefusal, McpTool};
use mail_store::NewMcpAccess;
use tokio::sync::{broadcast, mpsc};

use crate::core::{Connector, WeakCore};

/// Entries written in one transaction at most.
const BATCH: usize = 200;

/// The host's [`AccessLog`].
pub(crate) struct Sink {
    tx: mpsc::UnboundedSender<AccessEvent>,
    /// Every event, also those of the app's own connection test (which are not stored).
    pub(crate) observers: broadcast::Sender<AccessEvent>,
    /// The temporary clients of "test connection".
    pub(crate) probes: Arc<Mutex<HashMap<ClientId, ClientKey>>>,
}

impl Sink {
    pub(crate) fn new(
        probes: Arc<Mutex<HashMap<ClientId, ClientKey>>>,
    ) -> (Self, mpsc::UnboundedReceiver<AccessEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let (observers, _) = broadcast::channel(64);
        (
            Self {
                tx,
                observers,
                probes,
            },
            rx,
        )
    }
}

impl AccessLog for Sink {
    fn record(&self, event: AccessEvent) {
        let _ = self.observers.send(event.clone());
        let probe = event.client_id.is_some_and(|id| {
            self.probes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains_key(&id)
        });
        if probe {
            return;
        }
        // The rate limit of the host (120 calls per minute and connection, 16 connections)
        // bounds the queue.
        if self.tx.send(event).is_err() {
            tracing::warn!("MCP access log is closed; an entry was dropped");
        }
    }
}

pub(crate) fn tool(name: mail_mcp::ToolName) -> Option<McpTool> {
    McpTool::parse(name.as_str())
}

pub(crate) fn refusal(reason: Refusal) -> McpRefusal {
    match reason {
        Refusal::UnsupportedProtocol => McpRefusal::UnsupportedProtocol,
        Refusal::Disabled => McpRefusal::Disabled,
        Refusal::UnknownClient => McpRefusal::UnknownClient,
        Refusal::BadProof => McpRefusal::BadProof,
        Refusal::Malformed => McpRefusal::Malformed,
        Refusal::Timeout => McpRefusal::Timeout,
        Refusal::TooManyConnections => McpRefusal::TooManyConnections,
    }
}

fn row(event: AccessEvent) -> NewMcpAccess {
    let (tool, messages, outcome, refusal) = match event.kind {
        AccessKind::ToolCall {
            tool: name,
            messages,
            outcome,
        } => (
            tool(name),
            messages,
            match outcome {
                CallOutcome::Ok => McpAccessOutcome::Ok,
                CallOutcome::Error => McpAccessOutcome::Error,
                CallOutcome::Refused => McpAccessOutcome::Refused,
            },
            None,
        ),
        AccessKind::Refused { reason } => (
            None,
            0,
            McpAccessOutcome::Refused,
            Some(self::refusal(reason)),
        ),
    };
    NewMcpAccess {
        at: event.at,
        client_id: event.client_id.map(|id| id.to_string()),
        client_name: event.client_name,
        tool,
        messages,
        outcome,
        refusal,
    }
}

/// Writes the log until every [`Sink`] is gone. Holds the core only weakly and only while it
/// writes a batch, so dropping the core closes the database at once (as without stage 12).
pub(crate) async fn run<C: Connector>(
    core: WeakCore<C>,
    mut rx: mpsc::UnboundedReceiver<AccessEvent>,
) {
    while let Some(first) = rx.recv().await {
        let mut batch = vec![row(first)];
        while batch.len() < BATCH {
            match rx.try_recv() {
                Ok(event) => batch.push(row(event)),
                Err(_) => break,
            }
        }
        let Some(core) = core.upgrade() else { return };
        let Ok(store) = core.store_ref() else { return };
        match store.record_mcp_access(batch, core.now_ms()).await {
            Ok(entries) => core.emit_event(CoreEvent::McpAccess { entries }),
            Err(err) => tracing::error!(error = %err, "MCP access log could not be written"),
        }
    }
}
