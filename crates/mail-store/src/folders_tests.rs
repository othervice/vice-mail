//! Audit 24.11.2026: renaming and deleting folders through the outbox, in the store: what is
//! refused (read-only accounts, folders with a role, taken names, subfolders, a change on its
//! way), what is recorded, and the rows after the server confirmed a rename.

use mail_model::{
    ActionError, ActionKind, MailboxId, MailboxRole, OperationKind, OperationPayload,
};
use rusqlite::Connection;

use crate::test_support::{Fixture, fixture, remote};
use crate::{StoreError, folders};

/// The fixture with writes on and two folders of the user's own: `Projekte` and
/// `Projekte/2026`, with a favourite and a colour on them.
async fn setup() -> (Fixture, MailboxId, MailboxId) {
    let f = fixture().await;
    f.store
        .sync_mailboxes(
            f.account,
            vec![
                remote("INBOX", &[]),
                remote("Archive", &["\\Archive"]),
                remote("Sent", &["\\Sent"]),
                remote("Projekte", &[]),
                remote("Projekte/2026", &[]),
            ],
        )
        .await
        .expect("mailboxes");
    let boxes = f.store.mailboxes(f.account).await.expect("boxes");
    let id = |path: &str| {
        boxes
            .iter()
            .find(|m| m.path == path)
            .map(|m| m.id)
            .expect(path)
    };
    let (projects, year) = (id("Projekte"), id("Projekte/2026"));
    f.store
        .set_writes_enabled(f.account, true)
        .await
        .expect("writes");
    f.store
        .set_mailbox_favorite(projects, true, 1)
        .await
        .expect("favourite");
    f.store
        .set_mailbox_color(
            year,
            Some(mail_model::AccountColor::new(3).expect("colour")),
            1,
        )
        .await
        .expect("colour");
    (f, projects, year)
}

fn action(err: StoreError) -> Option<ActionError> {
    match err {
        StoreError::Action(action) => Some(action),
        _ => None,
    }
}

#[tokio::test]
async fn a_rename_is_recorded_once_and_its_rows_follow_the_confirmed_name() {
    let (f, projects, year) = setup().await;
    let change = f
        .store
        .enqueue_rename_mailbox(f.account, projects, " Kunden ".into(), "req-1".into())
        .await
        .expect("rename");
    assert_eq!(change.operations.len(), 1);
    let op = &change.operations[0];
    assert_eq!(op.payload.kind(), OperationKind::RenameMailbox);
    assert_eq!(op.action, ActionKind::RenameMailbox);
    assert_eq!(
        op.mailbox_id,
        Some(projects),
        "the folder, for its pending state"
    );
    // Nothing changes locally before the server confirmed.
    let boxes = f.store.mailboxes(f.account).await.expect("boxes");
    assert!(boxes.iter().any(|m| m.path == "Projekte"));
    // The same request again is the same batch; another change waits for this one.
    let again = f
        .store
        .enqueue_rename_mailbox(f.account, projects, "Kunden".into(), "req-1".into())
        .await
        .expect("repeated");
    assert!(again.repeated);
    let busy = f
        .store
        .enqueue_delete_mailbox(f.account, year, "req-2".into())
        .await;
    assert!(busy.is_ok(), "another folder may change meanwhile");
    let twice = f
        .store
        .enqueue_rename_mailbox(f.account, projects, "Archiv alt".into(), "req-3".into())
        .await
        .expect_err("one change at a time");
    assert!(matches!(twice, StoreError::InvalidArgument(_)), "{twice:?}");
    let ops = f.store.operations(f.account, None, 10).await.expect("ops");
    let payload = ops
        .iter()
        .find(|o| o.payload.kind() == OperationKind::RenameMailbox)
        .expect("recorded");
    assert_eq!(payload.mailbox_id, Some(projects));

    // Confirmed: the folder and its subfolder take the new names; favourite and colour move.
    assert!(
        f.store
            .rename_mailbox_rows(projects, "Kunden".into(), "Kunden".into())
            .await
            .expect("rows")
    );
    let boxes = f.store.mailboxes(f.account).await.expect("boxes");
    let renamed = boxes.iter().find(|m| m.id == projects).expect("same row");
    assert_eq!(renamed.path, "Kunden");
    assert_eq!(renamed.favorite_position, Some(0));
    let child = boxes.iter().find(|m| m.id == year).expect("same row");
    assert_eq!(child.path, "Kunden/2026");
    let wire = |id: MailboxId| {
        let store = f.store.clone();
        async move {
            store
                .read(move |c| {
                    Ok(
                        c.query_row("SELECT name FROM mailbox WHERE id = ?1", [id.0], |r| {
                            r.get::<_, String>(0)
                        })?,
                    )
                })
                .await
                .expect("wire name")
        }
    };
    assert_eq!(wire(projects).await, "Kunden");
    assert_eq!(wire(year).await, "Kunden/2026");
    assert_eq!(child.color, mail_model::AccountColor::new(3));
}

#[tokio::test]
async fn what_may_not_be_renamed_or_deleted_is_refused_before_anything_is_recorded() {
    let (f, projects, year) = setup().await;
    let boxes = f.store.mailboxes(f.account).await.expect("boxes");
    let role = |r| {
        boxes
            .iter()
            .find(|m| m.role == r)
            .map(|m| m.id)
            .expect("role")
    };

    // Folders with a role, the inbox included.
    for mailbox in [role(MailboxRole::Inbox), role(MailboxRole::Archive)] {
        let err = f
            .store
            .enqueue_rename_mailbox(f.account, mailbox, "X".into(), format!("r-{mailbox}"))
            .await
            .expect_err("role");
        assert!(matches!(action(err), Some(ActionError::MailboxHasRole(_))));
        let err = f
            .store
            .enqueue_delete_mailbox(f.account, mailbox, format!("d-{mailbox}"))
            .await
            .expect_err("role");
        assert!(matches!(action(err), Some(ActionError::MailboxHasRole(_))));
    }
    // A taken name, an empty one, one with the delimiter.
    let err = f
        .store
        .enqueue_rename_mailbox(f.account, projects, "Sent".into(), "n1".into())
        .await
        .expect_err("taken");
    assert!(matches!(action(err), Some(ActionError::MailboxExists(_))));
    for bad in ["", "  ", "a/b", "tab\there"] {
        let err = f
            .store
            .enqueue_rename_mailbox(f.account, projects, bad.into(), format!("bad-{bad}"))
            .await
            .expect_err("invalid");
        assert!(
            matches!(err, StoreError::InvalidArgument(_)),
            "{bad:?}: {err:?}"
        );
    }
    // A folder with subfolders is not deleted; its leaf is.
    let err = f
        .store
        .enqueue_delete_mailbox(f.account, projects, "d1".into())
        .await
        .expect_err("subfolders");
    assert!(matches!(
        action(err),
        Some(ActionError::MailboxHasSubfolders(_))
    ));
    let deleted = f
        .store
        .enqueue_delete_mailbox(f.account, year, "d2".into())
        .await
        .expect("leaf");
    assert!(matches!(
        deleted.operations[0].payload.kind(),
        OperationKind::DeleteMailbox
    ));
    // Read-only: refused, nothing recorded.
    f.store
        .set_writes_enabled(f.account, false)
        .await
        .expect("writes off");
    let before = f
        .store
        .operations(f.account, None, 50)
        .await
        .expect("ops")
        .len();
    for result in [
        f.store
            .enqueue_rename_mailbox(f.account, projects, "Neu".into(), "ro1".into())
            .await,
        f.store
            .enqueue_delete_mailbox(f.account, projects, "ro2".into())
            .await,
    ] {
        let err = result.expect_err("read-only");
        assert!(matches!(action(err), Some(ActionError::ReadOnly(_))));
    }
    assert_eq!(
        f.store
            .operations(f.account, None, 50)
            .await
            .expect("ops")
            .len(),
        before
    );
}

#[test]
fn the_payloads_round_trip() {
    let payload = OperationPayload::RenameMailbox {
        mailbox: mail_model::TargetMailbox {
            mailbox_id: MailboxId(3),
            name: "Projekte".into(),
        },
        path: "Projekte".into(),
        new_path: "Kunden".into(),
    };
    let text = serde_json::to_string(&payload).expect("json");
    assert_eq!(
        serde_json::from_str::<OperationPayload>(&text).expect("back"),
        payload
    );
}

fn plan(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap_or_else(|err| panic!("prepare {sql}: {err}"));
    let nulls = vec![rusqlite::types::Null; stmt.parameter_count()];
    stmt.query_map(rusqlite::params_from_iter(nulls), |r| r.get::<_, String>(3))
        .expect("plan")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows")
}

#[tokio::test]
async fn the_folder_queries_use_their_index() {
    let (f, ..) = setup().await;
    f.store
        .read(|conn| {
            for (sql, want) in [
                (
                    folders::FOLDER,
                    "SEARCH mailbox USING INTEGER PRIMARY KEY (rowid=?)",
                ),
                (
                    folders::HAS_SUBFOLDER,
                    "SEARCH mailbox USING COVERING INDEX mailbox_parent (parent_id=?)",
                ),
                (
                    folders::PATH_TAKEN,
                    "SEARCH mailbox USING COVERING INDEX mailbox_path (account_id=? AND path=?)",
                ),
                (
                    folders::PENDING_CHANGE,
                    "SEARCH operation USING INDEX operation_location (mailbox_id=?)",
                ),
            ] {
                let lines = plan(conn, sql);
                assert!(
                    lines.iter().any(|l| l.starts_with(want)),
                    "{sql}: {lines:#?}"
                );
            }
            Ok(())
        })
        .await
        .expect("plans");
}
