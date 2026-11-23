//! Size and count limits of every tool call and of the IPC channel (decision 0015). One place,
//! so the tool descriptions, the argument checks and the tests agree.

/// Default and largest number of messages `search_messages` and `list_messages` return.
pub const DEFAULT_PAGE: u32 = 20;
pub const MAX_PAGE: u32 = 50;

/// Longest search query in bytes (the search language reads no more, `mail_model::search`).
pub const MAX_QUERY_BYTES: usize = mail_model::search::MAX_QUERY_BYTES;

/// Most mailboxes `list_mailboxes` returns (selectable ones, in the app's order).
pub const MAX_MAILBOXES: usize = 500;

/// Body text of `read_message`, in characters; longer bodies are cut and marked.
pub const MESSAGE_BODY_CHARS: usize = 30_000;

/// `read_thread`: the newest messages of the thread, each body at most this long, all bodies
/// together at most [`THREAD_TOTAL_BODY_CHARS`] (later messages get less once it is used up).
pub const THREAD_MESSAGES: u32 = 25;
pub const THREAD_BODY_CHARS: usize = 8_000;
pub const THREAD_TOTAL_BODY_CHARS: usize = 60_000;

/// Preview text of a list entry.
pub const PREVIEW_CHARS: usize = 200;
/// Subjects, display names, mailbox paths, attachment names.
pub const LINE_CHARS: usize = 300;
/// Addresses per header field (`to`, `cc`, ...); the rest is counted, not listed.
pub const ADDRESSES_PER_FIELD: usize = 50;
/// Attachments listed per message.
pub const ATTACHMENTS_PER_MESSAGE: usize = 50;

/// `create_draft`: recipients (`to` and `cc` together), subject and body.
pub const DRAFT_RECIPIENTS: usize = 50;
pub const DRAFT_SUBJECT_CHARS: usize = 500;
pub const DRAFT_BODY_CHARS: usize = 50_000;

/// Safety net for one tool result in bytes. The character budgets above keep results far below.
pub const MAX_RESULT_BYTES: usize = 768 * 1024;

/// Tool calls per connection and minute; more are answered with an error (the MCP spec asks
/// servers to rate limit tool invocations).
pub const CALLS_PER_MINUTE: usize = 120;

/// Longest time a tool call may take in the app (a body may have to be fetched from the server).
pub const CALL_TIMEOUT_SECS: u64 = 60;

/// Connections the app accepts at the same time (each AI client usually holds one).
pub const MAX_CONNECTIONS: usize = 16;

/// IPC lines: before authentication (handshake), from the helper (tool arguments) and from the
/// app (results). MCP lines from the AI client are bounded like helper lines.
pub const HANDSHAKE_LINE_BYTES: usize = 4 * 1024;
pub const REQUEST_LINE_BYTES: usize = 1024 * 1024;
pub const RESPONSE_LINE_BYTES: usize = 2 * 1024 * 1024;

/// Time the app waits for the helper to take an answer before it drops the connection.
pub const WRITE_TIMEOUT_SECS: u64 = 30;

/// Time for the whole handshake.
pub const HANDSHAKE_TIMEOUT_SECS: u64 = 5;
