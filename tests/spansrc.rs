#![allow(clippy::uninlined_format_args)]

//! Targeted tests for `TokenStream::span_source_report` and `Span::source`.
//!
//! The report only distinguishes original spans, `Span::call_site()` and
//! `Span::mixed_site()`, and reports `Unknown` when the backend cannot
//! provide span source information. It never modifies the token stream.

#[cfg(span_locations)]
use proc_macro2::{Delimiter, Group, Ident, Literal, Punct, Spacing, TokenTree};
use proc_macro2::{Span, SpanSource, TokenSpanSource, TokenStream};
use std::str::FromStr;

fn sources(report: &proc_macro2::SpanSourceReport) -> Vec<SpanSource> {
    report.iter().map(TokenSpanSource::source).collect()
}

#[cfg(span_locations)]
#[test]
fn parsed_tokens_have_original_spans() {
    let stream = TokenStream::from_str("answer += 42").unwrap();
    let report = stream.span_source_report();
    assert_eq!(report.len(), 4);
    assert_eq!(
        sources(&report),
        [
            SpanSource::Original,
            SpanSource::Original,
            SpanSource::Original,
            SpanSource::Original,
        ]
    );
    assert!(matches!(report.tokens()[0], TokenSpanSource::Ident(_)));
    assert!(matches!(report.tokens()[1], TokenSpanSource::Punct(_)));
    assert!(matches!(report.tokens()[2], TokenSpanSource::Punct(_)));
    assert!(matches!(report.tokens()[3], TokenSpanSource::Literal(_)));
}

#[cfg(span_locations)]
#[test]
fn constructed_tokens_have_call_site_spans() {
    let mut stream = TokenStream::new();
    stream.extend([
        TokenTree::Ident(Ident::new("constructed", Span::call_site())),
        TokenTree::Punct(Punct::new('=', Spacing::Alone)),
        TokenTree::Literal(Literal::u32_unsuffixed(7)),
    ]);
    let report = stream.span_source_report();
    assert_eq!(
        sources(&report),
        [
            SpanSource::CallSite,
            SpanSource::CallSite,
            SpanSource::CallSite,
        ]
    );
}

#[cfg(span_locations)]
#[test]
fn mixed_site_is_indistinguishable_from_call_site_on_fallback() {
    // Outside a procedural macro the fallback backend represents
    // `Span::mixed_site()` identically to `Span::call_site()`. The report
    // reflects this honestly instead of pretending to distinguish them.
    let ident = Ident::new("mixed", Span::mixed_site());
    assert_eq!(ident.span().source(), SpanSource::CallSite);
    assert_eq!(Span::mixed_site().source(), Span::call_site().source());
}

#[cfg(span_locations)]
#[test]
fn two_fragments_keep_their_own_sources() {
    // Fragment parsed from source text: original spans.
    let mut combined = TokenStream::from_str("original = 1").unwrap();
    // Fragment constructed programmatically: call_site spans.
    combined.extend([
        TokenTree::Ident(Ident::new("generated", Span::call_site())),
        TokenTree::Punct(Punct::new('=', Spacing::Alone)),
    ]);

    let report = combined.span_source_report();
    assert_eq!(
        sources(&report),
        [
            SpanSource::Original,
            SpanSource::Original,
            SpanSource::Original,
            SpanSource::CallSite,
            SpanSource::CallSite,
        ]
    );
}

#[cfg(span_locations)]
#[test]
fn nested_groups_keep_their_own_sources() {
    // Inner tokens with mixed origins: one parsed, one constructed.
    let mut content = TokenStream::new();
    content.extend([TokenTree::Ident(Ident::new("made", Span::call_site()))]);
    content.extend(TokenStream::from_str("parsed_inner").unwrap());
    // Group::new gives the delimiters a call_site span.
    let group = Group::new(Delimiter::Parenthesis, content);

    let mut top = TokenStream::new();
    top.extend([TokenTree::Group(group)]);

    let report = top.span_source_report();
    assert_eq!(report.len(), 1);
    let group_report = match &report.tokens()[0] {
        TokenSpanSource::Group(group) => group,
        other => panic!("expected group, got {:?}", other),
    };
    // The group's own delimiter spans are call_site...
    assert_eq!(group_report.span(), SpanSource::CallSite);
    assert_eq!(group_report.span_open(), SpanSource::CallSite);
    assert_eq!(group_report.span_close(), SpanSource::CallSite);
    // ...while the child tokens keep their own sources.
    assert_eq!(
        sources(group_report.stream()),
        [SpanSource::CallSite, SpanSource::Original]
    );
}

#[cfg(span_locations)]
#[test]
fn parsed_nested_groups_are_original_throughout() {
    let stream = TokenStream::from_str("a (b (c)) d").unwrap();
    let report = stream.span_source_report();
    assert_eq!(report.len(), 3);
    assert_eq!(report.tokens()[0].source(), SpanSource::Original);
    assert_eq!(report.tokens()[2].source(), SpanSource::Original);

    let outer = match &report.tokens()[1] {
        TokenSpanSource::Group(group) => group,
        other => panic!("expected group, got {:?}", other),
    };
    assert_eq!(outer.span(), SpanSource::Original);
    assert_eq!(outer.span_open(), SpanSource::Original);
    assert_eq!(outer.span_close(), SpanSource::Original);
    assert_eq!(outer.stream().len(), 2);
    assert_eq!(outer.stream().tokens()[0].source(), SpanSource::Original);

    let inner = match &outer.stream().tokens()[1] {
        TokenSpanSource::Group(group) => group,
        other => panic!("expected group, got {:?}", other),
    };
    assert_eq!(inner.span(), SpanSource::Original);
    assert_eq!(sources(inner.stream()), [SpanSource::Original]);
}

#[cfg(span_locations)]
#[test]
fn call_site_is_not_confused_with_real_zero_positions() {
    // `Span::call_site()` genuinely has an empty 0..0 byte range; that is
    // its definition, not a fabricated unknown.
    let call_site = Span::call_site();
    assert_eq!(call_site.source(), SpanSource::CallSite);
    assert_eq!(call_site.byte_range(), 0..0);
    // A parsed token at the very start of a file is still `Original`.
    let stream = TokenStream::from_str("x").unwrap();
    let token = stream.into_iter().next().unwrap();
    assert_eq!(token.span().source(), SpanSource::Original);
    assert_eq!(token.span().byte_range(), 0..1);
}

#[cfg(not(span_locations))]
#[test]
fn unknown_when_backend_has_no_locations() {
    // Without the `span-locations` feature the fallback backend carries no
    // location information at all. Every source is reported as `Unknown`
    // rather than being fabricated as position 0.
    assert_eq!(Span::call_site().source(), SpanSource::Unknown);
    assert_eq!(Span::mixed_site().source(), SpanSource::Unknown);

    let stream = TokenStream::from_str("a (b)").unwrap();
    let report = stream.span_source_report();
    assert_eq!(report.len(), 2);
    assert_eq!(report.tokens()[0].source(), SpanSource::Unknown);
    let group = match &report.tokens()[1] {
        TokenSpanSource::Group(group) => group,
        other => panic!("expected group, got {:?}", other),
    };
    assert_eq!(group.span(), SpanSource::Unknown);
    assert_eq!(group.span_open(), SpanSource::Unknown);
    assert_eq!(group.span_close(), SpanSource::Unknown);
    assert_eq!(sources(group.stream()), [SpanSource::Unknown]);
}

#[test]
fn report_does_not_change_token_structure() {
    let source = "fn main() { let pair = (1, 2); }";
    let stream = TokenStream::from_str(source).unwrap();
    let tokens_before: Vec<String> = stream
        .clone()
        .into_iter()
        .map(|token| token.to_string())
        .collect();
    let display_before = stream.to_string();

    let report = stream.span_source_report();

    let tokens_after: Vec<String> = stream
        .clone()
        .into_iter()
        .map(|token| token.to_string())
        .collect();
    assert_eq!(tokens_before, tokens_after);
    assert_eq!(display_before, stream.to_string());
    // Reparsing the printed stream reproduces the same tokens.
    assert_eq!(
        TokenStream::from_str(&display_before).unwrap().to_string(),
        display_before
    );

    // One report entry per top-level token tree; groups nest their own.
    assert_eq!(report.len(), tokens_before.len());
    assert!(!report.is_empty());
    let top_level_groups = report
        .tokens()
        .iter()
        .filter(|entry| matches!(entry, TokenSpanSource::Group(_)))
        .count();
    assert_eq!(top_level_groups, 2); // `()` of main and `{}` of the body
}

#[test]
fn empty_stream_has_empty_report() {
    let report = TokenStream::new().span_source_report();
    assert!(report.is_empty());
    assert_eq!(report.len(), 0);
    assert_eq!(report.tokens(), &[]);
}
