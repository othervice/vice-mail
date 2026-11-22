//! Commands exposed to the UI. In the product these only forward to `AppCore`; here they return
//! a fixed spike fixture so the IPC shape can be inspected.

use std::time::Duration;

use chrono::{TimeZone, Utc};
use tauri::AppHandle;
use tauri_specta::Event;

use crate::model::{
    Address, Attachment, Body, CoreEvent, ErrorCode, Flag, IpcError, MessageDetail, MessageQuery,
};

/// 2^53 + 1: the smallest integer a JavaScript `number` cannot hold.
pub const DEMO_MOD_SEQ: u64 = 9_007_199_254_740_993;

#[tauri::command]
#[specta::specta]
pub fn get_message(query: MessageQuery) -> Result<MessageDetail, IpcError> {
    if query.account_id.trim().is_empty() {
        return Err(IpcError::new(
            ErrorCode::InvalidArgument,
            "Account id must not be empty.",
            None,
        ));
    }
    if query.uid == 0 {
        return Err(IpcError::new(
            ErrorCode::NotFound,
            "Message not found.",
            Some(format!(
                "uid 0 in account {} (UIDs start at 1)",
                query.account_id
            )),
        ));
    }

    let body = if query.include_body {
        Body::Html {
            sanitized_html: "<p>Spike fixture body</p>".into(),
            blocked_remote_images: 2,
        }
    } else {
        Body::NotLoaded
    };

    Ok(MessageDetail {
        account_id: query.account_id,
        mailbox: "INBOX".into(),
        uid: query.uid,
        mod_seq: DEMO_MOD_SEQ,
        size_bytes: 48_213,
        internal_date_ms: 1_795_000_000_000,
        sent_at: Utc.with_ymd_and_hms(2026, 11, 20, 9, 30, 0).single(),
        subject: Some("Spike fixture".into()),
        from: Address {
            name: Some("Alice Example".into()),
            email: "alice@example.test".into(),
        },
        to: vec![Address {
            name: None,
            email: "bob@example.test".into(),
        }],
        cc: Vec::new(),
        flags: vec![Flag::Seen, Flag::Keyword("$Label1".into())],
        body,
        attachments: vec![Attachment {
            part_id: "2".into(),
            file_name: Some("report.pdf".into()),
            mime_type: "application/pdf".into(),
            size_bytes: 31_337,
            content_id: None,
        }],
    })
}

/// Emits a few `CoreEvent`s from a background thread, like a sync run would.
#[tauri::command]
#[specta::specta]
pub fn start_demo_sync(app: AppHandle, account_id: String) -> Result<(), IpcError> {
    if account_id.trim().is_empty() {
        return Err(IpcError::new(
            ErrorCode::InvalidArgument,
            "Account id must not be empty.",
            None,
        ));
    }
    std::thread::spawn(move || {
        let mailbox = "INBOX".to_string();
        let total = 5;
        for done in 1..=total {
            std::thread::sleep(Duration::from_millis(120));
            let event = CoreEvent::SyncProgress {
                account_id: account_id.clone(),
                mailbox: mailbox.clone(),
                done,
                total,
            };
            if let Err(err) = event.emit(&app) {
                eprintln!("[core] failed to emit event: {err}");
                return;
            }
        }
        let changed = CoreEvent::MessagesChanged {
            account_id,
            mailbox,
            uids: vec![41, 42],
        };
        if let Err(err) = changed.emit(&app) {
            eprintln!("[core] failed to emit event: {err}");
        }
    });
    Ok(())
}

/// Lets the UI report its smoke-test result to stdout. With `SPIKE_SELFTEST=1` the app exits
/// afterwards (exit code 0 on success), which makes the release build checkable from a shell.
#[tauri::command]
#[specta::specta]
pub fn report_frontend(app: AppHandle, ok: bool, summary: String) {
    println!("[frontend] ok={ok} {summary}");
    if std::env::var("SPIKE_SELFTEST").as_deref() == Ok("1") {
        app.exit(if ok { 0 } else { 1 });
    }
}
