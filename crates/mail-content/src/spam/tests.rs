use mail_model::Address;

use super::score::chi2q;
use super::*;

const SPAM: &str = "Return-Path: <bounce-77@mailer.cheap-pills.example>\r\n\
Received: from mx.cheap-pills.example (unknown [203.0.113.7])\r\n\tby mx.local.example with ESMTP id 1234; Tue, 3 Mar 2026 10:00:00 +0000\r\n\
Authentication-Results: mx.local.example; spf=fail smtp.mailfrom=cheap-pills.example; dkim=none; dmarc=fail\r\n\
X-Spam-Status: Yes, score=9.1\r\n\
X-Spam-Level: *********\r\n\
From: \"Best Offers\" <offers1234@cheap-pills.example>\r\n\
Reply-To: winner@other.example\r\n\
To: undisclosed-recipients:;\r\n\
Subject: WIN A FREE IPHONE NOW!!!\r\n\
Message-ID: <x@cheap-pills.example>\r\n\
MIME-Version: 1.0\r\n\
Content-Type: text/html; charset=\"iso-8859-1\"\r\n\
\r\n\
<html><body><p>Congratulations! Click <a href=\"http://203.0.113.9/claim?id=1\">here</a> \
or visit <a href=\"https://claim.prizes.cheap-pills.example/win\">this link</a>.</p>\
<div style=\"display:none\">hidden filler words to fool filters</div>\
<img src=\"http://track.example/p.gif\"></body></html>\r\n";

const HAM: &str = "From: Alice Example <alice@friends.example>\r\n\
To: Bob <bob@local.example>\r\n\
Subject: Re: Lunch on Friday?\r\n\
Message-ID: <abc@friends.example>\r\n\
In-Reply-To: <def@local.example>\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Hi Bob, Friday works for me. Shall we meet at the usual place around noon?\r\n\
Cheers, Alice\r\n";

#[test]
fn token_hash_is_fnv1a_and_stable() {
    // Published FNV-1a 64 test vectors.
    assert_eq!(spam_token_hash(""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(spam_token_hash("a"), 0xaf63_dc4c_8601_ec8c);
    assert_eq!(spam_token_hash("foobar"), 0x8594_4171_f739_67e8);
}

#[test]
fn tokens_round_trip_through_bytes() {
    let tokens = spam_tokens_of_raw(SPAM.as_bytes());
    assert!(!tokens.is_empty());
    let back = SpamTokens::from_bytes(&tokens.to_bytes());
    assert_eq!(back, tokens);
    assert_eq!(tokens.to_bytes().len(), tokens.len() * 8);
    // Sorted and unique.
    assert!(tokens.hashes().windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn header_tokens_of_a_spam_mail() {
    let tokens = spam_tokens_of_raw(SPAM.as_bytes());
    for expected in [
        "from:addr:offers1234@cheap-pills.example",
        "from:domain:cheap-pills.example",
        "from:domain:tld:example",
        "from:local:digits",
        "from:name:best",
        "from:name:offers",
        "reply-to:domain:other.example",
        "reply-to:other-domain",
        "return-path:domain:mailer.cheap-pills.example",
        "return-path:other-domain",
        "received:mx.cheap-pills.example",
        "received:cheap-pills.example",
        "received:ip:203.0",
        "received:ip:203.0.113",
        "received:with:esmtp",
        "auth:spf=fail",
        "auth:dkim=none",
        "auth:dmarc=fail",
        "x-spam-status:yes",
        "x-spam-level:5",
        "to:undisclosed",
        "subject:win",
        "subject:free",
        "subject:iphone",
        "subject:caps",
        "subject:punct:!!!",
        "msgid:domain:cheap-pills.example",
        "ct:text/html",
        "charset:iso-8859-1",
        "h:mime-version",
        "h:x-spam-status",
    ] {
        assert!(tokens.contains(expected), "missing {expected}");
    }
    assert!(!tokens.contains("subject:reply"));
    assert!(!tokens.contains("msgid:none"));
}

#[test]
fn body_tokens_of_an_html_mail() {
    let tokens = spam_tokens_of_raw(SPAM.as_bytes());
    for expected in [
        "skip:c 10",
        "click",
        "visit",
        "html:only",
        "html:hidden-text",
        "url:ip",
        "url:scheme:http",
        "url:scheme:https",
        "url:prizes.cheap-pills.example",
        "url:cheap-pills.example",
        "url:path:claim",
        "url:track.example",
        "html:img:1",
    ] {
        assert!(tokens.contains(expected), "missing {expected}");
    }
    // Hidden text is not read as words.
    assert!(!tokens.contains("filler"));
}

#[test]
fn tokens_of_a_plain_reply() {
    let tokens = spam_tokens_of_raw(HAM.as_bytes());
    for expected in [
        "friday",
        "works",
        "noon",
        "usual",
        "subject:reply",
        "subject:lunch",
        "subject:friday",
        "subject:punct:?",
        "from:name:alice",
        "from:domain:friends.example",
        "to:count:1",
        "body:words:1",
        "ct:text/plain",
        "received:count:0",
    ] {
        assert!(tokens.contains(expected), "missing {expected}");
    }
    // Short words do not count; the subject's own words are tokens of their own kind.
    assert!(!tokens.contains("hi"));
    assert!(!tokens.contains("lunch"));
    assert!(!tokens.contains("html:only"));
}

#[test]
fn long_words_count_by_first_letter_and_length() {
    let raw =
        "From: a@b.example\r\nSubject: x\r\n\r\nsupercalifragilistic incomprehensibilities ok\r\n";
    let tokens = spam_tokens_of_raw(raw.as_bytes());
    assert!(tokens.contains("skip:s 20"));
    assert!(tokens.contains("skip:i 20"));
    assert!(!tokens.contains("ok"));
}

#[test]
fn words_are_trimmed_and_lower_cased_and_links_and_addresses_recognised() {
    let raw = "From: a@b.example\r\nSubject: x\r\n\r\n\"Hello,\" (World)! Price: $100 or 50% off. \
               Mail ME@Shop.Example or see www.shop.example/sale today.\r\n";
    let tokens = spam_tokens_of_raw(raw.as_bytes());
    for expected in [
        "hello",
        "world",
        "price",
        "$100",
        "50%",
        "email:addr:me@shop.example",
        "email:domain:shop.example",
        "url:shop.example",
        "url:path:sale",
        "today",
    ] {
        assert!(tokens.contains(expected), "missing {expected}");
    }
}

#[test]
fn tokens_are_bounded() {
    let mut raw = String::from("From: a@b.example\r\nSubject: many\r\n\r\n");
    for i in 0..20_000 {
        raw.push_str(&format!("word{i} "));
    }
    let tokens = spam_tokens_of_raw(raw.as_bytes());
    assert!(tokens.len() <= MAX_SPAM_TOKENS);
}

#[test]
fn broken_input_gives_tokens_without_panicking() {
    for raw in [
        &b""[..],
        b"\xff\xfe garbage \x00\x01",
        b"From: <>\r\nReceived: from [999.1.1.1]\r\nAuthentication-Results: =;=;spf=\r\n\r\n",
        b"Subject: \xc3\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\n",
    ] {
        let tokens = spam_tokens_of_raw(raw);
        assert!(tokens.len() <= MAX_SPAM_TOKENS);
    }
}

#[test]
fn stored_fields_give_a_subset_of_the_full_tokens() {
    let from = Address {
        name: Some("Alice Example".into()),
        email: "alice@friends.example".into(),
    };
    let fields = SpamFields {
        from: Some(&from),
        reply_to: &[],
        recipients: 1,
        subject: "Re: Lunch on Friday?",
        list_id: None,
        preview: Some("Hi Bob, Friday works for me. Shall we meet at the usual place around noon?"),
    };
    let partial = spam_tokens_of_fields(&fields);
    let full = spam_tokens_of_raw(HAM.as_bytes());
    assert!(partial.len() > 10);
    let missing: Vec<u64> = partial
        .hashes()
        .iter()
        .filter(|h| full.hashes().binary_search(h).is_err())
        .copied()
        .collect();
    assert!(
        missing.is_empty(),
        "{} tokens only in the stored fields",
        missing.len()
    );
}

#[test]
fn list_ids_are_spelled_alike_in_both_ways() {
    let raw = "From: news@list.example\r\nList-Id: Weekly News <News.List.Example>\r\nSubject: s\r\n\r\nx\r\n";
    let full = spam_tokens_of_raw(raw.as_bytes());
    let from = Address {
        name: None,
        email: "news@list.example".into(),
    };
    let partial = spam_tokens_of_fields(&SpamFields {
        from: Some(&from),
        list_id: Some("news.list.example"),
        subject: "s",
        ..SpamFields::default()
    });
    assert!(full.contains("list-id:news.list.example"));
    assert!(partial.contains("list-id:news.list.example"));
}

// ---------------------------------------------------------------------------------------------
// Scores

#[test]
fn chi2q_matches_known_values() {
    // P(X ≥ x) for chi-square with 2 degrees of freedom is exp(-x/2).
    assert!((chi2q(2.0, 2) - (-1.0f64).exp()).abs() < 1e-12);
    // With 4 degrees of freedom: exp(-x/2) (1 + x/2).
    assert!((chi2q(3.0, 4) - (-1.5f64).exp() * 2.5).abs() < 1e-12);
    assert!((chi2q(0.0, 10) - 1.0).abs() < 1e-12);
    assert!(chi2q(2000.0, 300) < 1e-100);
}

#[test]
fn token_probability_follows_robinson() {
    let totals = SpamTotals { spam: 10, ham: 10 };
    let unknown = token_probability(totals, SpamCounts::default());
    assert!((unknown - 0.5).abs() < 1e-12);
    // Seen once in spam only: (0.45·0.5 + 1·1) / 1.45.
    let once = token_probability(totals, SpamCounts { spam: 1, ham: 0 });
    assert!((once - (0.225 + 1.0) / 1.45).abs() < 1e-12);
    // Many times in ham only: close to 0, never 0.
    let ham = token_probability(totals, SpamCounts { spam: 0, ham: 10 });
    assert!(ham > 0.0 && ham < 0.03);
    // Relative frequencies count, not raw numbers.
    let skewed = SpamTotals { spam: 100, ham: 10 };
    let even = token_probability(skewed, SpamCounts { spam: 10, ham: 1 });
    assert!((even - 0.5).abs() < 0.01);
}

#[test]
fn nothing_is_said_before_both_classes_are_learned() {
    let counts = [SpamCounts { spam: 5, ham: 0 }];
    assert!(spam_score(SpamTotals { spam: 5, ham: 0 }, counts).is_none());
    assert!(spam_score(SpamTotals { spam: 0, ham: 5 }, counts).is_none());
}

#[test]
fn scores_follow_the_evidence() {
    let totals = SpamTotals { spam: 50, ham: 50 };
    let spammy = vec![SpamCounts { spam: 40, ham: 1 }; 30];
    let hammy = vec![SpamCounts { spam: 1, ham: 40 }; 30];
    let s = spam_score(totals, spammy.clone()).expect("score");
    let h = spam_score(totals, hammy.clone()).expect("score");
    assert!(s.score > 0.99, "{}", s.score);
    assert!(h.score < 0.01, "{}", h.score);
    assert_eq!(s.clues, 30);
    // Unknown tokens decide nothing.
    let unknown = spam_score(totals, vec![SpamCounts::default(); 30]).expect("score");
    assert!((unknown.score - 0.5).abs() < 1e-12);
    assert_eq!(unknown.clues, 0);
    // Strong evidence both ways is undecided, not spam.
    let mixed: Vec<SpamCounts> = spammy.into_iter().chain(hammy).collect();
    let m = spam_score(totals, mixed).expect("score");
    assert!((m.score - 0.5).abs() < 0.05, "{}", m.score);
}

#[test]
fn at_most_the_strongest_clues_decide() {
    let totals = SpamTotals {
        spam: 100,
        ham: 100,
    };
    let mut counts = vec![SpamCounts { spam: 3, ham: 1 }; 400];
    counts.extend(vec![SpamCounts { spam: 0, ham: 90 }; 10]);
    let score = spam_score(totals, counts).expect("score");
    assert_eq!(score.clues as usize, MAX_DISCRIMINATORS);
}
