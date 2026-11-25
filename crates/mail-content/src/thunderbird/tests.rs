//! Tests of the Thunderbird readers: the fixture profile in `testmail/thunderbird/`, `prefs.js`
//! as Thunderbird 128 writes it, escapes and broken input, and `profiles.ini` variants.

use std::time::Instant;

use super::*;

const PROFILES_INI: &str = include_str!("../../../../testmail/thunderbird/profiles.ini");
const PREFS_JS: &str =
    include_str!("../../../../testmail/thunderbird/Profiles/t3stpr0f.default-release/prefs.js");
const OLD_PREFS_JS: &str =
    include_str!("../../../../testmail/thunderbird/Profiles/0ldpr0f.default/prefs.js");

fn s(value: &str) -> PrefValue {
    PrefValue::Str(value.to_owned())
}

#[test]
fn the_fixture_profiles_name_the_installed_default() {
    let profiles = parse_profiles_ini(PROFILES_INI);
    assert_eq!(
        profiles,
        vec![
            ThunderbirdProfileEntry {
                name: "old".into(),
                path: "Profiles/0ldpr0f.default".into(),
                is_relative: true,
                // `Default=1`, but the `[Install…]` section names the other one.
                is_default: false,
            },
            ThunderbirdProfileEntry {
                name: "default-release".into(),
                path: "Profiles/t3stpr0f.default-release".into(),
                is_relative: true,
                is_default: true,
            },
        ]
    );
}

#[test]
fn the_fixture_accounts_come_with_servers_identities_signatures_and_smtp() {
    let prefs = parse_prefs_js(PREFS_JS);
    assert_eq!(
        prefs.get("mail.server.server1.port"),
        Some(&PrefValue::Int(1993))
    );
    assert_eq!(
        prefs.get("mail.identity.id1.htmlSigFormat"),
        Some(&PrefValue::Bool(true))
    );
    let accounts = thunderbird_accounts(&prefs);
    assert_eq!(
        accounts.iter().map(|a| a.key.as_str()).collect::<Vec<_>>(),
        ["account1", "account2", "account3"]
    );
    let smtp1 = TbSmtp {
        key: "smtp1".into(),
        host: "127.0.0.1".into(),
        port: Some(1465),
        security: TbSecurity::Tls,
        user_name: "alice@vicemail.test".into(),
    };
    let alice = &accounts[0];
    assert_eq!(
        alice.server,
        TbServer {
            key: "server1".into(),
            kind: TbServerKind::Imap,
            host: "127.0.0.1".into(),
            port: Some(1993),
            security: TbSecurity::Tls,
            user_name: "alice@vicemail.test".into(),
            name: "Alice (Thunderbird)".into(),
            directory: Some("[ProfD]ImapMail/127.0.0.1".into()),
        }
    );
    assert_eq!(
        alice.identities,
        vec![
            TbIdentity {
                key: "id1".into(),
                email: "alice@vicemail.test".into(),
                full_name: "Alice Example".into(),
                reply_to: None,
                signature: TbSignature::Html("<b>Alice</b><br>Example Ltd.".into()),
                smtp: Some(smtp1.clone()),
            },
            TbIdentity {
                key: "id2".into(),
                email: "alice.alias@vicemail.test".into(),
                full_name: "Alice Alias".into(),
                reply_to: Some("alice@vicemail.test".into()),
                signature: TbSignature::File {
                    path: "[ProfD]signature.txt".into(),
                    html: false,
                },
                // No `smtpServer` of its own: the default server.
                smtp: Some(smtp1),
            },
        ]
    );
    let bob = &accounts[1];
    assert_eq!(bob.server.kind, TbServerKind::Pop3);
    assert_eq!(bob.server.port, Some(995));
    assert_eq!(bob.identities.len(), 1);
    assert_eq!(bob.identities[0].full_name, "Bob Example");
    assert_eq!(bob.identities[0].signature, TbSignature::None);
    assert_eq!(
        bob.identities[0].smtp,
        Some(TbSmtp {
            key: "smtp2".into(),
            host: "127.0.0.1".into(),
            port: Some(1587),
            security: TbSecurity::StartTls,
            user_name: "bob@vicemail.test".into(),
        })
    );
    let local = &accounts[2];
    assert_eq!(local.server.kind, TbServerKind::Local);
    assert_eq!(local.server.name, "Local Folders");
    assert_eq!(local.server.port, None);
    assert_eq!(local.server.security, TbSecurity::None);
    assert_eq!(
        local.server.directory.as_deref(),
        Some("[ProfD]Mail/Local Folders")
    );
    assert!(local.identities.is_empty());

    // The other profile has no accounts.
    let old = parse_prefs_js(OLD_PREFS_JS);
    assert_eq!(old.len(), 2);
    assert!(thunderbird_accounts(&old).is_empty());
}

#[test]
fn prefs_js_reads_strings_with_escapes_numbers_and_booleans() {
    let text = r#"// Mozilla User Preferences
/* a block
   comment */
# a hash comment
user_pref("a.double", "say \"hi\"\n\ttab \\ back\/slash");
user_pref('a.single', 'it\'s');
user_pref("a.unicode", "caf\u00e9 \uD83D\uDE00 \x41 \uD800x \u12");
pref("a.default", -42);
user_pref("a.plus", +7);
user_pref("a.true", true);
user_pref("a.false",false);
user_pref ( "a.spaced" , "yes" ) ;
user_pref("a.later", 1);
user_pref("a.later", 2);
user_pref("a.overflow", 99999999999999999999);
user_pref("a.empty", "");
user_pref("a.nosemicolon", "end")"#;
    let prefs = parse_prefs_js(text);
    assert_eq!(
        prefs.get("a.double"),
        Some(&s("say \"hi\"\n\ttab \\ back/slash"))
    );
    assert_eq!(prefs.get("a.single"), Some(&s("it's")));
    assert_eq!(
        prefs.get("a.unicode"),
        Some(&s("café 😀 A \u{fffd}x \u{fffd}12"))
    );
    assert_eq!(prefs.get("a.default"), Some(&PrefValue::Int(-42)));
    assert_eq!(prefs.get("a.plus"), Some(&PrefValue::Int(7)));
    assert_eq!(prefs.get("a.true"), Some(&PrefValue::Bool(true)));
    assert_eq!(prefs.get("a.false"), Some(&PrefValue::Bool(false)));
    assert_eq!(prefs.get("a.spaced"), Some(&s("yes")));
    assert_eq!(prefs.get("a.later"), Some(&PrefValue::Int(2)));
    assert_eq!(prefs.get("a.overflow"), None);
    assert_eq!(prefs.get("a.empty"), Some(&s("")));
    assert_eq!(prefs.get("a.nosemicolon"), Some(&s("end")));
    assert_eq!(prefs.len(), 11);
}

#[test]
fn prefs_js_skips_what_it_does_not_understand() {
    let text = "garbage line (\"x\", 1);\n\
        user_pref(\"good.one\", 1);\n\
        user_pref(\"no.value\");\n\
        user_pref(\"bad.value\", undefined);\n\
        user_pref(\"unterminated, 3);\n\
        user_pref(\"good.two\", \"ü\");\n\
        sticky_pref(\"sticky\", 1);\n\
        user_pref(42, 1);\n\
        user_pref(\"good.three\", true);";
    let prefs = parse_prefs_js(text);
    assert_eq!(prefs.get("good.one"), Some(&PrefValue::Int(1)));
    assert_eq!(prefs.get("good.two"), Some(&s("ü")));
    assert_eq!(prefs.get("good.three"), Some(&PrefValue::Bool(true)));
    assert_eq!(prefs.len(), 3, "{prefs:?}");
    assert!(parse_prefs_js("").is_empty());
    assert!(parse_prefs_js("/* never closed").is_empty());
    assert!(parse_prefs_js("user_pref(\"x\", \"never closed").is_empty());
}

#[test]
fn prefs_js_is_bounded_and_linear() {
    // A value over 1 MiB is dropped; the statements around it stay.
    let big = format!(
        "user_pref(\"a\", 1);\nuser_pref(\"big\", \"{}\");\nuser_pref(\"b\", 2);\n",
        "x".repeat(MAX_PREF_VALUE_BYTES + 1)
    );
    let prefs = parse_prefs_js(&big);
    assert_eq!(prefs.get("big"), None);
    assert_eq!(prefs.get("a"), Some(&PrefValue::Int(1)));
    assert_eq!(prefs.get("b"), Some(&PrefValue::Int(2)));

    let start = Instant::now();
    let mut many = String::new();
    for n in 0..(MAX_PREFS + 10) {
        many.push_str(&format!("user_pref(\"k{n}\", {n});\n"));
    }
    let prefs = parse_prefs_js(&many);
    assert_eq!(prefs.len(), MAX_PREFS);
    // Hostile: many unterminated strings and openings on one long line.
    let hostile = "user_pref(\"".repeat(200_000) + &"(".repeat(1_000_000);
    assert!(parse_prefs_js(&hostile).is_empty());
    let multibyte = "ä".repeat(1_000_000) + "\nuser_pref(\"ok\", 1);";
    assert_eq!(parse_prefs_js(&multibyte).len(), 1);
    assert!(start.elapsed().as_secs() < 20, "{:?}", start.elapsed());
}

#[test]
fn accounts_follow_the_account_list_and_skip_what_is_missing() {
    let text = r#"
user_pref("mail.accountmanager.accounts", " account2 , account1,account1,missing,noserver, ");
user_pref("mail.account.account1.server", "server1");
user_pref("mail.account.account1.identities", "id1,idX,id2,id3");
user_pref("mail.account.account2.server", "server2");
user_pref("mail.account.noserver.server", "serverZ");
user_pref("mail.server.server1.type", "imap");
user_pref("mail.server.server1.hostname", " imap.example.org ");
user_pref("mail.server.server1.port", 0);
user_pref("mail.server.server1.socketType", 2);
user_pref("mail.server.server1.userName", "u");
user_pref("mail.server.server1.directory", "C:\\Users\\x\\AppData\\Roaming\\Thunderbird\\Profiles\\p\\ImapMail\\imap.example.org");
user_pref("mail.server.server2.type", "rss");
user_pref("mail.server.server2.hostname", "Feeds");
user_pref("mail.server.server2.port", 70000);
user_pref("mail.server.server2.socketType", 9);
user_pref("mail.identity.id1.useremail", "one@example.org");
user_pref("mail.identity.id1.htmlSigText", "-- \nOne");
user_pref("mail.identity.id1.smtpServer", "smtpGone");
user_pref("mail.identity.id2.useremail", "  ");
user_pref("mail.identity.id3.useremail", "three@example.org");
user_pref("mail.identity.id3.attach_signature", true);
user_pref("mail.identity.id3.sig_file", "C:\\Sig\\three.HTML");
user_pref("mail.identity.id3.htmlSigText", "ignored when a file is attached");
user_pref("mail.smtpservers", "smtpB,smtpA");
user_pref("mail.smtpserver.smtpA.hostname", "a.example.org");
user_pref("mail.smtpserver.smtpA.try_ssl", 1);
user_pref("mail.smtpserver.smtpB.hostname", "b.example.org");
user_pref("mail.smtpserver.smtpB.port", "2525");
user_pref("mail.smtpserver.smtpB.try_ssl", 0);
"#;
    let accounts = thunderbird_accounts(&parse_prefs_js(text));
    assert_eq!(
        accounts.iter().map(|a| a.key.as_str()).collect::<Vec<_>>(),
        ["account2", "account1"]
    );
    let feeds = &accounts[0];
    assert_eq!(feeds.server.kind, TbServerKind::Other);
    assert_eq!(feeds.server.port, None, "out of range");
    assert_eq!(feeds.server.security, TbSecurity::None);
    assert!(feeds.identities.is_empty());

    let imap = &accounts[1];
    assert_eq!(imap.server.host, "imap.example.org");
    assert_eq!(imap.server.port, None, "0 means unset");
    assert_eq!(imap.server.security, TbSecurity::StartTls);
    assert_eq!(
        imap.server.directory.as_deref(),
        Some(r"C:\Users\x\AppData\Roaming\Thunderbird\Profiles\p\ImapMail\imap.example.org")
    );
    assert_eq!(
        imap.identities
            .iter()
            .map(|i| i.email.as_str())
            .collect::<Vec<_>>(),
        ["one@example.org", "three@example.org"]
    );
    let one = &imap.identities[0];
    assert_eq!(one.signature, TbSignature::Text("-- \nOne".into()));
    // Its own server is gone and there is no default: the first of `mail.smtpservers`.
    assert_eq!(
        one.smtp,
        Some(TbSmtp {
            key: "smtpB".into(),
            host: "b.example.org".into(),
            port: Some(2525),
            security: TbSecurity::None,
            user_name: String::new(),
        })
    );
    let three = &imap.identities[1];
    assert_eq!(
        three.signature,
        TbSignature::File {
            path: r"C:\Sig\three.HTML".into(),
            html: true,
        }
    );
    assert_eq!(three.full_name, "");
    assert_eq!(three.reply_to, None);

    // No SMTP server at all.
    let lone = thunderbird_accounts(&parse_prefs_js(
        r#"user_pref("mail.accountmanager.accounts", "a");
user_pref("mail.account.a.server", "s");
user_pref("mail.account.a.identities", "i");
user_pref("mail.server.s.type", "imap");
user_pref("mail.identity.i.useremail", "i@example.org");
user_pref("mail.identity.i.attach_signature", true);
user_pref("mail.identity.i.htmlSigText", "<p>fallback</p>");
user_pref("mail.identity.i.htmlSigFormat", true);"#,
    ));
    assert_eq!(lone.len(), 1);
    assert_eq!(lone[0].identities[0].smtp, None);
    // `attach_signature` without a file: the inline signature.
    assert_eq!(
        lone[0].identities[0].signature,
        TbSignature::Html("<p>fallback</p>".into())
    );
    assert!(thunderbird_accounts(&BTreeMap::new()).is_empty());
}

#[test]
fn accounts_and_identities_are_bounded() {
    let mut text = String::new();
    let keys: Vec<String> = (0..100).map(|n| format!("a{n}")).collect();
    text.push_str(&format!(
        "user_pref(\"mail.accountmanager.accounts\", \"{}\");\n",
        keys.join(",")
    ));
    let ids: Vec<String> = (0..50).map(|n| format!("i{n}")).collect();
    for key in &keys {
        text.push_str(&format!(
            "user_pref(\"mail.account.{key}.server\", \"s\");\nuser_pref(\"mail.account.{key}.identities\", \"{}\");\n",
            ids.join(",")
        ));
    }
    text.push_str("user_pref(\"mail.server.s.type\", \"imap\");\n");
    for id in &ids {
        text.push_str(&format!(
            "user_pref(\"mail.identity.{id}.useremail\", \"{id}@example.org\");\n"
        ));
    }
    let accounts = thunderbird_accounts(&parse_prefs_js(&text));
    assert_eq!(accounts.len(), MAX_ACCOUNTS);
    assert!(
        accounts
            .iter()
            .all(|a| a.identities.len() == MAX_IDENTITIES)
    );
}

#[test]
fn profiles_ini_variants() {
    // Without an install section, `Default=1` decides; absolute paths, Windows separators, a
    // BOM, CRLF, comments, and sections without a path.
    let text = "\u{feff}[General]\r\nStartWithLastProfile=1\r\n\r\n; comment\r\n\
        [Profile0]\r\nName=work\r\nIsRelative=0\r\nPath=D:\\Mail\\work.profile\r\nDefault=1\r\n\r\n\
        [Profile1]\r\nName = home \r\nIsRelative=1\r\nPath=Profiles/home.default\r\n\r\n\
        [Profile2]\r\nName=broken\r\n\r\n\
        [Profile3]\r\nPath=/Users/x/tb/abs.default\r\n";
    let profiles = parse_profiles_ini(text);
    assert_eq!(
        profiles,
        vec![
            ThunderbirdProfileEntry {
                name: "work".into(),
                path: r"D:\Mail\work.profile".into(),
                is_relative: false,
                is_default: true,
            },
            ThunderbirdProfileEntry {
                name: "home".into(),
                path: "Profiles/home.default".into(),
                is_relative: true,
                is_default: false,
            },
            ThunderbirdProfileEntry {
                name: "abs.default".into(),
                path: "/Users/x/tb/abs.default".into(),
                is_relative: false,
                is_default: false,
            },
        ]
    );
    // An install section that names a path no profile has: `Default=1` stays.
    let text = "[InstallAAAA]\nDefault=Profiles/gone\n[Profile0]\nPath=Profiles/a\nDefault=1\n[Profile1]\nPath=Profiles\\b\n";
    let profiles = parse_profiles_ini(text);
    assert!(profiles[0].is_default);
    assert!(!profiles[1].is_default);
    // The install section matches whatever the separators.
    let text = "[Profile0]\nPath=Profiles/a\nDefault=1\n[Profile1]\nPath=Profiles\\b\n[InstallAAAA]\nDefault=Profiles/b\n";
    let profiles = parse_profiles_ini(text);
    assert!(!profiles[0].is_default);
    assert!(profiles[1].is_default);
    assert!(parse_profiles_ini("").is_empty());
    assert!(parse_profiles_ini("no sections at all\n=\n[").is_empty());
}

#[test]
fn profiles_ini_is_bounded() {
    let mut text = String::new();
    for n in 0..200 {
        text.push_str(&format!("[Profile{n}]\nPath=Profiles/p{n}\n"));
    }
    assert_eq!(parse_profiles_ini(&text).len(), MAX_PROFILES);
    let huge = format!("[Profile0]\nPath=Profiles/a\n{}", "x".repeat(1_000_000));
    assert_eq!(parse_profiles_ini(&huge).len(), 1);
    // Cut inside a multi-byte character.
    let cut_text = format!("{}ü", "a".repeat(MAX_PROFILES_INI_BYTES - 1));
    assert!(parse_profiles_ini(&cut_text).is_empty());
}
