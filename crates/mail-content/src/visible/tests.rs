//! Every technique with a hidden instruction and a visible text next to it; positive controls
//! with visible text in the same colours, sizes and positions; the corpus files
//! `testmail/corpus/hostile/hidden-text-*.eml`; seeded random input.

use std::path::Path;

use mail_model::RenderToken;

use super::MAX_LAYERS_VALUE;
use crate::{RenderOptions, html_to_text, html_to_visible_text, parse, render_html};

fn doc(body: &str) -> String {
    format!("<!DOCTYPE html><html><head></head><body>{body}</body></html>")
}

/// `HIDDEN` is left out and counted, `SHOWN` stays; without the check both are there.
#[track_caller]
fn hidden(body: &str) {
    let html = doc(body);
    let result = html_to_visible_text(&html);
    assert!(
        !result.text.contains("HIDDEN"),
        "hidden text kept: {body}\n→ {:?}",
        result.text
    );
    assert!(
        result.text.contains("SHOWN"),
        "visible text lost: {body}\n→ {:?}",
        result.text
    );
    assert!(
        result.hidden_chars >= "HIDDEN".len(),
        "not counted: {body} ({})",
        result.hidden_chars
    );
}

/// Nothing is left out.
#[track_caller]
fn shown(body: &str) {
    let html = doc(body);
    let result = html_to_visible_text(&html);
    assert_eq!(
        (result.text.as_str(), result.hidden_chars),
        (html_to_text(&html).as_str(), 0),
        "{body}"
    );
    assert!(result.text.contains("SHOWN"), "{body}: {:?}", result.text);
}

#[test]
fn display_attributes_and_unrendered_elements() {
    hidden("<div style='display:none'>HIDDEN</div><p>SHOWN</p>");
    hidden("<div style='DISPLAY: NONE !important'>HIDDEN</div><p>SHOWN</p>");
    hidden("<p>SHOWN <span hidden>HIDDEN</span></p>");
    hidden("<div hidden='until-found'>HIDDEN</div>SHOWN");
    hidden("<div style='content-visibility:hidden'>HIDDEN</div>SHOWN");
    hidden("<template><p>HIDDEN</p></template>SHOWN");
    hidden("<noscript><p>HIDDEN</p></noscript>SHOWN");
    hidden("<div style='visibility:hidden'>HIDDEN</div>SHOWN");
    hidden("<div style='visibility:collapse'><b>HIDDEN</b></div>SHOWN");
    // The mail sets `display` itself: the attribute no longer hides.
    shown("<div hidden style='display:block'>SHOWN</div>");
    // A value the sanitiser does not keep: the pane has no `hidden` attribute.
    shown("<div hidden='x:y'>SHOWN</div>");
    // A child made visible again.
    let html =
        doc("<div style='visibility:hidden'>HIDDEN <p style='visibility:visible'>SHOWN</p></div>");
    let result = html_to_visible_text(&html);
    assert_eq!(result.text, "SHOWN");
    assert_eq!(result.hidden_chars, 6);
    // Decided: `aria-hidden` alone hides nothing from a sighted reader.
    shown("<p aria-hidden='true'>SHOWN</p>");
}

#[test]
fn closed_details_show_only_their_first_summary() {
    for body in [
        "<details><summary>SHOWN</summary><p>HIDDEN</p></details>",
        "<details><summary>SHOWN</summary>HIDDEN right in it</details>",
        "<details>HIDDEN before it<summary>SHOWN</summary></details>",
        "<details><summary>SHOWN</summary><summary>HIDDEN</summary></details>",
        // Without a summary of its own the browser shows "Details" and nothing else.
        "<p>SHOWN</p><details><p>HIDDEN</p></details>",
        "<p>SHOWN</p><details><div><summary>HIDDEN</summary></div></details>",
        // CSS on the content or the element does not open it (checked in WebKit and Blink).
        "<style>details > p{display:block!important}</style><details><summary>SHOWN</summary><p>HIDDEN</p></details>",
        "<details style='display:contents'><summary>SHOWN</summary><p>HIDDEN</p></details>",
        // The sanitiser drops an `open` with such a value.
        "<details open='x:y'><summary>SHOWN</summary><p>HIDDEN</p></details>",
        // An element the sanitiser unwraps: its summary is the first child in the pane.
        "<details><x-wrap><summary>SHOWN</summary></x-wrap><summary>HIDDEN</summary></details>",
        // The first summary takes its place even when CSS hides it.
        "<p>SHOWN</p><details><summary style='display:none'>HIDDEN</summary><summary>HIDDEN too</summary></details>",
        "<details open><summary>SHOWN</summary><details><summary>SHOWN too</summary><p>HIDDEN</p></details></details>",
    ] {
        hidden(body);
    }
    // After the element everything shows again.
    let result = html_to_visible_text(&doc(
        "<details><summary>A</summary><p>HIDDEN</p></details><p>SHOWN</p>",
    ));
    assert_eq!(result.text, "A\n\nSHOWN");
    assert_eq!(result.hidden_chars, 6);
    // The pane gets the same `open` attributes the check reads.
    let options = RenderOptions::new(RenderToken::from_random(1));
    let pane = |body: &str| render_html(&doc(body), &options).srcdoc;
    assert!(!pane("<details open='x:y'><summary>A</summary></details>").contains("open"));
    assert!(pane("<details open><summary>A</summary></details>").contains("<details open"));
    // Positive controls: open, and only a summary.
    shown("<details open><summary>SHOWN</summary><p>SHOWN too</p></details>");
    shown("<details open=''><summary>SHOWN</summary>SHOWN too</details>");
    shown("<details OPEN='open'><summary>SHOWN</summary><div><p>SHOWN too</p></div></details>");
    shown("<details><summary>SHOWN <b>in bold</b> <x-wrap>wrapped</x-wrap></summary></details>");
}

#[test]
fn opacity_and_filters() {
    hidden("<div style='opacity:0'>HIDDEN</div>SHOWN");
    hidden("<div style='opacity:0.01'>HIDDEN</div>SHOWN");
    hidden("<div style='opacity:2%'>HIDDEN</div>SHOWN");
    hidden("<div style='opacity:calc(0.1 / 10)'>HIDDEN</div>SHOWN");
    hidden("<div style='opacity:.2'><p style='opacity:.2'>HIDDEN</p></div>SHOWN");
    hidden("<div style='filter:opacity(0)'>HIDDEN</div>SHOWN");
    hidden("<div style='-webkit-filter:blur(1px) opacity(0%)'>HIDDEN</div>SHOWN");
    shown("<div style='opacity:0.5'>SHOWN</div>");
    shown("<div style='opacity:0.2'><p style='opacity:1'>SHOWN</p></div>");
    shown("<div style='filter:grayscale(1)'>SHOWN</div>");
}

#[test]
fn tiny_fonts() {
    for style in [
        "font-size:0",
        "font-size:0px",
        "font-size:1px",
        "font-size:3.9px",
        "font-size:2pt",
        "font-size:0.1em",
        "font-size:1%",
        "font-size:calc(1px - 1px)",
        "font-size:min(0px, 12px)",
        "font-size:clamp(0px, 0px, 1px)",
        "font:0/0 a",
        "font:italic bold 1px/0 Arial",
    ] {
        hidden(&format!("<p style='{style}'>HIDDEN</p><p>SHOWN</p>"));
    }
    hidden("<div style='font-size:1px'><span style='font-size:200%'>HIDDEN</span></div>SHOWN");
    // Positive controls: fine print, the boundary, and the `font-size:0` layout trick where
    // the content sets its own size.
    shown("<p style='font-size:9px'>SHOWN</p>");
    shown("<p style='font-size:4px'>SHOWN</p>");
    shown("<p style='font:bold 12px/1.4 Arial'>SHOWN</p>");
    shown("<p style='font:700 13px Arial'>SHOWN</p>");
    shown(
        "<table><tr><td style='font-size:0'><div style='display:inline-block;font-size:14px'>SHOWN</div></td></tr></table>",
    );
    shown("<p style='font-size:-1px'>SHOWN</p>");
    shown("<p style='font-size:foo'>SHOWN</p>");
    shown("<p style='font-size:1px; font-size:16px'>SHOWN</p>");
}

#[test]
fn clipped_boxes() {
    for style in [
        "max-height:0;overflow:hidden",
        "max-height:0px;overflow:hidden;mso-hide:all",
        "height:0;overflow:hidden",
        "height:1px;overflow:hidden",
        "width:0;overflow:hidden",
        "max-width:0;overflow:clip",
        "height:0;overflow-y:hidden",
        "height:0;overflow-x:hidden",
        "max-block-size:0;overflow:hidden",
        "position:absolute;width:1px;height:1px;overflow:hidden",
    ] {
        hidden(&format!("<div style='{style}'>HIDDEN</div><p>SHOWN</p>"));
    }
    // Browsers ignore heights on inline elements and grow table cells to their content.
    shown("<span style='height:0;overflow:hidden'>SHOWN</span>");
    shown("<table><tr><td style='height:0;overflow:hidden'>SHOWN</td></tr></table>");
    shown("<div style='height:0'>SHOWN</div>");
    shown("<div style='max-height:20px;overflow:hidden'>SHOWN</div>");
    shown("<div style='height:0;overflow:visible'>SHOWN</div>");
}

#[test]
fn colours_like_the_background() {
    for body in [
        "<p style='color:#fff'>HIDDEN</p>",
        "<p style='color:#FFFFFF;background:#ffffff'>HIDDEN</p>",
        "<p style='color:white'>HIDDEN</p>",
        "<p style='color:#fefefe'>HIDDEN</p>",
        "<p style='color:snow'>HIDDEN</p>",
        "<p style='color:hsl(0 0% 100%)'>HIDDEN</p>",
        "<p style='color:oklch(1 0 0)'>HIDDEN</p>",
        "<p style='color:red;background-color:red'>HIDDEN</p>",
        "<p style='color:#123456;background:#123456 none'>HIDDEN</p>",
        "<p style='color:transparent'>HIDDEN</p>",
        "<p style='color:rgba(0,0,0,0)'>HIDDEN</p>",
        "<p style='color:#0000'>HIDDEN</p>",
        "<p style='-webkit-text-fill-color:transparent'>HIDDEN</p>",
        "<p style='color:#00f;background:currentcolor'>HIDDEN</p>",
        "<p style='color:rgb(from white r g b)'>HIDDEN</p>",
        "<font color='white'>HIDDEN</font>",
        "<font color='ffffff'>HIDDEN</font>",
        "<table bgcolor='#000000'><tr><td><span style='color:#000'>HIDDEN</span></td></tr></table>",
        "<div style='background:#336699'><div><p style='color:#336699'>HIDDEN</p></div></div>",
        "<div style='background:rgba(0,0,0,0.5)'><p style='color:#808080'>HIDDEN</p></div>",
        "<mark style='color:yellow'>HIDDEN</mark>",
        "<a href='https://shop.example/' style='color:#fff'>HIDDEN</a>",
        "<style>.w{color:#fff}</style><p class='w'>HIDDEN</p>",
    ] {
        hidden(&format!("{body}<p>SHOWN</p>"));
    }
    // Positive controls: the same colours on a contrasting backdrop, faint but readable grey,
    // links in a white container, text over images and gradients, text drawn otherwise.
    for body in [
        "<table bgcolor='#000000'><tr><td><font color='#ffffff'>SHOWN</font></td></tr></table>",
        "<table bgcolor='000000'><tr><td><font color='ffffff'>SHOWN</font></td></tr></table>",
        "<p style='color:#fff;background:#000'>SHOWN</p>",
        "<div style='background-color:#222'><p style='color:white'>SHOWN</p></div>",
        "<div style='background:#000'><p style='color:#fff'>SHOWN</p></div>",
        "<p style='color:#eee'>SHOWN</p>",
        "<p style='color:#ccc'>SHOWN</p>",
        "<p style='color:rgba(0,0,0,0.2)'>SHOWN</p>",
        "<div style='color:#fff'><a href='https://shop.example/'>SHOWN</a></div>",
        "<table><tr><td style='background-image:url(cid:hero)'><p style='color:#fff'>SHOWN</p></td></tr></table>",
        "<div background='cid:hero'><p style='color:#fff'>SHOWN</p></div>",
        "<div style='background:linear-gradient(#000, #333)'><p style='color:#fff'>SHOWN</p></div>",
        "<p style='color:transparent;text-shadow:0 0 0 #000'>SHOWN</p>",
        "<p style='color:transparent;-webkit-text-stroke:1px #000'>SHOWN</p>",
        "<p style='color:transparent;-webkit-background-clip:text;background-image:linear-gradient(red, blue)'>SHOWN</p>",
        "<p style='color:#fff;color:#000'>SHOWN</p>",
        "<p style='color:notacolor'>SHOWN</p>",
        "<p style='color:AccentColor'>SHOWN</p>",
        "<style>body{background:#000;color:#fff}</style><p>SHOWN</p>",
    ] {
        shown(body);
    }
    shown("<body bgcolor='black' text='white'><p>SHOWN</p></body>");
}

#[test]
fn text_shadows_draw_text_unless_they_do_not_show_either() {
    for style in [
        "color:transparent;text-shadow:0 0 0 transparent",
        "color:transparent;text-shadow:1px 1px 0 rgba(255,0,0,0)",
        "color:transparent;text-shadow:0 0 #ff000008",
        // Without a colour the shadow takes the transparent text colour.
        "color:transparent;text-shadow:2px 2px",
        "color:transparent;text-shadow:2px 2px currentcolor",
        "color:transparent;text-shadow:0 0 0 transparent, 1px 1px rgba(0,0,0,0)",
        "-webkit-text-fill-color:transparent;color:transparent;text-shadow:0 0",
        // White glyphs with a white shadow on white.
        "color:#fff;text-shadow:0 0 0 #fff",
        "color:transparent;text-shadow:0 0 0 #fefefe",
        // Off the screen.
        "color:transparent;text-shadow:-9999px 0 0 red",
        "color:transparent;text-shadow:0 -600px red",
        // Invalid: the browser draws no shadow.
        "color:transparent;text-shadow:red",
        "color:transparent;text-shadow:1px 1px 1px 1px red",
        "color:transparent;text-shadow:1px 1px red blue",
        "color:transparent;text-shadow:1 1 red",
        "color:transparent;text-shadow:0 0 0 #000;text-shadow:none",
    ] {
        hidden(&format!("<p style='{style}'>HIDDEN</p><p>SHOWN</p>"));
    }
    // `currentcolor` is the colour where the text is, not where the shadow is set.
    hidden(
        "<div style='text-shadow:0 0 0'><p style='color:transparent'>HIDDEN</p></div><p>SHOWN</p>",
    );
    hidden(
        "<div style='color:transparent;text-shadow:0 0 red'><p style='text-shadow:initial'>HIDDEN</p>SHOWN</div>",
    );
    hidden(
        "<style>.s{color:transparent;text-shadow:0 0 transparent}</style><p class='s'>HIDDEN</p><p>SHOWN</p>",
    );
    // A value too long to evaluate draws nothing (fail closed).
    let long = "0 0 red, ".repeat(MAX_LAYERS_VALUE / 8);
    hidden(&format!(
        "<p style='color:transparent;text-shadow:{long}0 0 red'>HIDDEN</p><p>SHOWN</p>"
    ));
    shown(&format!("<p style='text-shadow:{long}0 0 red'>SHOWN</p>"));
    // Positive controls: a shadow that shows draws the text.
    for style in [
        "color:transparent;text-shadow:0 0 0 red",
        "color:transparent;text-shadow:red 1px 1px",
        "color:transparent;text-shadow:1px 1px transparent, 0 0 0 #000",
        "color:transparent;text-shadow:0 0 0 #000, -9999px 0 red",
        "color:transparent;text-shadow:0 0 5px rgba(0,0,0,0.5)",
        "color:transparent;text-shadow:calc(1px + 1vw) 0 #000",
        "color:transparent;text-shadow:0 0 1em AccentColor",
        "-webkit-text-fill-color:transparent;text-shadow:0 0 0",
        "color:#fff;text-shadow:1px 1px 2px #000",
        "color:transparent;text-shadow:0 -100px #000",
    ] {
        shown(&format!("<p style='{style}'>SHOWN</p>"));
    }
    shown("<div style='color:transparent;text-shadow:0 0 0 #000'><p>SHOWN</p></div>");
    shown(
        "<div style='text-shadow:0 0 0'><p style='-webkit-text-fill-color:transparent'>SHOWN</p></div>",
    );
}

#[test]
fn zoom_scales_the_text() {
    for style in [
        "zoom:0.05",
        "zoom:5%",
        "zoom:0.2",
        "font-size:12px;zoom:0.3",
        "zoom:calc(1 / 10)",
    ] {
        hidden(&format!("<p style='{style}'>HIDDEN</p><p>SHOWN</p>"));
    }
    // Multiplied through the ancestors.
    hidden(
        "<div style='zoom:0.5'><p style='zoom:0.5'><span style='zoom:0.5'>HIDDEN</span></p></div><p>SHOWN</p>",
    );
    hidden("<div style='font-size:12px;zoom:0.5'>SHOWN<p style='zoom:inherit'>HIDDEN</p></div>");
    hidden("<style>.z{zoom:4%}</style><p class='z'>HIDDEN</p><p>SHOWN</p>");
    // `initial` resets the element's own factor, not its ancestors'.
    hidden("<div style='zoom:0.2'><p style='zoom:initial'>HIDDEN</p></div><p>SHOWN</p>");
    // Positive controls. `zoom:0` and `0%` count as 1 in WebKit and Blink.
    for style in [
        "zoom:0",
        "zoom:0%",
        "zoom:1",
        "zoom:normal",
        "zoom:0.5",
        "zoom:-1",
        "zoom:2",
        "font-size:2px;zoom:4",
    ] {
        shown(&format!("<p style='{style}'>SHOWN</p>"));
    }
    shown("<div style='zoom:0.2'><p style='zoom:5'>SHOWN</p></div>");
}

#[test]
fn masks_that_let_nothing_through() {
    for style in [
        "mask-image:linear-gradient(transparent,transparent)",
        "-webkit-mask-image:linear-gradient(rgba(0,0,0,0) 0%, rgba(0,0,0,0) 100%)",
        "mask:linear-gradient(transparent,transparent)",
        "-webkit-mask:linear-gradient(to right, #0000, transparent 50%) no-repeat center / cover",
        "mask-image:radial-gradient(circle at center, transparent 0, transparent 100%)",
        "mask-image:repeating-linear-gradient(45deg, transparent, transparent 10px)",
        "mask-image:conic-gradient(from 90deg, hsl(0 0% 0% / 0), transparent)",
        "mask-image:-webkit-linear-gradient(top, transparent, transparent)",
        "mask-image:linear-gradient(in oklch, transparent, transparent)",
        "mask-image:none, linear-gradient(transparent, transparent)",
        "mask-image:linear-gradient(transparent,transparent), linear-gradient(#0000,#0000)",
        "mask-image:linear-gradient(transparent,transparent) !important",
    ] {
        hidden(&format!("<div style='{style}'>HIDDEN</div><p>SHOWN</p>"));
    }
    // Inline elements too, and everything inside.
    hidden(
        "<p>SHOWN <span style='-webkit-mask-image:linear-gradient(transparent,transparent)'>HIDDEN</span></p>",
    );
    hidden(
        "<div style='mask:linear-gradient(transparent,transparent)'><p style='color:#000;opacity:1'>HIDDEN</p></div><p>SHOWN</p>",
    );
    // `currentcolor` in the mask is the element's colour.
    hidden(
        "<div style='color:transparent;mask-image:linear-gradient(currentcolor,currentcolor)'><p style='color:#000'>HIDDEN</p></div><p>SHOWN</p>",
    );
    hidden(
        "<style>.m{-webkit-mask:linear-gradient(transparent,transparent)}</style><p class='m'>HIDDEN</p><p>SHOWN</p>",
    );
    // A value too long to evaluate lets nothing through (fail closed).
    let long = "linear-gradient(#000,#000), ".repeat(MAX_LAYERS_VALUE / 20);
    hidden(&format!(
        "<div style='mask-image:{long}none'>HIDDEN</div><p>SHOWN</p>"
    ));
    // Positive controls: masks that show the content or part of it, and masks not evaluated.
    for style in [
        "mask:linear-gradient(#000,#000)",
        "mask-image:linear-gradient(transparent, #000)",
        "-webkit-mask-image:linear-gradient(to bottom, rgba(0,0,0,1) 60%, rgba(0,0,0,0))",
        "mask-image:linear-gradient(transparent,transparent), linear-gradient(#000,#000)",
        "mask-image:linear-gradient(currentcolor, currentcolor)",
        "mask-image:linear-gradient(AccentColor, transparent)",
        "mask-image:linear-gradient(foo)",
        "mask-image:none",
        "mask:none",
        "mask-image:url(cid:mask)",
        "mask:url(#m)",
        "mask-image:inherit",
        "mask-image:linear-gradient(transparent,transparent);mask-image:none",
    ] {
        shown(&format!("<div style='{style}'>SHOWN</div>"));
    }
}

#[test]
fn moved_off_the_screen_or_clipped_away() {
    for style in [
        "position:absolute;left:-9999px",
        "position:absolute;top:-10000px",
        "position:fixed;left:-100%",
        "position:relative;right:9999px",
        "position:absolute;bottom:600px",
        "position:absolute;inset:0 0 0 -9999px",
        "text-indent:-9999px",
        "text-indent:-999em",
        "margin-left:-9999px",
        "margin:0 0 0 -9999px",
        "margin-top:-2000px",
        "transform:translateX(-9999px)",
        "transform:translate(0, -9999px)",
        "-webkit-transform:scale(0)",
        "transform:scaleY(0)",
        "translate:-9999px",
        "scale:0",
        "position:absolute;clip:rect(0 0 0 0)",
        "position:absolute;clip:rect(1px, 1px, 1px, 1px)",
        "clip-path:inset(50%)",
        "clip-path:inset(0 50%)",
        "clip-path:circle(0)",
        "-webkit-clip-path:ellipse(0 10px)",
    ] {
        hidden(&format!("<div style='{style}'>HIDDEN</div><p>SHOWN</p>"));
    }
    // A child inherits the indent; one that resets it is visible.
    hidden(
        "<div style='text-indent:-9999px'><p>HIDDEN</p><p style='text-indent:0'>SHOWN</p></div>",
    );
    // Positive controls: ordinary layout offsets and clips.
    for style in [
        "position:absolute;left:-20px",
        "position:relative;top:-100px",
        "position:sticky;top:-9999px",
        "left:-9999px",
        "margin-left:-100px",
        "text-indent:-2em",
        "transform:translateX(-50%)",
        "transform:scale(0.5)",
        "clip:rect(0 0 0 0)",
        "position:absolute;clip:rect(0 100px 20px 0)",
        "clip-path:inset(10%)",
        "clip-path:circle(50%)",
    ] {
        shown(&format!("<div style='{style}'>SHOWN</div>"));
    }
    shown("<span style='transform:scale(0)'>SHOWN</span>");
}

#[test]
fn style_sheet_rules() {
    for (css, body) in [
        (".h{display:none}", "<p class='h'>HIDDEN</p>"),
        ("#h{display:none}", "<p id='h'>HIDDEN</p>"),
        ("p.h{font-size:0}", "<p class='x h'>HIDDEN</p>"),
        ("div > .h{opacity:0}", "<div><p class='h'>HIDDEN</p></div>"),
        (
            ".a .b{visibility:hidden}",
            "<div class='a'><table><tr><td><span class='b'>HIDDEN</span></td></tr></table></div>",
        ),
        (
            "p:not(.keep){display:none}",
            "<p>HIDDEN</p><div>SHOWN too</div>",
        ),
        (
            ".h:first-child{display:none}",
            "<div><p class='h'>HIDDEN</p><p class='h'>SHOWN too</p></div>",
        ),
        (
            "p:nth-child(2){display:none}",
            "<div><p>SHOWN too</p><p>HIDDEN</p></div>",
        ),
        ("h2 + p{display:none}", "<h2>SHOWN too</h2><p>HIDDEN</p>"),
        (":is(.x, .h){display:none}", "<p class='h'>HIDDEN</p>"),
        (".h:read-only{display:none}", "<p class='h'>HIDDEN</p>"),
        (
            "[title=secret]{display:none}",
            "<p title='secret'>HIDDEN</p>",
        ),
        ("[lang|=en]{display:none}", "<p lang='en-GB'>HIDDEN</p>"),
        (
            "*{color:#fff} .v{color:#000}",
            "<p>HIDDEN</p><p class='v'>SHOWN too</p>",
        ),
        ("@media screen{.h{display:none}}", "<p class='h'>HIDDEN</p>"),
        (
            "@media (min-width:600px){.h{display:none}}",
            "<p class='h'>HIDDEN</p>",
        ),
        (
            "@media (orientation:portrait){.h{display:none}}",
            "<p class='h'>HIDDEN</p>",
        ),
        (
            "@media screen{@media all{.h{display:none}}}",
            "<p class='h'>HIDDEN</p>",
        ),
        (
            ".h{display:none!important}",
            "<p class='h' style='display:block'>HIDDEN</p>",
        ),
        (
            "#h{color:#fff} .c{color:#000}",
            "<p id='h' class='c'>HIDDEN</p>",
        ),
        (".h{display:none}", "<P CLASS='h'>HIDDEN</P>"),
    ] {
        hidden(&format!(
            "<style>{css}</style>{body}<p class='keep'>SHOWN</p>"
        ));
    }
    // Rules that do not apply in the reading pane.
    for (css, body) in [
        (
            "@media (max-width:600px){.d{display:none}}",
            "<p class='d'>SHOWN</p>",
        ),
        ("@media print{p{display:none}}", "<p>SHOWN</p>"),
        (
            "@media (prefers-color-scheme:dark){p{color:#fff}}",
            "<p>SHOWN</p>",
        ),
        ("p:hover{display:none}", "<p>SHOWN</p>"),
        (
            "a:visited{color:#fff}",
            "<a href='https://shop.example/'>SHOWN</a>",
        ),
        ("p::before{display:none}", "<p>SHOWN</p>"),
        ("p:first-line{color:#fff}", "<p>SHOWN</p>"),
        (".H{display:none}", "<p class='h'>SHOWN</p>"),
        // The sanitiser removes the attribute, so the rule matches nothing in the pane.
        ("[data-x]{display:none}", "<p data-x='1'>SHOWN</p>"),
        // An unwrapped element is not in the pane, its style attribute neither.
        (
            "custom{display:none}",
            "<custom style='display:none'>SHOWN</custom>",
        ),
        // Important stylesheet beats the inline style.
        (
            "@media screen{.m{display:block !important}}",
            "<div class='m' style='display:none'>SHOWN</div>",
        ),
        (
            ".h{display:none} .h{display:block}",
            "<p class='h'>SHOWN</p>",
        ),
        (
            "#a .h{display:none}",
            "<div id='b'><p class='h'>SHOWN</p></div>",
        ),
        (
            "div > .h{display:none}",
            "<div><table><tr><td><p class='h'>SHOWN</p></td></tr></table></div>",
        ),
    ] {
        shown(&format!("<style>{css}</style>{body}"));
    }
}

#[test]
fn very_long_selectors_neither_overflow_nor_escape() {
    // 50,000 compounds: matched by the rightmost one, on a thread with a small stack.
    let selector = "div ".repeat(50_000);
    let html =
        format!("<style>{selector}p{{display:none}}</style><div><p>HIDDEN</p></div><b>SHOWN</b>");
    let result = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || html_to_visible_text(&doc(&html)))
        .expect("thread")
        .join()
        .expect("no overflow");
    assert_eq!(result.text, "SHOWN");
}

#[test]
fn hidden_link_text_takes_the_target_along() {
    let html = doc(
        "<p><a href='https://evil.example/HIDDEN' style='color:#fff'>x</a> \
         <a href='https://shop.example/SHOWN'>deals</a> \
         <a href='https://shop.example/image'><img src='cid:x' alt=''></a></p>",
    );
    let result = html_to_visible_text(&html);
    assert_eq!(
        result.text,
        "deals <https://shop.example/SHOWN> <https://shop.example/image>"
    );
    assert_eq!(result.hidden_chars, 1);
}

#[test]
fn counts_and_structure() {
    let html = doc(
        "<h1>SHOWN</h1><div style='display:none'>one two <b>three</b><script>x()</script>\
         <template>four</template><noscript><p>five</p></noscript></div>\
         <p style='color:#fff'>six seven</p><ul><li>a</li><li style='font-size:0'>eight</li></ul>",
    );
    let result = html_to_visible_text(&html);
    assert_eq!(result.text, "SHOWN\n\n• a\n•");
    assert_eq!(
        result.hidden_chars,
        "onetwothreefourfivesixseveneight".len()
    );
    // Too complex to parse: the pane shows it as plain text, so it is all visible.
    let deep = format!(
        "<p style='display:none'>SHOWN</p>{}x{}",
        "<div>".repeat(3000),
        "</div>".repeat(3000)
    );
    let result = html_to_visible_text(&deep);
    assert!(result.text.contains("SHOWN"));
    assert_eq!(result.hidden_chars, 0);
}

#[test]
fn matching_budget_fails_closed() {
    // 2,000 rules with long descendant chains that pass the ancestor-key test but never match
    // (no element is a second child), on 120 nested elements: every rule searches all
    // ancestors, far beyond the budget.
    let mut css = String::new();
    for i in 0..2000 {
        css.push_str(&format!(
            ".a:nth-child(2) .a .a .a .a .a{{color:#00{:04x}}}\n",
            i % 0xFFFF
        ));
    }
    let html = format!(
        "<html><head><style>{css}</style></head><body><div class='a'>SHOWN {}HIDDEN{}</div></body></html>",
        "<div class='a'>".repeat(120),
        "</div>".repeat(120)
    );
    let started = std::time::Instant::now();
    let result = html_to_visible_text(&html);
    assert!(result.text.contains("SHOWN"), "{:?}", result.text);
    assert!(!result.text.contains("HIDDEN"), "{:?}", result.text);
    assert!(result.hidden_chars >= 6);
    assert!(started.elapsed().as_secs() < 30, "{:?}", started.elapsed());
}

/// Whether the non-white-space characters of `part` appear in `whole` in the same order.
fn is_subsequence(part: &str, whole: &str) -> bool {
    let mut whole = whole.chars().filter(|c| !c.is_whitespace());
    part.chars()
        .filter(|c| !c.is_whitespace())
        .all(|c| whole.any(|w| w == c))
}

/// Words of the form `HIDDEN-…`/`VISIBLE-…` in `text`.
fn markers<'a>(text: &'a str, prefix: &str) -> Vec<&'a str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .filter(|word| word.starts_with(prefix) && word.len() > prefix.len())
        .collect()
}

fn corpus_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testmail/corpus")
}

fn eml_files(folder: &str) -> Vec<std::path::PathBuf> {
    let dir = corpus_dir().join(folder);
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "eml"))
        .collect();
    files.sort();
    files
}

#[test]
fn hidden_text_corpus() {
    let files: Vec<_> = eml_files("hostile")
        .into_iter()
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("hidden-text-"))
        })
        .collect();
    assert!(
        files.len() >= 6,
        "hidden-text corpus missing: {}",
        files.len()
    );
    let mut hidden_total = 0;
    for path in &files {
        let name = path.display().to_string();
        let raw = std::fs::read(path).expect("corpus file");
        assert!(
            raw.windows(2).any(|w| w == b"\r\n")
                && !raw.windows(2).any(|w| w[1] == b'\n' && w[0] != b'\r'),
            "{name}: not CRLF"
        );
        let message = parse(&raw);
        let html = message.html.as_deref().expect("HTML body");
        let full = html_to_text(html);
        let visible = html_to_visible_text(html);
        let hidden_markers = markers(html, "HIDDEN-");
        let visible_markers = markers(html, "VISIBLE-");
        assert!(hidden_markers.len() >= 4, "{name}: {hidden_markers:?}");
        assert!(visible_markers.len() >= 3, "{name}: {visible_markers:?}");
        for marker in &hidden_markers {
            assert!(
                !visible.text.contains(marker),
                "{name}: {marker} reached the text:\n{}",
                visible.text
            );
        }
        for marker in &visible_markers {
            assert!(
                visible.text.contains(marker),
                "{name}: visible {marker} lost:\n{}",
                visible.text
            );
            assert!(
                full.contains(marker),
                "{name}: {marker} not in the full text"
            );
        }
        assert!(visible.text.contains("SAFE-CONTENT-VISIBLE"), "{name}");
        // Positive control: without the check the instructions are in the text.
        let leaked = hidden_markers
            .iter()
            .filter(|marker| full.contains(**marker))
            .count();
        assert!(
            leaked * 2 >= hidden_markers.len(),
            "{name}: the full text has only {leaked} of {} hidden markers",
            hidden_markers.len()
        );
        assert!(visible.hidden_chars > 0, "{name}");
        hidden_total += visible.hidden_chars;
        // What a model gets through `ParsedMessage::visible_text`.
        assert_eq!(message.visible_text(), visible, "{name}");
    }
    assert!(hidden_total > 500, "{hidden_total}");
}

#[test]
fn ordinary_mails_lose_nothing_visible() {
    // The rest of the hostile corpus and the seed mails: the benign markers and every word a
    // reader sees stay.
    let mut checked = 0;
    for folder in ["hostile", "seed"] {
        for path in eml_files(folder) {
            let name = path.display().to_string();
            if name.contains("hidden-text-") {
                continue;
            }
            let message = parse(&std::fs::read(&path).expect("corpus file"));
            let Some(html) = message.html.as_deref() else {
                continue;
            };
            checked += 1;
            let full = html_to_text(html);
            let visible = html_to_visible_text(html);
            if full.contains("SAFE-CONTENT-VISIBLE") {
                assert!(visible.text.contains("SAFE-CONTENT-VISIBLE"), "{name}");
            }
            assert!(
                is_subsequence(&visible.text, &full),
                "{name}: text not from the mail"
            );
            if folder == "seed" {
                assert_eq!(visible.text, full, "{name}: seed mail lost text");
            }
        }
    }
    assert!(checked >= 20, "{checked}");
}

#[test]
fn random_markup_and_styles_never_panic() {
    let parts = [
        "<div>",
        "</div>",
        "<p class='a'>",
        "</p>",
        "<span style='",
        "'>",
        "</span>",
        "<style>",
        "</style>",
        ".a{",
        "}",
        "display:none;",
        "color:#fff;",
        "font-size:0;",
        "@media screen{",
        "background:#fff;",
        "opacity:0;",
        "position:absolute;left:-9999px;",
        "<table bgcolor=#000>",
        "<td>",
        "</table>",
        "text",
        " ",
        "<a href='https://x.example'>",
        "</a>",
        "<template>",
        "</template>",
        "<font color=white>",
        "#a .a > p ~ b + i",
        ",",
        "<b id=a>",
        "calc(",
        ")",
        "rgb(from red r g b)",
        "<!--",
        "-->",
        "\u{0}",
        "&nbsp;",
        "<svg><text>x</text></svg>",
        ":not(",
        ":nth-child(2n+1)",
        "[x=",
        "]",
        "!important",
    ];
    let mut state: u64 = 0x5EED_CAFE;
    for _ in 0..400 {
        let mut html = String::new();
        for _ in 0..60 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            html.push_str(parts[(state % parts.len() as u64) as usize]);
        }
        let full = html_to_text(&html);
        let visible = html_to_visible_text(&html);
        assert!(
            is_subsequence(&visible.text, &full),
            "{html}\n{:?} not from {full:?}",
            visible.text
        );
    }
}

/// Matching steps the visibility check needs for `html`.
fn steps(html: &str) -> u64 {
    let dom = crate::dom::parse_document(html);
    let mut visibility = super::Visibility::new(&dom);
    crate::dom::walk(&dom.document, |visit| match visit {
        crate::dom::Visit::Enter(node) => match crate::dom::html_name(node) {
            Some(name) if !crate::text::SKIPPED.contains(&name) => {
                if visibility.enter(node, name) {
                    crate::dom::Descend::Yes
                } else {
                    crate::dom::Descend::Skip
                }
            }
            Some(_) => crate::dom::Descend::Skip,
            None => crate::dom::Descend::Yes,
        },
        crate::dom::Visit::Leave(node) => {
            if crate::dom::html_name(node).is_some() {
                visibility.leave();
            }
            crate::dom::Descend::Yes
        }
    });
    assert!(!visibility.exhausted);
    visibility.steps_used()
}

#[test]
fn newsletters_use_a_small_part_of_the_budget() {
    // A large newsletter: 300 rules of the usual kinds, 60 articles in tables nested 12 deep,
    // about 4,000 elements.
    let mut css = String::new();
    for i in 0..60 {
        css.push_str(&format!(
            ".c{i}{{color:#333}} table.t{i} td{{padding:0}} .footer .c{i} a{{color:#999}} \
             #a{i} > p{{margin:0}} td.c{i} span:first-child{{font-size:12px}}\n"
        ));
    }
    css.push_str("@media only screen and (max-width:600px){.c1{display:none}}\n");
    let mut body = String::new();
    for i in 0..60 {
        body.push_str(&"<table><tr><td>".repeat(10));
        body.push_str(&format!(
            "<div id='a{i}' class='c{i}'><p class='c{}'>Article {i}</p><p><span>Read</span> \
             <a href='https://news.example/{i}'>more</a></p></div>",
            (i + 1) % 60
        ));
        body.push_str(&"</td></tr></table>".repeat(10));
    }
    let html = format!(
        "<html><head><style>{css}</style></head><body><div class='footer'>{body}</div></body></html>"
    );
    let used = steps(&html);
    assert!(used < super::MATCH_BUDGET / 50, "{used} steps");
    eprintln!("synthetic newsletter: {used} matching steps");
    let mut most = 0;
    for folder in ["seed", "hostile"] {
        for path in eml_files(folder) {
            let message = parse(&std::fs::read(&path).expect("corpus file"));
            if let Some(html) = message.html.as_deref() {
                most = most.max(steps(html));
            }
        }
    }
    assert!(most < super::MATCH_BUDGET / 100, "{most} steps");
    eprintln!("corpus: at most {most} matching steps");
}
