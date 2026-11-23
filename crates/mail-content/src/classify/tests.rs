//! Table tests of the category rules: realistic header sets (international and German services),
//! local signals, the user's corrections and important senders.

use mail_model::{
    CategoryCorrection, CategoryReason, MessageCategory, MessageCategory::*, SenderKey,
};

use super::*;

const ALICE: &str = "alice@vicemail.test";

/// A header block from lines (`\r\n` added).
fn headers(lines: &[&str]) -> ClassifyHeaders {
    let mut raw = lines.join("\r\n");
    raw.push_str("\r\nTo: Alice Archer <alice@vicemail.test>\r\n\r\nbody\r\n");
    ClassifyHeaders::from_raw(raw.as_bytes())
}

fn own() -> Vec<String> {
    vec![ALICE.to_owned(), "a.archer@work.example".to_owned()]
}

/// The reason's kind as the UI sees it (`"knownSystem"`, …): the variant name in camelCase.
fn kind(reason: &CategoryReason) -> String {
    let debug = format!("{reason:?}");
    let name: String = debug.chars().take_while(|c| c.is_alphanumeric()).collect();
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

struct Case {
    name: &'static str,
    lines: &'static [&'static str],
    sent_to_sender: u32,
    replies_to_own: bool,
    category: MessageCategory,
    reason: &'static str,
}

const fn case(
    name: &'static str,
    lines: &'static [&'static str],
    category: MessageCategory,
    reason: &'static str,
) -> Case {
    Case {
        name,
        lines,
        sent_to_sender: 0,
        replies_to_own: false,
        category,
        reason,
    }
}

const CASES: &[Case] = &[
    // --- People ---------------------------------------------------------------------------
    case(
        "plain person",
        &[
            "From: Carol Clark <carol@example.org>",
            "Subject: Treffen morgen?",
            "Message-ID: <1@example.org>",
        ],
        Person,
        "noBulkSignals",
    ),
    case(
        "Outlook person",
        &[
            "From: \"Müller, Hans\" <hans.mueller@firma.example>",
            "Subject: AW: Angebot",
            "X-Mailer: Microsoft Outlook 16.0",
            "X-MS-Has-Attach: yes",
            "Thread-Index: AQHZ",
        ],
        Person,
        "noBulkSignals",
    ),
    case(
        "Apple Mail person, umlaut address",
        &[
            "From: Jürgen Groß <jürgen@firma.example>",
            "Subject: Größenänderung",
            "X-Mailer: Apple Mail (2.3774)",
        ],
        Person,
        "noBulkSignals",
    ),
    case(
        "calendar invitation from a person through Google",
        &[
            "From: Carol Clark <carol@gmail.com>",
            "Sender: Google Calendar <calendar-notification@google.com>",
            "Reply-To: carol@gmail.com",
            "Subject: Invitation: Kick-off @ Wed 10 Jun 2026",
            "Content-Type: multipart/mixed; boundary=x",
        ],
        Person,
        "noBulkSignals",
    ),
    case(
        "small business info address",
        &[
            "From: Tischlerei Holz <info@tischlerei.example>",
            "Subject: Ihr Angebot für die Küche",
        ],
        Person,
        "noBulkSignals",
    ),
    case(
        "Outlook mail with X-Auto-Response-Suppress only",
        &[
            "From: Dave Dawson <dave@example.net>",
            "Subject: Quarterly numbers",
            "X-Auto-Response-Suppress: All",
        ],
        Person,
        "noBulkSignals",
    ),
    case(
        "own message",
        &[
            "From: Alice Archer <alice@vicemail.test>",
            "Subject: Notiz an mich",
        ],
        Person,
        "ownMessage",
    ),
    case(
        "own message, tagged and upper case",
        &["From: ALICE+notes@vicemail.test", "Subject: Einkaufsliste"],
        Person,
        "ownMessage",
    ),
    case(
        "own list post comes back through the list",
        &[
            "From: Alice Archer <a.archer@work.example>",
            "List-Id: Rust users <rust-users.lists.example>",
            "List-Post: <mailto:rust-users@lists.example>",
            "Subject: [rust-users] My question",
        ],
        Person,
        "ownMessage",
    ),
    Case {
        name: "answer to the user's list post",
        lines: &[
            "From: Jane Doe <jane@example.com>",
            "List-Id: Rust users <rust-users.lists.example>",
            "List-Post: <mailto:rust-users@lists.example>",
            "Precedence: list",
            "Subject: Re: [rust-users] My question",
        ],
        sent_to_sender: 0,
        replies_to_own: true,
        category: Person,
        reason: "replyToOwn",
    },
    Case {
        name: "correspondent writes through a CRM with List-Unsubscribe",
        lines: &[
            "From: Erika Mustermann <erika@vertrieb.example>",
            "Subject: Unser Gespräch",
            "List-Unsubscribe: <https://crm.example/u/1>",
            "Feedback-ID: 123:crm",
        ],
        sent_to_sender: 3,
        replies_to_own: false,
        category: Person,
        reason: "correspondent",
    },
    Case {
        name: "support answer from a role address the user wrote to",
        lines: &[
            "From: Kundenservice <rechnung@stadtwerke.example>",
            "Subject: Re: Frage zur Abrechnung",
            "X-SES-Outgoing: 2026.11.23-1.2.3.4",
        ],
        sent_to_sender: 1,
        replies_to_own: false,
        category: Person,
        reason: "correspondent",
    },
    // --- Notifications --------------------------------------------------------------------
    case(
        "GitHub notification",
        &[
            "From: Jane Doe <notifications@github.com>",
            "Reply-To: owner/repo <reply+ABC@reply.github.com>",
            "Subject: Re: [owner/repo] Fix the parser (PR #42)",
            "List-ID: owner/repo <repo.owner.github.com>",
            "List-Unsubscribe: <mailto:unsub+ABC@reply.github.com>",
            "List-Archive: https://github.com/owner/repo",
            "Precedence: list",
            "X-GitHub-Reason: mention",
            "X-GitHub-Sender: janedoe",
        ],
        Notification,
        "knownSystem",
    ),
    case(
        "GitHub account mail",
        &[
            "From: GitHub <noreply@github.com>",
            "Subject: [GitHub] A new SSH key was added to your account",
        ],
        Notification,
        "noReplyAddress",
    ),
    case(
        "GitLab pipeline",
        &[
            "From: GitLab <gitlab@code.example>",
            "Subject: Failed pipeline for main",
            "X-GitLab-Project: tools",
            "X-GitLab-Pipeline-Id: 77",
            "List-Id: tools <123.code.example>",
        ],
        Notification,
        "knownSystem",
    ),
    case(
        "Jira issue",
        &[
            "From: Jane Doe (Jira) <jira@acme.atlassian.net>",
            "Subject: [JIRA] (PROJ-12) Login broken",
            "X-JIRA-FingerPrint: 5f3c",
        ],
        Notification,
        "knownSystem",
    ),
    case(
        "Gitea",
        &[
            "From: Gitea <gitea@git.home.example>",
            "Subject: [othervice/vice-mail] New issue",
            "X-Gitea-Reason: mention",
        ],
        Notification,
        "knownSystem",
    ),
    case(
        "Jenkins",
        &[
            "From: jenkins@ci.home.example",
            "Subject: Build failed in Jenkins: app #12",
            "X-Jenkins-Job: app",
        ],
        Notification,
        "knownSystem",
    ),
    case(
        "Bugzilla",
        &[
            "From: bugzilla-daemon@bugs.example",
            "Subject: [Bug 1234] Crash on start",
            "X-Bugzilla-Reason: AssignedTo",
            "X-Bugzilla-Type: changed",
        ],
        Notification,
        "knownSystem",
    ),
    case(
        "CI with Auto-Submitted and Precedence bulk",
        &[
            "From: CI Example <notifications@ci.example>",
            "Subject: [ci] Build #4711 failed on main",
            "Auto-Submitted: auto-generated",
            "Precedence: bulk",
        ],
        Notification,
        "autoSubmitted",
    ),
    case(
        "Exchange out of office",
        &[
            "From: Jürgen Groß <juergen.gross@firma.example>",
            "Subject: Automatische Antwort: Projekt Phönix",
            "Auto-Submitted: auto-replied",
            "X-Auto-Response-Suppress: All",
        ],
        Notification,
        "autoSubmitted",
    ),
    case(
        "old out of office without Auto-Submitted",
        &[
            "From: Sabine Schäfer <s.schaefer@behoerde.example>",
            "Subject: Abwesenheitsnotiz: Ihre Anfrage",
        ],
        Notification,
        "autoReply",
    ),
    case(
        "English out of office",
        &[
            "From: Priya Sharma <priya@example.net>",
            "Subject: Out of Office: Re: Budget",
        ],
        Notification,
        "autoReply",
    ),
    case(
        "vacation program",
        &[
            "From: Thomas Weiß <thomas@verein.example>",
            "Subject: Re: Mitgliederversammlung",
            "X-Autoreply: yes",
        ],
        Notification,
        "autoReply",
    ),
    case(
        "Precedence auto_reply",
        &[
            "From: Zoe <zoe@uni.example>",
            "Subject: Re: Paper",
            "Precedence: auto_reply",
        ],
        Notification,
        "autoReply",
    ),
    case(
        "delivery status notification",
        &[
            "From: Mail Delivery System <MAILER-DAEMON@mx.example.net>",
            "Subject: Undelivered Mail Returned to Sender",
            "Content-Type: multipart/report; report-type=delivery-status; boundary=b",
        ],
        Notification,
        "report",
    ),
    case(
        "read receipt",
        &[
            "From: Carol Clark <carol@example.org>",
            "Subject: Gelesen: Vertrag",
            "Content-Type: multipart/report; report-type=disposition-notification; boundary=b",
        ],
        Notification,
        "report",
    ),
    case(
        "null return path",
        &[
            "Return-Path: <>",
            "From: Server <root@server.home.example>",
            "Subject: Cron <root@server> backup",
        ],
        Notification,
        "report",
    ),
    case(
        "Paketdienst",
        &[
            "From: Paketdienst <noreply@paket.example>",
            "Subject: Ihre Sendung kommt heute zwischen 10:00 und 13:00 Uhr",
        ],
        Notification,
        "noReplyAddress",
    ),
    case(
        "Bahn booking",
        &[
            "From: Bahn Fernverkehr <buchungsbestaetigung@bahn.example>",
            "Subject: Ihre Fahrkarte für die Reise am 24.11.2026",
            "Feedback-ID: booking:bahn",
        ],
        Notification,
        "automatedAddress",
    ),
    case(
        "shop shipping confirmation through Amazon SES",
        &[
            "Return-Path: <0102018f@eu-west-1.amazonses.com>",
            "From: Shop.de <versandbestaetigung@shop.example>",
            "Subject: Ihre Bestellung wurde versandt",
            "Feedback-ID: 1.eu-west-1.abc:AmazonSES",
            "X-SES-Outgoing: 2026.11.23-54.240.1.1",
        ],
        Notification,
        "automatedAddress",
    ),
    case(
        "shop order confirmation",
        &[
            "From: Shop <bestellung@shop.example>",
            "Subject: Bestellbestätigung Nr. 20261123",
        ],
        Notification,
        "automatedAddress",
    ),
    case(
        "Finanzamt",
        &[
            "From: ELSTER <nicht-antworten@elster.example>",
            "Subject: Neue Nachricht in Ihrem Postfach",
        ],
        Notification,
        "noReplyAddress",
    ),
    case(
        "Krankenkasse",
        &[
            "From: Meine Kasse <keine-antwort@kasse.example>",
            "Subject: Ihr Dokument ist bereit",
        ],
        Notification,
        "noReplyAddress",
    ),
    case(
        "Stadtwerke invoice",
        &[
            "From: Stadtwerke <rechnung@stadtwerke.example>",
            "Subject: Ihre Rechnung für Oktober",
        ],
        Notification,
        "automatedAddress",
    ),
    case(
        "bank message",
        &[
            "From: Sparkasse <benachrichtigung@sparkasse.example>",
            "Subject: Neue Nachricht im elektronischen Postfach",
        ],
        Notification,
        "automatedAddress",
    ),
    case(
        "calendar reminder",
        &[
            "From: Google Calendar <calendar-notification@google.com>",
            "Subject: Notification: Zahnarzt @ Mon 30 Nov 2026",
        ],
        Notification,
        "automatedAddress",
    ),
    case(
        "Google security alert",
        &[
            "From: Google <no-reply@accounts.google.com>",
            "Subject: Security alert",
        ],
        Notification,
        "noReplyAddress",
    ),
    case(
        "password reset through SendGrid",
        &[
            "From: App <hello@app.example>",
            "Subject: Reset your password",
            "X-SG-EID: abc",
        ],
        Notification,
        "mailingService",
    ),
    case(
        "VERP bounce address",
        &[
            "Return-Path: <bounces+123-alice=vicemail.test@em.app.example>",
            "From: App <team@app.example>",
            "Subject: Your invoice is ready",
        ],
        Notification,
        "mailingService",
    ),
    case(
        "Postmark",
        &[
            "From: Tool <support@tool.example>",
            "Subject: Welcome",
            "X-PM-Message-Id: 6a1f",
        ],
        Notification,
        "mailingService",
    ),
    case(
        "Facebook",
        &[
            "From: Facebook <notification@facebookmail.com>",
            "Subject: Jane commented on your post",
            "List-Unsubscribe: <https://www.facebook.com/o.php?u=1>",
            "X-Facebook-Notify: feed_comment",
        ],
        Notification,
        "knownSystem",
    ),
    // --- Newsletters ----------------------------------------------------------------------
    case(
        "newsletter with list headers",
        &[
            "From: Example News <news@news.example>",
            "Subject: Example News: issue #1",
            "List-Id: Example News <weekly.news.example>",
            "List-Unsubscribe: <mailto:unsubscribe@news.example?subject=unsubscribe>",
            "List-Unsubscribe-Post: List-Unsubscribe=One-Click",
            "Precedence: bulk",
        ],
        Newsletter,
        "mailingList",
    ),
    case(
        "discussion list",
        &[
            "From: Jane Doe <jane@example.com>",
            "Subject: [rust-users] Async traits are stable now",
            "List-Id: Rust users discussion <rust-users.lists.example>",
            "List-Post: <mailto:rust-users@lists.example>",
            "Precedence: list",
        ],
        Newsletter,
        "discussionList",
    ),
    case(
        "announcement list (List-Post: NO)",
        &[
            "From: Projekt <announce@projekt.example>",
            "Subject: Release 2.0",
            "List-Id: <announce.projekt.example>",
            "List-Post: NO",
        ],
        Newsletter,
        "mailingList",
    ),
    case(
        "Precedence junk",
        &[
            "From: Deals <deals@shop.example>",
            "Subject: 50% off everything!!!",
            "Precedence: junk",
        ],
        Newsletter,
        "precedence",
    ),
    case(
        "Mailchimp",
        &[
            "Return-Path: <bounce-mc.us5_1.2-alice=vicemail.test@mail193.atl121.mcsv.net>",
            "From: Verein <info@verein.example>",
            "Subject: Vereinsinfo November",
            "List-Unsubscribe: <https://verein.us5.list-manage.com/unsubscribe>",
            "X-MC-User: 1a2b",
            "X-campaignid: mailchimp1a2b.3c",
        ],
        Newsletter,
        "campaign",
    ),
    case(
        "German ESP with CSA header",
        &[
            "From: Zeitung <newsletter@zeitung.example>",
            "Subject: Ihr Morgen-Briefing",
            "List-Unsubscribe: <https://esp.example/u/1>",
            "X-CSA-Complaints: csa-complaints@eco.de",
            "Feedback-ID: 9:1:esp",
        ],
        Newsletter,
        "campaign",
    ),
    case(
        "newsletter software in X-Mailer",
        &[
            "From: Laden <info@laden.example>",
            "Subject: Neue Herbstkollektion",
            "X-Mailer: rapidmail",
        ],
        Newsletter,
        "campaign",
    ),
    case(
        "campaign header",
        &[
            "From: Reisen <service@reisen.example>",
            "Subject: Last-Minute-Angebote",
            "X-Newsletter-Id: 2026-47",
        ],
        Newsletter,
        "campaign",
    ),
    case(
        "HubSpot bounce domain",
        &[
            "Return-Path: <1axbkdz@bounce.hubspotemail.net>",
            "From: SaaS Team <team@saas.example>",
            "Subject: What's new in November",
        ],
        Newsletter,
        "campaign",
    ),
    case(
        "Campaign Monitor numbered domain",
        &[
            "Return-Path: <bounce@cmail19.com>",
            "From: Museum <hello@museum.example>",
            "Subject: Ausstellung",
        ],
        Newsletter,
        "campaign",
    ),
    case(
        "only List-Unsubscribe",
        &[
            "From: Autorin <autorin@blog.example>",
            "Subject: Neuer Beitrag",
            "List-Unsubscribe: <https://blog.example/unsubscribe>",
        ],
        Newsletter,
        "unsubscribe",
    ),
    case(
        "no-reply sender with List-Unsubscribe",
        &[
            "From: Video <noreply@video.example>",
            "Subject: New upload from a channel you follow",
            "List-Unsubscribe: <https://video.example/u>",
        ],
        Newsletter,
        "unsubscribe",
    ),
    case(
        "newsletter address without headers",
        &[
            "From: Shop <newsletter@shop.example>",
            "Subject: Black Week",
        ],
        Newsletter,
        "newsletterAddress",
    ),
    case(
        "offers address",
        &[
            "From: Möbelhaus <angebote@moebel.example>",
            "Subject: Nur heute: 20 %",
        ],
        Newsletter,
        "newsletterAddress",
    ),
    case(
        "newsletter subdomain",
        &[
            "From: Verlag <info@newsletter.verlag.example>",
            "Subject: Leseprobe",
        ],
        Newsletter,
        "newsletterAddress",
    ),
    case(
        "digest",
        &[
            "From: Rust Users <rust-users@lists.example>",
            "Subject: Mailing list digest",
            "List-Id: Rust users discussion <rust-users.lists.example>",
            "Content-Type: multipart/digest; boundary=d",
        ],
        Newsletter,
        "mailingList",
    ),
    // --- Known limits (decision 0016) -----------------------------------------------------
    case(
        "phishing from a plain address looks like a person",
        &[
            "From: Vice Bank <service@bank.example>",
            "Subject: Your account will be suspended – verify now",
        ],
        Person,
        "noBulkSignals",
    ),
    Case {
        name: "a club newsletter with only List-Unsubscribe from someone the user wrote to",
        lines: &[
            "From: Verein <info@verein.example>",
            "Subject: Einladung",
            "List-Unsubscribe: <https://verein.example/abmelden>",
        ],
        sent_to_sender: 1,
        replies_to_own: false,
        category: Person,
        reason: "correspondent",
    },
];

#[test]
fn table_of_realistic_header_sets() {
    let own = own();
    let rules = SenderRules::default();
    let mut misses = Vec::new();
    for case in CASES {
        let h = headers(case.lines);
        let local = LocalSignals {
            own_addresses: &own,
            sent_to_sender: case.sent_to_sender,
            replies_to_own: case.replies_to_own,
        };
        let decision = classify(&h, &local, &rules);
        if decision.category != case.category || kind(&decision.reason) != case.reason {
            misses.push(format!(
                "{}: {:?} / {:?}, expected {:?} / {}",
                case.name, decision.category, decision.reason, case.category, case.reason
            ));
        }
        assert_eq!(decision.important, None);
    }
    assert!(misses.is_empty(), "{}", misses.join("\n"));
    assert!(CASES.len() >= 60, "{} cases", CASES.len());
}

#[test]
fn reasons_carry_the_evidence() {
    let own = own();
    let local = LocalSignals {
        own_addresses: &own,
        ..LocalSignals::default()
    };
    let decide = |lines: &[&str]| classify_automatic(&headers(lines), &local).reason;
    assert_eq!(
        decide(&["From: a@b.example", "Auto-Submitted: auto-generated"]),
        CategoryReason::AutoSubmitted {
            value: "auto-generated".into()
        }
    );
    assert_eq!(
        decide(&["From: a@b.example", "Auto-Submitted: no", "Subject: Hallo"]),
        CategoryReason::NoBulkSignals,
        "Auto-Submitted: no is a person"
    );
    assert_eq!(
        decide(&["From: x@github.com", "X-GitHub-Reason: review_requested"]),
        CategoryReason::KnownSystem {
            system: "GitHub".into(),
            header: "X-GitHub-Reason".into()
        }
    );
    assert_eq!(
        decide(&["From: a@b.example", "List-Id: Liste <Liste.B.Example>"]),
        CategoryReason::MailingList {
            list_id: "liste.b.example".into()
        }
    );
    assert_eq!(
        decide(&["From: a@b.example", "Precedence: BULK"]),
        CategoryReason::Precedence {
            value: "bulk".into()
        }
    );
    assert_eq!(
        decide(&["From: Paket <NoReply@paket.example>"]),
        CategoryReason::NoReplyAddress {
            address: "NoReply@paket.example".into()
        }
    );
    assert_eq!(
        decide(&[
            "From: a@b.example",
            "X-Mailer: CleverReach Newsletter System"
        ]),
        CategoryReason::Campaign {
            evidence: "X-Mailer: CleverReach Newsletter System".into()
        }
    );
    let long = format!("Subject: Automatic reply: {}", "x".repeat(200));
    let CategoryReason::AutoReply { evidence } = decide(&["From: a@b.example", long.as_str()])
    else {
        panic!("auto reply expected");
    };
    assert_eq!(evidence.chars().count(), 64, "values are cut for the UI");
}

#[test]
fn messages_without_sender_or_headers() {
    let own = own();
    let local = LocalSignals {
        own_addresses: &own,
        ..LocalSignals::default()
    };
    let empty = classify_automatic(&ClassifyHeaders::default(), &local);
    assert_eq!(
        (empty.category, empty.reason),
        (Person, CategoryReason::NoBulkSignals)
    );
    let garbage = classify_automatic(&ClassifyHeaders::from_raw(b"\x00\xff garbage"), &local);
    assert_eq!(garbage.category, Person);
    // A no-reply sender never counts as a correspondent.
    let decision = classify_automatic(
        &headers(&["From: noreply@shop.example"]),
        &LocalSignals {
            own_addresses: &own,
            sent_to_sender: 5,
            replies_to_own: false,
        },
    );
    assert_eq!(decision.category, Notification);
}

fn correction(key: SenderKey, category: MessageCategory) -> CategoryCorrection {
    CategoryCorrection { key, category }
}

#[test]
fn user_corrections_come_first() {
    let own = own();
    let local = LocalSignals {
        own_addresses: &own,
        ..LocalSignals::default()
    };
    let ci = headers(&[
        "From: CI <Notifications@CI.example>",
        "Auto-Submitted: auto-generated",
        "Precedence: bulk",
    ]);
    let list = headers(&[
        "From: Jane Doe <jane@example.com>",
        "List-Id: Rust users <rust-users.lists.example>",
        "List-Post: <mailto:rust-users@lists.example>",
    ]);
    let sub = headers(&["From: Shop <newsletter@news.shop.example>"]);

    // Address: beats Auto-Submitted; spelling and case do not matter.
    let rules = SenderRules::new(
        &[correction(
            SenderKey::address(" <notifications@ci.EXAMPLE> "),
            Person,
        )],
        &[],
    );
    let decision = classify(&ci, &local, &rules);
    assert_eq!(decision.category, Person);
    assert_eq!(
        decision.reason,
        CategoryReason::UserRule {
            key: SenderKey::address(" <notifications@ci.EXAMPLE> ")
        }
    );

    // List-Id.
    let rules = SenderRules::new(
        &[correction(
            SenderKey::list_id("Rust-Users.Lists.Example"),
            Person,
        )],
        &[],
    );
    assert_eq!(classify(&list, &local, &rules).category, Person);

    // Domain, also for subdomains; a dot or `@` in front is fine.
    let rules = SenderRules::new(
        &[correction(SenderKey::domain("@shop.example"), Notification)],
        &[],
    );
    assert_eq!(classify(&sub, &local, &rules).category, Notification);

    // Address beats list beats domain; the longest domain wins.
    let rules = SenderRules::new(
        &[
            correction(SenderKey::domain("example.com"), Notification),
            correction(SenderKey::list_id("rust-users.lists.example"), Newsletter),
            correction(SenderKey::address("jane@example.com"), Person),
        ],
        &[],
    );
    assert_eq!(classify(&list, &local, &rules).category, Person);
    let rules = SenderRules::new(
        &[
            correction(SenderKey::domain("example.com"), Notification),
            correction(SenderKey::list_id("rust-users.lists.example"), Person),
        ],
        &[],
    );
    assert_eq!(classify(&list, &local, &rules).category, Person);
    let rules = SenderRules::new(
        &[
            correction(SenderKey::domain("shop.example"), Notification),
            correction(SenderKey::domain("news.shop.example"), Person),
        ],
        &[],
    );
    assert_eq!(classify(&sub, &local, &rules).category, Person);
    // A domain does not match a longer name that merely ends the same way.
    let rules = SenderRules::new(&[correction(SenderKey::domain("op.example"), Person)], &[]);
    assert_eq!(classify(&sub, &local, &rules).category, Newsletter);

    // IDNA: the Unicode and the ASCII spelling are one domain.
    let umlaut = headers(&["From: Info <newsletter@xn--mller-kva.example>"]);
    let rules = SenderRules::new(
        &[correction(SenderKey::domain("müller.example"), Person)],
        &[],
    );
    assert_eq!(classify(&umlaut, &local, &rules).category, Person);

    // Later corrections of the same key replace earlier ones; unusable keys are ignored.
    let rules = SenderRules::new(
        &[
            correction(SenderKey::address("jane@example.com"), Notification),
            correction(SenderKey::address("JANE@example.com"), Person),
            correction(SenderKey::address("not an address"), Notification),
            correction(SenderKey::list_id("  "), Notification),
        ],
        &[],
    );
    assert_eq!(classify(&list, &local, &rules).category, Person);
}

#[test]
fn important_senders_form_the_priority_block() {
    let own = own();
    let local = LocalSignals {
        own_addresses: &own,
        ..LocalSignals::default()
    };
    let boss = headers(&["From: Chefin <chefin@firma.example>", "Subject: Kurz"]);
    let colleague = headers(&["From: Kollege <kollege@team.firma.example>"]);
    let newsletter = headers(&[
        "From: Chefin <chefin@firma.example>",
        "List-Id: <intern.firma.example>",
    ]);
    let other = headers(&["From: carol@example.org"]);
    let rules = SenderRules::new(&[], &[SenderKey::address("Chefin@Firma.example")]);
    assert_eq!(
        classify(&boss, &local, &rules).important,
        Some(SenderKey::address("Chefin@Firma.example"))
    );
    // Orthogonal to the category: an important sender's list mail stays a newsletter.
    let decision = classify(&newsletter, &local, &rules);
    assert_eq!(decision.category, Newsletter);
    assert!(decision.important.is_some());
    assert_eq!(classify(&other, &local, &rules).important, None);
    // A domain covers its subdomains.
    let rules = SenderRules::new(&[], &[SenderKey::domain("firma.example")]);
    assert!(classify(&colleague, &local, &rules).important.is_some());
    // A correction and an important mark together.
    let rules = SenderRules::new(
        &[correction(
            SenderKey::address("chefin@firma.example"),
            Person,
        )],
        &[SenderKey::domain("firma.example")],
    );
    let decision = classify(&newsletter, &local, &rules);
    assert_eq!(decision.category, Person);
    assert_eq!(decision.important, Some(SenderKey::domain("firma.example")));
}

#[test]
fn stored_decisions_get_the_rules_applied_later() {
    let own = own();
    let local = LocalSignals {
        own_addresses: &own,
        ..LocalSignals::default()
    };
    let h = headers(&["From: Shop <newsletter@shop.example>"]);
    let stored = classify_automatic(&h, &local);
    assert_eq!(stored.category, Newsletter);
    let sender = SenderIdentity::new(Some("Newsletter@Shop.example"), None);
    let rules = SenderRules::new(
        &[correction(SenderKey::domain("shop.example"), Person)],
        &[],
    );
    let applied = apply_user_rules(stored.clone(), &sender, &rules);
    assert_eq!(applied.category, Person);
    // Removing the rule restores the stored decision.
    assert_eq!(
        apply_user_rules(stored.clone(), &sender, &SenderRules::default()),
        stored
    );
    assert_eq!(
        SenderIdentity::new(Some("x@Y.example"), Some("Liste <A.B>")),
        SenderIdentity {
            address: Some("x@y.example".into()),
            domain: Some("y.example".into()),
            list_id: Some("a.b".into()),
        }
    );
    assert_eq!(
        normalize_key(&SenderKey::address("Jürgen@Müller.example")).as_deref(),
        Some("jürgen@xn--mller-kva.example")
    );
}
