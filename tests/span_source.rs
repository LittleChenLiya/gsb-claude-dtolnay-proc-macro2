//! Targeted tests for `TokenStream::span_source_report`.
//!
//! Run with span tracking for the full attribution tests:
//!
//! ```sh
//! cargo test --features span-locations --test span_source
//! ```

// The set of exercised APIs differs between feature configurations.
#![allow(dead_code, unused_imports)]

use proc_macro2::{
    Delimiter, Group, Ident, Literal, Punct, Spacing, Span, SpanSource, TokenSpanSource,
    TokenStream, TokenTree,
};

fn sources(report: &proc_macro2::SpanSourceReport) -> Vec<SpanSource> {
    report.tokens.iter().map(TokenSpanSource::source).collect()
}

// Two source fragments and a nested group: every token is attributed to the
// fragment or constructor it actually came from.
#[cfg(span_locations)]
#[test]
fn two_fragments_and_constructed_tokens_are_attributed() {
    // First source fragment.
    let mut stream: TokenStream = "alpha beta".parse().unwrap();
    // Second, independently parsed source fragment.
    let second: TokenStream = "gamma".parse().unwrap();
    stream.extend(second);
    // Constructed tokens: call-site and mixed-site spans.
    stream.extend([
        TokenTree::Ident(Ident::new("call", Span::call_site())),
        TokenTree::Ident(Ident::new("mixed", Span::mixed_site())),
        TokenTree::Punct(Punct::new('+', Spacing::Alone)),
        TokenTree::Literal(Literal::u32_unsuffixed(7)),
    ]);

    let before = stream.to_string();
    let report = stream.span_source_report();
    // Generating the report does not modify the token stream.
    assert_eq!(stream.to_string(), before);

    assert_eq!(
        sources(&report),
        [
            SpanSource::Original,
            SpanSource::Original,
            SpanSource::Original,
            SpanSource::CallSite,
            SpanSource::MixedSite,
            SpanSource::CallSite,
            SpanSource::CallSite,
        ]
    );

    // Real locations for the parsed tokens of both fragments. Each fragment
    // is its own file, so `gamma` starts again at line 1, column 0.
    let location = report.tokens[0].location().expect("alpha has a location");
    assert_eq!((location.line, location.column), (1, 0));
    let location = report.tokens[1].location().expect("beta has a location");
    assert_eq!((location.line, location.column), (1, 6));
    let location = report.tokens[2].location().expect("gamma has a location");
    assert_eq!((location.line, location.column), (1, 0));

    // Constructed spans have no real location; it must not be faked as 0:0.
    for token in &report.tokens[3..] {
        assert_eq!(token.location(), None);
    }
}

// Nested groups keep the source attribution of their own tokens, and the
// group's own delimiter span is attributed independently.
#[cfg(span_locations)]
#[test]
fn nested_groups_keep_their_own_sources() {
    let inner_parsed: TokenStream = "inner_tok".parse().unwrap();

    let mut call_tokens = TokenStream::new();
    call_tokens.extend([TokenTree::Ident(Ident::new("call_tok", Span::call_site()))]);

    // Group with the default (call-site) delimiter span around parsed tokens.
    let parsed_group = Group::new(Delimiter::Brace, inner_parsed);

    // Group whose delimiter span is set to mixed-site around call-site tokens.
    let mut mixed_group = Group::new(Delimiter::Parenthesis, call_tokens);
    mixed_group.set_span(Span::mixed_site());

    // Deepest nesting: a none-delimited group around the mixed-site group.
    let nested = Group::new(
        Delimiter::None,
        TokenStream::from(TokenTree::Group(mixed_group)),
    );

    let mut stream = TokenStream::new();
    stream.extend([TokenTree::Group(parsed_group), TokenTree::Group(nested)]);

    let before = stream.to_string();
    let report = stream.span_source_report();
    assert_eq!(stream.to_string(), before);

    assert_eq!(report.tokens.len(), 2);

    let TokenSpanSource::Group(first) = &report.tokens[0] else {
        panic!("expected a group");
    };
    assert_eq!(first.delimiter, Delimiter::Brace);
    assert_eq!(first.source, SpanSource::CallSite);
    assert_eq!(first.location, None);
    assert_eq!(first.tokens.tokens.len(), 1);
    assert_eq!(first.tokens.tokens[0].source(), SpanSource::Original);
    assert!(first.tokens.tokens[0].location().is_some());

    let TokenSpanSource::Group(none_group) = &report.tokens[1] else {
        panic!("expected a group");
    };
    assert_eq!(none_group.delimiter, Delimiter::None);
    assert_eq!(none_group.source, SpanSource::CallSite);
    assert_eq!(none_group.tokens.tokens.len(), 1);

    let TokenSpanSource::Group(mixed) = &none_group.tokens.tokens[0] else {
        panic!("expected a group");
    };
    assert_eq!(mixed.delimiter, Delimiter::Parenthesis);
    assert_eq!(mixed.source, SpanSource::MixedSite);
    assert_eq!(mixed.location, None);
    assert_eq!(mixed.tokens.tokens.len(), 1);
    assert_eq!(mixed, &mixed.clone()); // report entries are value types
    assert_eq!(mixed.tokens.tokens[0].source(), SpanSource::CallSite);
}

// The report mirrors the token tree structure and computing it leaves the
// stream untouched, regardless of feature configuration.
#[test]
fn report_preserves_token_structure() {
    let mut stream: TokenStream = "fn main() { let x = 1; }".parse().unwrap();
    stream.extend([TokenTree::Ident(Ident::new("generated", Span::call_site()))]);

    let before = stream.to_string();
    let report = stream.span_source_report();
    let after = stream.to_string();
    assert_eq!(before, after);

    // Same number of top-level token trees.
    assert_eq!(report.tokens.len(), stream.clone().into_iter().count());

    // The brace group reports exactly the tokens inside the group.
    let TokenSpanSource::Group(group) = &report.tokens[3] else {
        panic!("expected a group");
    };
    let Some(TokenTree::Group(original)) = stream.clone().into_iter().nth(3) else {
        panic!("expected a group");
    };
    assert_eq!(group.delimiter, original.delimiter());
    assert_eq!(
        group.tokens.tokens.len(),
        original.stream().into_iter().count()
    );
}

// Without `span-locations` the fallback backend records no provenance: the
// source is explicitly `Unknown` and no location is fabricated as 0:0.
#[cfg(not(span_locations))]
#[test]
fn unknown_source_is_not_reported_as_zero_location() {
    let stream: TokenStream = "alpha".parse().unwrap();
    let report = stream.span_source_report();
    assert_eq!(report.tokens.len(), 1);
    assert_eq!(report.tokens[0].source(), SpanSource::Unknown);
    assert_eq!(report.tokens[0].location(), None);
}

// With `span-locations`, constructed spans are attributed correctly but still
// have no real location; it is reported as unknown, not as 0:0.
#[cfg(span_locations)]
#[test]
fn constructed_spans_have_no_fabricated_location() {
    let stream: TokenStream = [
        TokenTree::Ident(Ident::new("call", Span::call_site())),
        TokenTree::Ident(Ident::new("mixed", Span::mixed_site())),
    ]
    .into_iter()
    .collect();
    let report = stream.span_source_report();
    assert_eq!(report.tokens[0].source(), SpanSource::CallSite);
    assert_eq!(report.tokens[1].source(), SpanSource::MixedSite);
    assert_eq!(report.tokens[0].location(), None);
    assert_eq!(report.tokens[1].location(), None);
}

// An empty stream reports no tokens.
#[test]
fn empty_stream_reports_no_tokens() {
    let report = TokenStream::new().span_source_report();
    assert!(report.is_empty());
    assert_eq!(report.tokens.len(), 0);
}
