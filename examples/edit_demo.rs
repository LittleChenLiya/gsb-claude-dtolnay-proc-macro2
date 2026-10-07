//! Tiny in-memory fixture demonstrating the path-based token-tree editor
//! (proc_macro2::edit and TokenStream::replace_at).
//!
//! Run with:
//!     cargo run --example edit_demo
//! or with span locations:
//!     cargo run --example edit_demo --features span-locations

use proc_macro2::edit;
use proc_macro2::{Delimiter, Group, Literal, TokenStream, TokenTree};

fn trees(stream: &TokenStream) -> Vec<TokenTree> {
    stream.clone().into_iter().collect()
}

fn describe(stream: &TokenStream) -> String {
    fn level(stream: &TokenStream, out: &mut String, depth: usize) {
        for tree in trees(stream) {
            match tree {
                TokenTree::Group(group) => {
                    let kind = match group.delimiter() {
                        Delimiter::Parenthesis => "Parenthesis",
                        Delimiter::Brace => "Brace",
                        Delimiter::Bracket => "Bracket",
                        Delimiter::None => "None",
                    };
                    let span = span_label(group.span());
                    let indent = "  ".repeat(depth);
                    let _ = writeln_indent(out, &indent, &format!("Group({kind}){span}"));
                    level(&group.stream(), out, depth + 1);
                }
                other => {
                    let indent = "  ".repeat(depth);
                    let _ =
                        writeln_indent(out, &indent, &format!("{}: {}", variant(&other), other));
                }
            }
        }
    }

    #[cfg(feature = "span-locations")]
    fn span_label(span: proc_macro2::Span) -> String {
        format!(
            " [{}:{}..{}:{}]",
            span.start().line,
            span.start().column,
            span.end().line,
            span.end().column,
        )
    }

    #[cfg(not(feature = "span-locations"))]
    fn span_label(_span: proc_macro2::Span) -> String {
        String::new()
    }

    fn writeln_indent(out: &mut String, indent: &str, text: &str) -> std::fmt::Result {
        use std::fmt::Write as _;
        writeln!(out, "{indent}{text}")
    }

    fn variant(tree: &TokenTree) -> &str {
        match tree {
            TokenTree::Group(_) => "Group",
            TokenTree::Ident(_) => "Ident",
            TokenTree::Punct(_) => "Punct",
            TokenTree::Literal(_) => "Literal",
        }
    }

    let mut out = String::new();
    level(stream, &mut out, 0);
    out
}

fn main() {
    proc_macro2::fallback::force();

    // Fixture: two nested None groups around the literal 1, followed by 2.
    // Stream: (None (None 1)) 2
    let one = TokenTree::Literal(Literal::u64_unsuffixed(1));
    let inner = Group::new(Delimiter::None, TokenStream::from(one));
    let outer = Group::new(Delimiter::None, TokenStream::from(TokenTree::Group(inner)));
    let two = TokenTree::Literal(Literal::u64_unsuffixed(2));
    let input = TokenStream::from_iter([TokenTree::Group(outer), two]);

    println!("== input ==\n{}rendered: {input}", describe(&input));

    // 1. Replace the literal through the two None-delimited layers.
    let edited = edit::replace_at(input.clone(), &[0, 0, 0], "99").expect("valid edit");
    println!(
        "== after path [0,0,0] -> \"99\" ==\n{}rendered: {edited}",
        describe(&edited)
    );

    // 2. A bad fragment is rejected; the original stream comes back untouched.
    match edit::replace_at(input.clone(), &[0, 0, 0], "( unbalanced") {
        Ok(_) => panic!("unexpected success"),
        Err((untouched, error)) => {
            println!(
                "== bad fragment rejected ==\nerror: {error}\nrendered (unchanged): {untouched}"
            );
            assert_eq!(untouched.to_string(), input.to_string());
        }
    }

    // 3. An out-of-range path is rejected; the original stream is preserved.
    match edit::replace_at(input.clone(), &[0, 0, 7], "9") {
        Ok(_) => panic!("unexpected success"),
        Err((untouched, error)) => {
            println!("== bad path rejected ==\nerror: {error}\nrendered (unchanged): {untouched}");
            assert_eq!(untouched.to_string(), input.to_string());
        }
    }

    proc_macro2::fallback::unforce();
}
