//! Stage 22 (decision 0074) through [`AppCore`]: search inside words with highlights, the
//! index state deciding how the trigram index is used, `contact:` with every address of the
//! contact (merged ones too, also in a saved search and in a list view's quick filter), and
//! contact suggestions for the search field.

use mail_model::search::Substrings;
use mail_model::{
    Address, ContactFilter, ContactMergeRequest, InboxScope, ListOrder, ListViewRequest,
    MessageHeader, QuickFilter, SearchIndexState, SearchIndexStatus, SearchScope, SearchTermKind,
    TextRange,
};

use super::resolve;
use super::tests::{fixture, header, request, subjects};

fn addressed(mut h: MessageHeader, to: &[(&str, &str)], cc: &[(&str, &str)]) -> MessageHeader {
    let list = |items: &[(&str, &str)]| {
        items
            .iter()
            .map(|(name, email)| Address {
                name: (!name.is_empty()).then(|| (*name).to_owned()),
                email: (*email).to_owned(),
            })
            .collect::<Vec<_>>()
    };
    h.to = list(to);
    h.cc = list(cc);
    h
}

const D: i64 = 1_790_000_000;

#[tokio::test]
async fn words_match_inside_words_with_marks() {
    let f = fixture().await;
    let store = f.core.store_for_tests();
    let (_, inbox, _) = f.accounts[0];
    store
        .store_headers(
            inbox,
            super::tests::UV,
            vec![
                header(
                    1,
                    D,
                    "Muster für dich",
                    ("Max Mustermann", "max.mustermann@firma.example"),
                    "alice@example.org",
                ),
                header(
                    2,
                    D + 60,
                    "Bestellung AB1234XY",
                    ("Shop", "shop@laden.example"),
                    "alice@example.org",
                ),
                header(
                    3,
                    D + 120,
                    "Kunde und Hund",
                    ("Kunde", "kunde@laden.example"),
                    "alice@example.org",
                ),
            ],
            None,
        )
        .await
        .expect("headers");
    let page = f
        .core
        .search(request("uster", SearchScope::All))
        .await
        .expect("search");
    assert_eq!(subjects(&page), ["Muster für dich"]);
    let hit = &page.hits[0];
    assert_eq!(hit.highlights.subject, [TextRange { start: 1, end: 6 }]);
    assert_eq!(hit.highlights.from, [TextRange { start: 5, end: 10 }]);
    assert_eq!(page.index.state, SearchIndexState::Ready);

    let page = f
        .core
        .search(request("1234", SearchScope::All))
        .await
        .expect("search");
    assert_eq!(subjects(&page), ["Bestellung AB1234XY"]);
    assert_eq!(
        page.hits[0].highlights.subject,
        [TextRange { start: 13, end: 17 }]
    );
    // While typing too; quoted terms are words only.
    let mut typing = request("bestell", SearchScope::All);
    typing.typing = true;
    let page = f.core.search(typing).await.expect("search");
    assert_eq!(subjects(&page), ["Bestellung AB1234XY"]);
    let page = f
        .core
        .search(request("und", SearchScope::All))
        .await
        .expect("search");
    assert_eq!(subjects(&page), ["Kunde und Hund"]);
    assert_eq!(
        page.hits[0].highlights.subject,
        [
            TextRange { start: 1, end: 4 },
            TextRange { start: 6, end: 9 },
            TextRange { start: 11, end: 14 }
        ]
    );
    let page = f
        .core
        .search(request("\"und\"", SearchScope::All))
        .await
        .expect("search");
    assert_eq!(
        page.hits[0].highlights.subject,
        [TextRange { start: 6, end: 9 }]
    );
    let page = f
        .core
        .search(request("\"uster\"", SearchScope::All))
        .await
        .expect("search");
    assert!(page.hits.is_empty());
}

#[test]
fn the_index_state_decides_how_the_trigram_index_is_used() {
    let status = |state| SearchIndexStatus { state, percent: 50 };
    assert_eq!(
        resolve::substrings(&status(SearchIndexState::Building)),
        Substrings::Filling
    );
    assert_eq!(
        resolve::substrings(&status(SearchIndexState::Ready)),
        Substrings::Complete
    );
    assert_eq!(
        resolve::substrings(&status(SearchIndexState::Rebuilding)),
        Substrings::Complete
    );
}

/// Mail from, to and in copy of Max over both accounts, and some without him.
async fn max_mail(f: &super::tests::Fixture) {
    let store = f.core.store_for_tests();
    let (_, a_inbox, _) = f.accounts[0];
    let (_, b_inbox, _) = f.accounts[1];
    let max = ("Max Mustermann", "max.mustermann@firma.example");
    store
        .store_headers(
            a_inbox,
            super::tests::UV,
            vec![
                header(1, D, "Von Max", max, "alice@example.org"),
                header(
                    2,
                    D + 60,
                    "Von Jan",
                    ("Jan", "jan@firma.example"),
                    "alice@example.org",
                ),
                addressed(
                    header(3, D + 120, "An Max", ("Alice", "alice@example.org"), ""),
                    &[("Max", "Max.Mustermann@firma.example")],
                    &[],
                ),
                header(
                    4,
                    D + 180,
                    "Von Max privat",
                    ("Max", "max@privat.example"),
                    "alice@example.org",
                ),
            ],
            None,
        )
        .await
        .expect("alice");
    store
        .store_headers(
            b_inbox,
            super::tests::UV,
            vec![
                addressed(
                    header(
                        1,
                        D + 240,
                        "Rundschreiben",
                        ("Team", "team@firma.example"),
                        "",
                    ),
                    &[("", "bob@firma.example")],
                    &[max],
                ),
                header(
                    2,
                    D + 300,
                    "Max erwähnt",
                    ("Eve", "eve@firma.example"),
                    "bob@firma.example",
                ),
            ],
            None,
        )
        .await
        .expect("bob");
    // The contact directory reads the new mail.
    let own = crate::compose::contacts::own_addresses(store)
        .await
        .expect("own");
    while crate::compose::contacts::run_batch(store, &own)
        .await
        .expect("contacts")
        .0
        > 0
    {}
}

#[tokio::test]
async fn a_contact_finds_mail_from_to_and_in_copy_over_all_accounts() {
    let f = fixture().await;
    max_mail(&f).await;
    let page = f
        .core
        .search(request(
            "contact:max.mustermann@firma.example",
            SearchScope::All,
        ))
        .await
        .expect("search");
    assert_eq!(subjects(&page), ["Rundschreiben", "An Max", "Von Max"]);
    assert_eq!(page.terms[0].kind, SearchTermKind::Contact);
    assert!(page.problems.is_empty());
    // German key, other case; excluded; combined with text.
    let page = f
        .core
        .search(request(
            "kontakt:MAX.Mustermann@Firma.Example",
            SearchScope::All,
        ))
        .await
        .expect("search");
    assert_eq!(page.hits.len(), 3);
    let page = f
        .core
        .search(request(
            "-contact:max.mustermann@firma.example firma",
            SearchScope::All,
        ))
        .await
        .expect("search");
    assert_eq!(subjects(&page), ["Max erwähnt", "Von Jan"]);
    let page = f
        .core
        .search(request(
            "contact:max.mustermann@firma.example rundschr",
            SearchScope::All,
        ))
        .await
        .expect("search");
    assert_eq!(subjects(&page), ["Rundschreiben"]);
    // An address that is no contact is searched as itself; a value that is no address as words
    // in sender, recipients and copy.
    let page = f
        .core
        .search(request("contact:jan@firma.example", SearchScope::All))
        .await
        .expect("search");
    assert_eq!(subjects(&page), ["Von Jan"]);
    let page = f
        .core
        .search(request("contact:\"Max Mustermann\"", SearchScope::All))
        .await
        .expect("search");
    assert_eq!(subjects(&page), ["Rundschreiben", "An Max", "Von Max"]);

    // Merged: every address of the contact, also in a saved search made before the merge.
    let saved = f
        .core
        .save_search("Max".into(), "contact:max.mustermann@firma.example".into())
        .await
        .expect("save");
    f.core
        .merge_contacts(ContactMergeRequest {
            keep: "max.mustermann@firma.example".into(),
            merge: vec!["max@privat.example".into()],
            confirmed: true,
        })
        .await
        .expect("merge");
    let saved = f
        .core
        .list_saved_searches()
        .await
        .expect("saved")
        .into_iter()
        .find(|s| s.id == saved.id)
        .expect("saved search");
    let page = f
        .core
        .search(request(&saved.query, SearchScope::All))
        .await
        .expect("search");
    assert_eq!(
        subjects(&page),
        ["Rundschreiben", "Von Max privat", "An Max", "Von Max"]
    );
    // Either address names the contact.
    let page = f
        .core
        .search(request("contact:max@privat.example", SearchScope::All))
        .await
        .expect("search");
    assert_eq!(page.hits.len(), 4);

    // The quick filter of a list view resolves it the same way.
    let (_, a_inbox, _) = f.accounts[0];
    let view = f
        .core
        .list_view_messages(
            ListViewRequest {
                scope: InboxScope::Mailbox {
                    mailbox_id: a_inbox,
                },
                order: ListOrder::default(),
                filter: QuickFilter {
                    text: "contact:max@privat.example".into(),
                    ..QuickFilter::default()
                },
            },
            None,
            50,
        )
        .await
        .expect("view");
    let view_subjects: Vec<&str> = view.items.iter().map(|i| i.subject.as_str()).collect();
    assert_eq!(view_subjects, ["Von Max privat", "An Max", "Von Max"]);
}

#[tokio::test]
async fn contact_suggestions_match_names_addresses_and_domains_by_prefix() {
    let f = fixture().await;
    max_mail(&f).await;
    let names =
        |page: mail_model::ContactPage| page.items.into_iter().map(|c| c.email).collect::<Vec<_>>();
    for query in [
        "must",
        "max must",
        "max.mustermann@fi",
        "firma.exa",
        "MUSTERMANN",
    ] {
        let found = names(
            f.core
                .list_contacts(Some(query.into()), None, 3, ContactFilter::All)
                .await
                .expect("contacts"),
        );
        assert!(
            found.contains(&"max.mustermann@firma.example".to_owned()),
            "{query}: {found:?}"
        );
        assert!(found.len() <= 3);
    }
    let found = names(
        f.core
            .list_contacts(Some("privat".into()), None, 3, ContactFilter::All)
            .await
            .expect("contacts"),
    );
    assert_eq!(found, ["max@privat.example"]);
}
