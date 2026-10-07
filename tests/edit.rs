//! Integration tests for `proc_macro2::edit`: path-based replacement of a
//! single token tree, with preservation of group delimiters (in particular
//! `Delimiter::None`) and spans, reject-before-commit validation, and an
//! in-memory fixture that demonstrates the resulting streams.
//!
//! Run with:
//!
//! ```sh
//! cargo test --features span-locations --test edit
//! ```
#![cfg(feature = "span-locations")]

use proc_macro2::edit::{replace_token, EditError};
use proc_macro2::{Delimiter, Group, TokenStream, TokenTree};
use std::str::FromStr;

// A small in-memory fixture: the stream
//
// ```text
// before  ∅( middle  ∅( 1 )  )  after
// ```
//
// with two nested `Delimiter::None` groups whose delimiter spans come from a
// parsed anchor stream. The target literal `1` is at path `[1, 1, 0]`.
struct Fixture {
    stream: TokenStream,
    path: Vec<usize>,
    outer_open: proc_macro2::LineColumn,
    outer_close: proc_macro2::LineColumn,
    inner_open: proc_macro2::LineColumn,
    inner_close: proc_macro2::LineColumn,
}

fn build_fixture() -> Fixture {
    let anchors = TokenStream::from_str("111 222 333").unwrap();
    let mut anchors = anchors.into_iter();
    let outer_span = anchors.next().unwrap().span();
    let inner_span = anchors.next().unwrap().span();
    let side_span = anchors.next().unwrap().span();

    let target = TokenTree::Literal("1".parse().unwrap());
    let mut inner = Group::new(Delimiter::None, TokenStream::from(target));
    inner.set_span(inner_span);

    let middle = TokenTree::Ident(proc_macro2::Ident::new("middle", side_span));
    let mut outer = Group::new(
        Delimiter::None,
        TokenStream::from_iter([middle, TokenTree::Group(inner)]),
    );
    outer.set_span(outer_span);

    let before = TokenTree::Ident(proc_macro2::Ident::new("before", side_span));
    let after = TokenTree::Ident(proc_macro2::Ident::new("after", side_span));
    let stream = TokenStream::from_iter([before, TokenTree::Group(outer), after]);

    let outer = group_at(&stream, &[1]);
    let inner = group_at(&stream, &[1, 1]);
    Fixture {
        stream,
        path: vec![1, 1, 0],
        outer_open: outer.span_open().start(),
        outer_close: outer.span_close().end(),
        inner_open: inner.span_open().start(),
        inner_close: inner.span_close().end(),
    }
}

fn token_at(stream: &TokenStream, path: &[usize]) -> TokenTree {
    let mut current = stream.clone();
    let mut tt = None;
    for &index in path {
        tt = Some(current.clone().into_iter().nth(index).unwrap());
        if let TokenTree::Group(group) = tt.as_ref().unwrap() {
            current = group.stream();
        }
    }
    tt.unwrap()
}

fn group_at(stream: &TokenStream, path: &[usize]) -> Group {
    match token_at(stream, path) {
        TokenTree::Group(group) => group,
        other => panic!("expected group at {path:?}, got {other}"),
    }
}

// span-locations is the natural way to assert spans in an integration test.
// Without that feature `LineColumn` does not exist; run these under
// `cargo test --features span-locations`.

#[test]
fn two_level_none_groups_replace_literal() {
    let mut fixture = build_fixture();

    replace_token(&mut fixture.stream, &fixture.path, "999u32")
        .expect("well-formed single-token replacement at a valid path");

    // The addressed token is now the replacement literal.
    assert_eq!(
        token_at(&fixture.stream, &fixture.path).to_string(),
        "999u32"
    );

    // Both enclosing groups are still Delimiter::None: the helper must not
    // implicitly parenthesize them.
    assert_eq!(group_at(&fixture.stream, &[1]).delimiter(), Delimiter::None);
    assert_eq!(
        group_at(&fixture.stream, &[1, 1]).delimiter(),
        Delimiter::None
    );

    // Delimiter spans at both levels are byte-identical to the input.
    let outer = group_at(&fixture.stream, &[1]);
    let inner = group_at(&fixture.stream, &[1, 1]);
    assert_eq!(outer.span_open().start(), fixture.outer_open);
    assert_eq!(outer.span_close().end(), fixture.outer_close);
    assert_eq!(inner.span_open().start(), fixture.inner_open);
    assert_eq!(inner.span_close().end(), fixture.inner_close);

    // Untouched neighbors keep their own spans.
    let sides = TokenStream::from_str("111 222 333").unwrap();
    let side_span = sides.into_iter().nth(2).unwrap().span();
    let tops: Vec<TokenTree> = fixture.stream.clone().into_iter().collect();
    assert_eq!(tops[0].span().start(), side_span.start());
    assert_eq!(tops[2].span().start(), side_span.start());

    // The overall text keeps the None-group shape.
    assert_eq!(fixture.stream.to_string(), "before middle 999u32 after");
}

#[test]
fn bad_fragment_leaves_stream_untouched() {
    let mut fixture = build_fixture();
    let before = format!("{:#?}", fixture.stream);

    // Lex error: unbalanced parenthesis.
    assert!(matches!(
        replace_token(&mut fixture.stream, &fixture.path, "( nope").unwrap_err(),
        EditError::Lex,
    ));
    assert_eq!(format!("{:#?}", fixture.stream), before);

    // Empty fragment: zero token trees.
    assert!(matches!(
        replace_token(&mut fixture.stream, &fixture.path, "   ").unwrap_err(),
        EditError::NotSingleToken { len: 0 },
    ));
    assert_eq!(format!("{:#?}", fixture.stream), before);

    // Multiple token trees.
    assert!(matches!(
        replace_token(&mut fixture.stream, &fixture.path, "1 + 2").unwrap_err(),
        EditError::NotSingleToken { len: 3 },
    ));
    assert_eq!(format!("{:#?}", fixture.stream), before);

    // Target unchanged.
    assert_eq!(token_at(&fixture.stream, &fixture.path).to_string(), "1");
}

#[test]
fn out_of_bounds_path_leaves_stream_untouched() {
    let mut fixture = build_fixture();
    let before = format!("{:#?}", fixture.stream);

    // Top-level index past the end (stream length is 3).
    match replace_token(&mut fixture.stream, &[9], "2").unwrap_err() {
        EditError::PathNotFound {
            traversed,
            index,
            len,
        } => {
            assert!(traversed.is_empty());
            assert_eq!(index, 9);
            assert_eq!(len, 3);
        }
        other => panic!("expected PathNotFound, got {other:?}"),
    }
    assert_eq!(format!("{:#?}", fixture.stream), before);

    // Deep index past the end of the innermost None group (length 1).
    match replace_token(&mut fixture.stream, &[1, 1, 5], "2").unwrap_err() {
        EditError::PathNotFound {
            traversed,
            index,
            len,
        } => {
            assert_eq!(traversed, vec![1, 1]);
            assert_eq!(index, 5);
            assert_eq!(len, 1);
        }
        other => panic!("expected PathNotFound, got {other:?}"),
    }
    assert_eq!(format!("{:#?}", fixture.stream), before);

    // Descending past a leaf (the ident `middle` is not a group).
    match replace_token(&mut fixture.stream, &[1, 0, 0], "2").unwrap_err() {
        EditError::PathNotFound { traversed, .. } => assert_eq!(traversed, vec![1, 0]),
        other => panic!("expected PathNotFound, got {other:?}"),
    }
    assert_eq!(format!("{:#?}", fixture.stream), before);

    // Empty path is rejected too.
    assert!(matches!(
        replace_token(&mut fixture.stream, &[], "2").unwrap_err(),
        EditError::EmptyPath,
    ));
    assert_eq!(format!("{:#?}", fixture.stream), before);
}

#[test]
fn none_group_is_not_implicitly_bracketed_even_inside_brackets() {
    // [ a ∅( b ) c ] -> replace b; brackets survive, None stays invisible.
    let anchors = TokenStream::from_str("10 20").unwrap();
    let mut anchors = anchors.into_iter();
    let bracket_span = anchors.next().unwrap().span();
    let none_span = anchors.next().unwrap().span();

    let b = TokenTree::Ident(proc_macro2::Ident::new("b", none_span));
    let mut none_group = Group::new(Delimiter::None, TokenStream::from(b));
    none_group.set_span(none_span);
    let a = TokenTree::Ident(proc_macro2::Ident::new("a", bracket_span));
    let c = TokenTree::Ident(proc_macro2::Ident::new("c", bracket_span));
    let mut brackets = Group::new(
        Delimiter::Bracket,
        TokenStream::from_iter([a, TokenTree::Group(none_group), c]),
    );
    brackets.set_span(bracket_span);
    let mut stream = TokenStream::from(TokenTree::Group(brackets));

    replace_token(&mut stream, &[0, 1, 0], "TOK").unwrap();

    let outer = group_at(&stream, &[0]);
    assert_eq!(outer.delimiter(), Delimiter::Bracket);
    assert_eq!(outer.span_open().start(), bracket_span.start());
    let inner = group_at(&stream, &[0, 1]);
    assert_eq!(inner.delimiter(), Delimiter::None);
    assert_eq!(inner.span_open().start(), none_span.start());
    assert_eq!(outer.to_string(), "[a TOK c]");
}

#[test]
fn tokenstream_inherent_method_matches_free_function() {
    let mut stream = TokenStream::from_str("a ( b c ) d").unwrap();
    stream
        .replace_token(&[1, 0], "B")
        .expect("path [1, 0] is the ident b");
    assert_eq!(stream.to_string(), "a (B c) d");

    // The method rejects just like the free function and leaves no partial
    // edit behind.
    let err = stream.replace_token(&[1, 9], "x").unwrap_err();
    assert!(matches!(err, EditError::PathNotFound { .. }));
    assert_eq!(stream.to_string(), "a (B c) d");
}

// A tiny demonstration that prints the before/after streams. Kept as a test so
// the "fixture display" is deterministic and part of the acceptance run.
#[test]
fn fixture_demonstration() {
    let mut fixture = build_fixture();
    let input = fixture.stream.to_string();
    replace_token(&mut fixture.stream, &fixture.path, "\"v2\"").unwrap();
    let output = fixture.stream.to_string();

    assert_eq!(input, "before middle 1 after");
    assert_eq!(output, "before middle \"v2\" after");

    // The replaced token is a string literal one level inside each None group.
    assert_eq!(
        token_at(&fixture.stream, &fixture.path).to_string(),
        "\"v2\""
    );
}
