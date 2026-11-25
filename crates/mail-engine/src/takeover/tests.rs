//! Stage 19 (decision 0057): taking over the fixture profile `testmail/thunderbird/` through
//! `AppCore`, with the fake IMAP server for the local folder's import.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mail_model::{
    ErrorCode, MailboxRole, MboxJobState, Security, TakeoverAccountKind, TakeoverToken,
};
use tempfile::TempDir;
use tokio::time::Instant;

use crate::compose::tests::with_core::{Bed, drafts_server};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testmail/thunderbird")
}

/// A copy of the fixture in which the password stores cannot be read at all: the takeover must
/// not need them (it never opens them).
fn copy_without_passwords(into: &Path) -> PathBuf {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).expect("dir");
        for entry in std::fs::read_dir(from).expect("read dir") {
            let entry = entry.expect("entry");
            let target = to.join(entry.file_name());
            if entry.file_type().expect("type").is_dir() {
                copy(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).expect("copy");
            }
        }
    }
    let base = into.join("Thunderbird");
    copy(&fixture(), &base);
    let profile = base.join("Profiles/t3stpr0f.default-release");
    for name in ["logins.json", "key4.db"] {
        let path = profile.join(name);
        std::fs::remove_file(&path).expect("remove");
        // A directory in its place: opening it as a file fails, reading it fails.
        std::fs::create_dir(&path).expect("unreadable stand-in");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        }
    }
    base
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_fixture_profile_lists_accounts_identities_signatures_and_local_folders() {
    let server = drafts_server();
    server.with(|s| s.append = true);
    let bed = Bed::on(server).await;
    let dir = TempDir::new().expect("temp dir");
    let base = copy_without_passwords(dir.path());

    let profiles = bed
        .core
        .thunderbird_profiles(base.clone())
        .await
        .expect("profiles");
    assert_eq!(profiles.len(), 2);
    let default = profiles.iter().find(|p| p.is_default).expect("default");
    let preview = bed
        .core
        .open_thunderbird_profile(base.clone(), default.index)
        .await
        .expect("open");
    assert_eq!(preview.profile_name, "t3stpr0f.default-release");
    assert_eq!(
        preview.accounts.len(),
        2,
        "IMAP and POP3; Local Folders are folders"
    );
    let imap = &preview.accounts[0];
    assert_eq!(imap.kind, TakeoverAccountKind::Imap);
    assert_eq!(imap.email, "alice@vicemail.test");
    assert_eq!(imap.display_name, "Alice Example");
    let incoming = imap.imap.as_ref().expect("imap");
    assert_eq!(
        (incoming.host.as_str(), incoming.port, incoming.security),
        ("127.0.0.1", 1993, Security::Tls)
    );
    let outgoing = imap.smtp.as_ref().expect("smtp");
    assert_eq!((outgoing.port, outgoing.security), (1465, Security::Tls));
    assert_eq!(imap.identities.len(), 2);
    let html = imap.identities[0]
        .signature
        .as_ref()
        .expect("html signature");
    assert!(
        html.html && html.preview.contains("Example Ltd."),
        "{html:?}"
    );
    let file = imap.identities[1]
        .signature
        .as_ref()
        .expect("file signature");
    assert!(file.from_file && file.preview.contains("Sent from the fixture"));
    assert_eq!(preview.accounts[1].kind, TakeoverAccountKind::Pop3);
    assert!(preview.accounts[1].imap.is_none(), "POP3 is not taken over");
    let folders: Vec<&str> = preview.folders.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        folders,
        ["Archives/2025", "Inbox"],
        "empty files are left out"
    );

    // The account was set up by the user (normal setup); now its identities and signatures.
    let applied = bed
        .core
        .apply_thunderbird_account(preview.token.clone(), imap.index, bed.account)
        .await
        .expect("apply");
    assert_eq!((applied.identities, applied.signatures), (2, 2));
    let identities = bed
        .core
        .list_identities(bed.account)
        .await
        .expect("identities");
    let alias = identities
        .iter()
        .find(|i| i.email == "alice.alias@vicemail.test")
        .expect("alias");
    let defaults = bed
        .core
        .get_identity_signature_defaults(alias.id)
        .await
        .expect("defaults");
    assert!(defaults.new_signature.is_some() && defaults.reply_signature.is_some());
    let signatures = bed.core.list_signatures().await.expect("signatures");
    assert_eq!(signatures.len(), 2);

    // A local folder into the Archive folder, through the outbox.
    let archive = bed
        .core
        .list_mailboxes(bed.account)
        .await
        .expect("mailboxes")
        .into_iter()
        .find(|m| m.role == MailboxRole::Archive)
        .expect("archive")
        .id;
    let job = bed
        .core
        .import_thunderbird_folder(preview.token.clone(), 0, archive)
        .await
        .expect("import");
    let deadline = Instant::now() + Duration::from_secs(20);
    let job = loop {
        let now = bed
            .core
            .list_mbox_jobs()
            .into_iter()
            .find(|j| j.id == job.id)
            .expect("job");
        if now.state != MboxJobState::Running {
            break now;
        }
        assert!(Instant::now() < deadline, "{now:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!((job.state, job.done), (MboxJobState::Done, 2), "{job:?}");

    let unknown = bed
        .core
        .apply_thunderbird_account(TakeoverToken::from_random(1), 0, bed.account)
        .await
        .expect_err("unknown token");
    assert_eq!(unknown.code(), ErrorCode::NotFound);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_chosen_folder_may_be_the_profile_or_thunderbirds_own() {
    let server = drafts_server();
    let bed = Bed::on(server).await;
    let dir = TempDir::new().expect("temp dir");
    let base = copy_without_passwords(dir.path());
    let by_base = bed
        .core
        .open_thunderbird_dir(base.clone())
        .await
        .expect("base");
    assert_eq!(
        by_base.profile_name, "t3stpr0f.default-release",
        "the default"
    );
    let by_profile = bed
        .core
        .open_thunderbird_dir(base.join("Profiles/0ldpr0f.default"))
        .await
        .expect("profile");
    assert!(by_profile.accounts.is_empty());
    let err = bed
        .core
        .open_thunderbird_dir(dir.path().to_owned())
        .await
        .expect_err("not a profile");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
    assert!(
        bed.core
            .thunderbird_profiles(dir.path().join("none"))
            .await
            .expect("none")
            .is_empty()
    );
}
