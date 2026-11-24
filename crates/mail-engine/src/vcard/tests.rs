//! Stage 19 (decision 0056): vCard import and export through `AppCore` with the real store; the
//! picture step runs in the process (no helper in tests, as for chosen pictures).

use mail_model::{ContactExport, ContactFilter, VcardImportProblemKind, VcardVersionChoice};
use tempfile::TempDir;

use crate::credentials::{Credentials, MemorySecretStore};
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices};

/// A 4 × 2 red PNG.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAQAAAACCAIAAADwyuo0AAAAEElEQVR4nGP4z8AARwzIHABvqgf5gNwAKAAAAABJRU5ErkJggg==";

async fn core(dir: &TempDir) -> AppCore {
    let core = AppCore::start(
        CoreConfig {
            data_dir: dir.path().join("data"),
            build: BuildInfo {
                version: "0".into(),
                commit: None,
                build_date: None,
            },
        },
        CoreServices::new(Credentials::new(MemorySecretStore::new())),
    )
    .await;
    assert!(core.startup_error().is_none());
    core
}

fn file(dir: &TempDir, name: &str, text: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, text).expect("write");
    path
}

fn sample() -> String {
    format!(
        "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:aaaa-1\r\nFN:Ada Lovelace\r\nN:Lovelace;Ada;;;\r\n\
EMAIL;TYPE=INTERNET,pref:ada@example.org\r\nEMAIL;TYPE=INTERNET:ada@work.example\r\n\
NOTE:Met at the\\, conference\r\nPHOTO;ENCODING=b;TYPE=PNG:{PNG}\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:4.0\r\nUID:urn:uuid:bbbb-2\r\nFN:Grace Hopper\r\nEMAIL:grace@example.org\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:4.0\r\nKIND:group\r\nFN:Pioneers\r\nMEMBER:urn:uuid:aaaa-1\r\nMEMBER:urn:uuid:bbbb-2\r\n\
MEMBER:mailto:linus@example.org\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:3.0\r\nFN:No address\r\nEND:VCARD\r\n"
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn imported_cards_become_contacts_and_groups_and_duplicates_merge() {
    let dir = TempDir::new().expect("temp dir");
    let core = core(&dir).await;
    let report = core
        .import_vcard_file(file(&dir, "a.vcf", &sample()))
        .await
        .expect("import");
    assert_eq!(report.cards, 4);
    assert_eq!(
        report
            .problems
            .iter()
            .map(|p| (p.card, p.kind))
            .collect::<Vec<_>>(),
        vec![(3, VcardImportProblemKind::NoAddress)]
    );
    assert_eq!(report.created, 2);
    assert_eq!(report.lists, 1);
    assert_eq!(report.pictures, 1);
    assert_eq!(report.merged, 1, "two addresses of one card");
    let ada = core
        .get_contact("ada@work.example".into())
        .await
        .expect("ada");
    assert_eq!(ada.name.as_deref(), Some("Ada Lovelace"));
    assert_eq!(ada.email, "ada@example.org", "the preferred address leads");
    assert_eq!(ada.addresses.len(), 2);
    assert_eq!(ada.note.as_deref(), Some("Met at the, conference"));
    assert!(ada.picture.is_some(), "the picture, encoded again");
    let picture = core
        .get_contact_picture("ada@example.org".into())
        .await
        .expect("picture")
        .expect("one");
    assert!(picture.data_url.starts_with("data:image/jpeg;base64,"));

    let lists = core.list_contact_lists().await.expect("lists");
    assert_eq!(lists.len(), 1);
    let list = core
        .get_contact_list(lists[0].id.clone())
        .await
        .expect("list");
    assert_eq!(list.name, "Pioneers");
    let emails: Vec<&str> = list.members.iter().map(|m| m.email.as_str()).collect();
    assert_eq!(
        emails,
        ["ada@example.org", "grace@example.org", "linus@example.org"]
    );

    // Again, with a new address for Grace and a different name: merged, the name stays.
    let again = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Amazing Grace\r\nEMAIL:grace@example.org\r\n\
EMAIL:grace@navy.example\r\nNOTE:Admiral\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:4.0\r\nKIND:group\r\nFN:pioneers\r\nMEMBER:mailto:grace@navy.example\r\nEND:VCARD\r\n";
    let report = core
        .import_vcard_file(file(&dir, "b.vcf", again))
        .await
        .expect("import again");
    assert_eq!(report.created, 0);
    assert_eq!(report.updated, 1);
    let grace = core
        .get_contact("grace@navy.example".into())
        .await
        .expect("grace");
    assert_eq!(
        grace.name.as_deref(),
        Some("Grace Hopper"),
        "a name is never overwritten"
    );
    assert_eq!(
        grace.note.as_deref(),
        Some("Admiral"),
        "an empty note is filled"
    );
    assert_eq!(grace.addresses.len(), 2);
    let lists = core.list_contact_lists().await.expect("lists");
    assert_eq!(lists.len(), 1, "the group of the same name is extended");
    assert_eq!(lists[0].member_count, 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_export_imports_into_an_empty_installation_as_it_was() {
    for version in [VcardVersionChoice::V3, VcardVersionChoice::V4] {
        let dir = TempDir::new().expect("temp dir");
        let here = core(&dir).await;
        here.import_vcard_file(file(&dir, "a.vcf", &sample()))
            .await
            .expect("import");
        let out = dir.path().join("out.vcf");
        let cards = here
            .export_vcards(
                ContactExport::All { people_only: true },
                version,
                out.clone(),
            )
            .await
            .expect("export");
        assert_eq!(cards, 3, "two contacts, one group");
        let text = std::fs::read_to_string(&out).expect("read");
        assert!(text.contains(match version {
            VcardVersionChoice::V3 => "VERSION:3.0",
            VcardVersionChoice::V4 => "VERSION:4.0",
        }));

        let other = TempDir::new().expect("temp dir");
        let there = core(&other).await;
        let report = there.import_vcard_file(out).await.expect("import export");
        assert_eq!(report.created, 2, "{version:?}");
        assert_eq!(report.pictures, 1);
        let page = there
            .list_contacts(None, None, 50, ContactFilter::People)
            .await
            .expect("page");
        let mut names: Vec<String> = page.items.iter().filter_map(|c| c.name.clone()).collect();
        names.sort();
        assert_eq!(names, ["Ada Lovelace", "Grace Hopper"]);
        let lists = there.list_contact_lists().await.expect("lists");
        assert_eq!(lists.len(), 1);
        let list = there
            .get_contact_list(lists[0].id.clone())
            .await
            .expect("list");
        let emails: Vec<&str> = list.members.iter().map(|m| m.email.as_str()).collect();
        assert_eq!(
            emails,
            ["ada@example.org", "grace@example.org", "linus@example.org"]
        );

        // One group: its card and a card per member (linus has no contact: a small card).
        let id = lists[0].id.clone();
        let one = other.path().join("group.vcf");
        let written = there
            .export_vcards(ContactExport::List { id }, version, one.clone())
            .await
            .expect("group export");
        assert_eq!(written, 4);
        let single = other.path().join("ada.vcf");
        assert_eq!(
            there
                .export_vcards(
                    ContactExport::Contact {
                        email: "ada@work.example".into()
                    },
                    version,
                    single.clone()
                )
                .await
                .expect("one"),
            1
        );
        let text = std::fs::read_to_string(single).expect("read");
        assert!(
            text.contains("ada@work.example") && text.contains("PHOTO"),
            "{text}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn broken_files_report_problems_and_never_fail_the_import() {
    let dir = TempDir::new().expect("temp dir");
    let core = core(&dir).await;
    let text = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Link Photo\r\nEMAIL:link@example.org\r\n\
PHOTO;VALUE=uri:https://tracker.example/p.png\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Broken\r\nEMAIL:not an address\r\nEND:VCARD\r\n\
garbage line\r\nBEGIN:VCARD\r\nFN:Cut off\r\nEMAIL:cut@example.org\r\n";
    let report = core
        .import_vcard_file(file(&dir, "c.vcf", text))
        .await
        .expect("import");
    assert_eq!(report.created, 2, "link@ and cut@");
    let kinds: Vec<VcardImportProblemKind> = report.problems.iter().map(|p| p.kind).collect();
    assert!(
        kinds.contains(&VcardImportProblemKind::PictureSkipped),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&VcardImportProblemKind::NoAddress),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&VcardImportProblemKind::Malformed),
        "{kinds:?}"
    );
    let link = core
        .get_contact("link@example.org".into())
        .await
        .expect("link");
    assert!(link.picture.is_none(), "never loaded from the network");
    // Not a file: refused.
    let err = core
        .import_vcard_file(dir.path().to_owned())
        .await
        .expect_err("a folder");
    assert_eq!(err.code(), mail_model::ErrorCode::InvalidArgument);
}
