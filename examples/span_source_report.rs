//! Prints the span source report of a small token stream.
//!
//! The report classifies every token's span as coming from original source
//! text, `Span::call_site()`, or `Span::mixed_site()`, without modifying the
//! token stream. Nested groups keep the source attribution of their own
//! tokens.
//!
//! Run with span tracking enabled for full attribution:
//!
//! ```sh
//! cargo run --example span_source_report --features span-locations
//! ```

use proc_macro2::{
    Delimiter, Group, Ident, Span, SpanSource, TokenSpanSource, TokenStream, TokenTree,
};

fn main() {
    // Two fragments parsed from source text: their spans are `Original`.
    let mut stream: TokenStream = "let answer =".parse().unwrap();
    let rhs: TokenStream = "42".parse().unwrap();
    stream.extend(rhs);

    // Constructed tokens carry call-site and mixed-site spans.
    stream.extend([
        TokenTree::Ident(Ident::new("call_site_tok", Span::call_site())),
        TokenTree::Ident(Ident::new("mixed_site_tok", Span::mixed_site())),
    ]);

    // A nested group keeps the source attribution of its own tokens; the
    // group's delimiter span is attributed independently.
    let mut group: Group = Group::new(Delimiter::Brace, "parsed_inner".parse().unwrap());
    group.set_span(Span::mixed_site());
    stream.extend([TokenTree::Group(group)]);

    let before = stream.to_string();
    let report = stream.span_source_report();

    println!("token stream: {stream}");
    println!();
    for (i, token) in report.tokens.iter().enumerate() {
        let location = match token.location() {
            Some(location) => format!("{}:{}", location.line, location.column),
            None => "unknown".to_owned(),
        };
        println!("token {i}: source={:?} location={location}", token.source());
        if let TokenSpanSource::Group(group) = token {
            for (j, child) in group.tokens.tokens.iter().enumerate() {
                println!(
                    "  child {j} (inside {:?} group): source={:?}",
                    group.delimiter,
                    child.source()
                );
            }
        }
    }
    println!();
    println!("{report:#?}");

    // The report is read-only: the token stream is unchanged.
    assert_eq!(stream.to_string(), before);

    if cfg!(span_locations) {
        assert_eq!(report.tokens[0].source(), SpanSource::Original);
        assert!(report.tokens[0].location().is_some());
    } else {
        println!(
            "note: built without the `span-locations` feature, so the backend \
             cannot provide span provenance; every source is reported as \
             Unknown and no location is fabricated."
        );
        assert_eq!(report.tokens[0].source(), SpanSource::Unknown);
        assert_eq!(report.tokens[0].location(), None);
    }
}
