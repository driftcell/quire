#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

pub mod common;
use common::*;

#[test]
fn style_and_config_rules_are_detected() {
    let mut css = doc();
    css.theme.custom_css = ".a { color: red;".into();
    let report = lint_document(&css);
    assert!(!finding(&report, "QR501").suppressible);

    let mut external = doc();
    external.blocks.push(custom_html(
        "<img src=\"https://example.com/a.png\">",
        "Image",
    ));
    assert!(has_code(&lint_document(&external), "QR502"));

    let mut overflow = doc();
    overflow.blocks.push(custom_html(
        "<div style=\"width: 960px\">wide</div>",
        "Wide card",
    ));
    assert!(has_code(&lint_document(&overflow), "QR503"));

    let mut duplicate_title = doc();
    duplicate_title
        .blocks
        .push(custom_html("<h2>KPI Grid</h2><div>18%</div>", "18%"));
    assert!(has_code(&lint_document(&duplicate_title), "QR504"));

    let mut fallback = doc();
    fallback.blocks.push(custom_html("<div>x</div>", ""));
    assert!(has_code(&lint_document(&fallback), "QR505"));

    let mut empty_html = doc();
    empty_html.blocks.push(custom_html("", "Fallback"));
    assert!(has_code(&lint_document(&empty_html), "QR506"));

    let mut token_name = doc();
    token_name
        .theme
        .tokens
        .insert("color".into(), "#fff".into());
    assert!(has_code(&lint_document(&token_name), "QR507"));

    let mut token_value = doc();
    token_value
        .theme
        .tokens
        .insert("--x".into(), "#fff; body{}".into());
    assert!(has_code(&lint_document(&token_value), "QR508"));

    let mut preset = doc();
    preset.theme.preset = String::new();
    assert!(has_code(&lint_document(&preset), "QR509"));

    let mut toc = doc();
    toc.toc.max_depth = 5;
    assert!(has_code(&lint_document(&toc), "QR510"));

    let mut toc_title = doc();
    toc_title.toc.title = String::new();
    assert!(has_code(&lint_document(&toc_title), "QR511"));

    let mut cover_title = doc();
    cover_title.cover.enabled = true;
    cover_title.cover.title = String::new();
    assert!(has_code(&lint_document(&cover_title), "QR512"));

    let mut cover_variant = doc();
    cover_variant.cover.enabled = true;
    cover_variant.cover.title = "Report".into();
    cover_variant.cover.variant = String::new();
    assert!(has_code(&lint_document(&cover_variant), "QR513"));
}
