//! Drafts of other programs (audit 24.11.2026): their own inline images stay through
//! [`foreign_draft_html`], under the Content-IDs the caller gives them; everything the editor
//! cannot keep is counted, so the caller can leave the original alone.

use std::collections::HashMap;

use super::{ForeignDraftHtml, foreign_draft_html, inline_image_ids};

fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

#[test]
fn own_inline_images_stay_with_size_and_text_around_them() {
    let html = "<html><body><p>Hello <b>Bob</b>,</p>\
                <p><img src=\"cid:logo@other.example\" width=\"120\" height=\"40\" alt=\"Logo\"></p>\
                <div>Regards<br><img src='CID:sig%40other.example' style='width: 64px'></div>\
                <script>alert(1)</script></body></html>";
    let out = foreign_draft_html(
        html,
        &map(&[
            ("logo@other.example", "logo@other.example"),
            ("sig@other.example", "sig@other.example"),
        ]),
    );
    assert_eq!(out.dropped, 0, "{out:?}");
    assert_eq!(out.shown, vec!["logo@other.example", "sig@other.example"]);
    assert!(
        out.editor_html.contains(
            "<img src=\"cid:logo@other.example\" alt=\"Logo\" width=\"120\" height=\"40\">"
        ),
        "{}",
        out.editor_html
    );
    assert!(
        out.editor_html
            .contains("<img src=\"cid:sig@other.example\" alt=\"\" width=\"64\">"),
        "{}",
        out.editor_html
    );
    assert!(out.editor_html.contains("<strong>Bob</strong>"));
    assert!(!out.editor_html.contains("script") && !out.editor_html.contains("alert"));
    // The editor reads it back as its own content with the same images.
    assert_eq!(
        inline_image_ids(&out.editor_html),
        vec!["logo@other.example", "sig@other.example"]
    );
}

#[test]
fn ids_other_programs_write_get_the_callers_new_id() {
    // Gmail's ids have no `@`; this module would not accept them in a `cid:`.
    let out = foreign_draft_html(
        "<p><img src=\"cid:ii_lk3j4h5g0\"></p>",
        &map(&[("ii_lk3j4h5g0", "0123abcd@example.org")]),
    );
    assert_eq!(out.dropped, 0);
    assert_eq!(out.shown, vec!["ii_lk3j4h5g0"]);
    assert!(
        out.editor_html.contains("cid:0123abcd@example.org"),
        "{}",
        out.editor_html
    );
    // One case-insensitive match is accepted, as in the reading pane; two are not.
    let one = foreign_draft_html(
        "<img src=\"cid:LOGO@x.example\">",
        &map(&[("logo@x.example", "logo@x.example")]),
    );
    assert_eq!((one.shown.len(), one.dropped), (1, 0));
    let two = foreign_draft_html(
        "<img src=\"cid:LOGO@x.example\">",
        &map(&[
            ("logo@x.example", "a@x.example"),
            ("Logo@x.example", "b@x.example"),
        ]),
    );
    assert_eq!((two.shown.len(), two.dropped), (0, 1));
}

#[test]
fn what_the_editor_cannot_keep_is_counted() {
    let keep = map(&[("logo@other.example", "logo@other.example")]);
    let count = |html: &str| foreign_draft_html(html, &keep).dropped;
    // An image from the web, a data: URL, a part the caller cannot keep.
    assert_eq!(count("<img src=\"https://cdn.example/logo.png\">"), 1);
    assert_eq!(count("<img src=\"data:image/png;base64,iVBORw0KGgo=\">"), 1);
    assert_eq!(count("<img src=\"cid:chart@other.example\">"), 1);
    // In preformatted text, inside a link (the link would be lost), as a background.
    assert_eq!(
        count("<pre>a<img src=\"cid:logo@other.example\">b</pre>"),
        1
    );
    assert_eq!(
        count("<a href=\"https://example.org\"><img src=\"cid:logo@other.example\"></a>"),
        1
    );
    assert_eq!(
        count("<table background=\"cid:logo@other.example\"><tr><td>x</td></tr></table>"),
        1
    );
    assert_eq!(
        count("<div style=\"background-image: URL(cid:logo@other.example)\">x</div>"),
        1
    );
    // Players, embedded objects, pictures with sources, drawings.
    assert_eq!(count("<video src=\"cid:clip@other.example\"></video>"), 1);
    assert_eq!(
        count("<picture><source srcset=\"a.webp\"><img src=\"cid:logo@other.example\"></picture>"),
        1
    );
    assert_eq!(count("<p><svg width=\"10\"><circle r=\"4\"/></svg></p>"), 1);
    // Text, links and kept images alone lose nothing that counts.
    assert_eq!(
        count(
            "<p>Hi <a href=\"https://example.org\">there</a></p><img src=\"cid:logo@other.example\">"
        ),
        0
    );
}

#[test]
fn html_the_guard_refuses_is_text_and_its_images_count() {
    let deep = format!(
        "{}<img src=\"cid:logo@other.example\">{}",
        "<div>".repeat(5_000),
        "</div>".repeat(5_000)
    );
    let out: ForeignDraftHtml =
        foreign_draft_html(&deep, &map(&[("logo@other.example", "logo@other.example")]));
    assert!(out.shown.is_empty());
    assert!(out.dropped >= 1, "{out:?}");
}
