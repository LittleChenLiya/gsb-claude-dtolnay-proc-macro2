//! Prints the span-source report of a token stream built from two
//! fragments: one parsed from source text (original spans) and one
//! constructed programmatically (`Span::call_site()` spans), including a
//! nested group whose children keep their own sources.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example spansrc --features span-locations
//! ```

use proc_macro2::{Delimiter, Group, Ident, Span, TokenSpanSource, TokenStream, TokenTree};
use std::str::FromStr;

fn print_report(report: &proc_macro2::SpanSourceReport, indent: usize) {
    for entry in report {
        let pad = " ".repeat(indent);
        match entry {
            TokenSpanSource::Group(group) => {
                println!(
                    "{}Group(span: {:?}, open: {:?}, close: {:?})",
                    pad,
                    group.span(),
                    group.span_open(),
                    group.span_close()
                );
                print_report(group.stream(), indent + 2);
            }
            TokenSpanSource::Ident(source) => println!("{}Ident => {:?}", pad, source),
            TokenSpanSource::Punct(source) => println!("{}Punct => {:?}", pad, source),
            TokenSpanSource::Literal(source) => println!("{}Literal => {:?}", pad, source),
        }
    }
}

fn main() {
    // Fragment 1: parsed from source text. Every token gets an original
    // span pointing into the parsed string.
    let mut stream = TokenStream::from_str("fn main() { let answer = 42; }").unwrap();

    // Fragment 2: constructed programmatically. Its tokens carry
    // `Span::call_site()` spans.
    stream.extend([
        TokenTree::Ident(Ident::new("generated", Span::call_site())),
        TokenTree::Group(Group::new(Delimiter::Brace, {
            let mut inner = TokenStream::new();
            inner.extend([TokenTree::Ident(Ident::new("inside", Span::call_site()))]);
            inner
        })),
    ]);

    // The report is read-only: the stream is unchanged afterwards.
    let before = stream.to_string();
    let report = stream.span_source_report();
    assert_eq!(before, stream.to_string());

    print_report(&report, 0);
}
