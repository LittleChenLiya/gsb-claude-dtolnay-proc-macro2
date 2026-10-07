//! Local tests for the path-based token-tree replacement helper
//! (proc_macro2::edit, also reachable as TokenStream::replace_at).

use proc_macro2::edit::{self, EditError};
use proc_macro2::{Delimiter, Group, Literal, Span, TokenStream, TokenTree};
use std::str::FromStr;

/// Collects the token trees of a stream into a Vec for indexing.
fn trees(stream: &TokenStream) -> Vec<TokenTree> {
    stream.clone().into_iter().collect()
}

/// Builds outer( inner( 1 ) ) 2 where both groups use Delimiter::None.
fn two_none_layers() -> TokenStream {
    let one = TokenTree::Literal(Literal::u64_unsuffixed(1));
    let inner = Group::new(Delimiter::None, TokenStream::from(one));
    let outer = Group::new(Delimiter::None, TokenStream::from(TokenTree::Group(inner)));
    let two = TokenTree::Literal(Literal::u64_unsuffixed(2));
    TokenStream::from_iter([TokenTree::Group(outer), two])
}

#[test]
fn replaces_literal_through_two_none_groups() {
    proc_macro2::fallback::force();

    let input = two_none_layers();
    let before = format!("{:?}", input);

    // Path: top-level index 0 (outer None group), then index 0 (inner None
    // group), then index 0 (the literal 1).
    let edited = edit::replace_at(input.clone(), &[0, 0, 0], "99").unwrap();

    // The literal at the addressed path is replaced.
    let outer = match &trees(&edited)[0] {
        TokenTree::Group(g) => g.clone(),
        other => panic!("expected group, got {other:?}"),
    };
    let inner = match &trees(&outer.stream())[0] {
        TokenTree::Group(g) => g.clone(),
        other => panic!("expected group, got {other:?}"),
    };
    let replaced = &trees(&inner.stream())[0];
    assert_eq!(replaced.to_string(), "99");

    // Both traversed groups are still None-delimited: nothing was implicitly
    // wrapped in parentheses.
    assert_eq!(outer.delimiter(), Delimiter::None);
    assert_eq!(inner.delimiter(), Delimiter::None);

    // The untouched trailing literal survives.
    assert_eq!(trees(&edited).len(), 2);
    assert_eq!(trees(&edited)[1].to_string(), "2");

    // None groups do not print delimiters, so the whole stream round-trips to
    // the expected sequence.
    assert_eq!(edited.to_string(), "99 2");

    // The input handed to the helper is the original stream.
    assert_eq!(format!("{:?}", input), before);

    proc_macro2::fallback::unforce();
}

#[test]
fn none_groups_keep_delimiter_spans() {
    proc_macro2::fallback::force();

    // Parse real source so that token positions are observable, then wrap the
    // middle token in two nested None groups like macro expansion would.
    let source = TokenStream::from_str("a 1 b").unwrap();
    let parts = trees(&source);
    let wrapped = Group::new(
        Delimiter::None,
        TokenStream::from(TokenTree::Group(Group::new(
            Delimiter::None,
            TokenStream::from(parts[1].clone()),
        ))),
    );
    let input = TokenStream::from_iter([
        parts[0].clone(),
        TokenTree::Group(wrapped),
        parts[2].clone(),
    ]);

    let outer_before = match &trees(&input)[1] {
        TokenTree::Group(g) => g.clone(),
        _ => panic!(),
    };
    let inner_before = match &trees(&outer_before.stream())[0] {
        TokenTree::Group(g) => g.clone(),
        _ => panic!(),
    };

    let edited = edit::replace_at(input.clone(), &[1, 0, 0], "7").unwrap();

    let outer_after = match &trees(&edited)[1] {
        TokenTree::Group(g) => g.clone(),
        _ => panic!(),
    };
    let inner_after = match &trees(&outer_after.stream())[0] {
        TokenTree::Group(g) => g.clone(),
        _ => panic!(),
    };

    assert_eq!(outer_after.delimiter(), Delimiter::None);
    assert_eq!(inner_after.delimiter(), Delimiter::None);
    assert_spans_equal(outer_before.span(), outer_after.span());
    assert_spans_equal(inner_before.span(), inner_after.span());
    assert_spans_equal(outer_before.span_open(), outer_after.span_open());
    assert_spans_equal(outer_before.span_close(), outer_after.span_close());
    assert_spans_equal(inner_before.span_open(), inner_after.span_open());
    assert_spans_equal(inner_before.span_close(), inner_after.span_close());

    // Sibling token trees keep their spans.
    assert_spans_equal(trees(&input)[0].span(), trees(&edited)[0].span());
    assert_spans_equal(trees(&input)[2].span(), trees(&edited)[2].span());

    proc_macro2::fallback::unforce();
}

#[cfg(feature = "span-locations")]
fn assert_spans_equal(a: Span, b: Span) {
    assert_eq!(a.start(), b.start());
    assert_eq!(a.end(), b.end());
}

#[cfg(not(feature = "span-locations"))]
fn assert_spans_equal(_a: Span, _b: Span) {
    // Without span-locations there is nothing position-wise to compare; the
    // delimiter-kind assertions in the caller still cover the no-implicit-
    // parentheses guarantee.
}

#[cfg(feature = "span-locations")]
#[test]
fn untouched_tokens_keep_exact_source_spans() {
    use proc_macro2::LineColumn;

    proc_macro2::fallback::force();

    // Source positions: a at col 0, 1 at col 2, b at col 4.
    let source = TokenStream::from_str("a 1 b").unwrap();
    let parts = trees(&source);

    let inner = Group::new(Delimiter::None, TokenStream::from(parts[1].clone()));
    let outer = Group::new(Delimiter::None, TokenStream::from(TokenTree::Group(inner)));
    let input =
        TokenStream::from_iter([parts[0].clone(), TokenTree::Group(outer), parts[2].clone()]);

    let edited = edit::replace_at(input.clone(), &[1, 0, 0], "777").unwrap();
    let edited_parts = trees(&edited);

    // The untouched `a` and `b` keep their exact line/column and source text.
    assert_eq!(
        edited_parts[0].span().start(),
        LineColumn { line: 1, column: 0 }
    );
    assert_eq!(
        edited_parts[0].span().end(),
        LineColumn { line: 1, column: 1 }
    );
    assert_eq!(edited_parts[0].span().source_text().as_deref(), Some("a"));
    assert_eq!(
        edited_parts[2].span().start(),
        LineColumn { line: 1, column: 4 }
    );
    assert_eq!(edited_parts[2].span().source_text().as_deref(), Some("b"));

    // The replaced token occupies the new fragment, not the old span.
    let outer = match &edited_parts[1] {
        TokenTree::Group(g) => g.clone(),
        _ => panic!(),
    };
    let inner = match &trees(&outer.stream())[0] {
        TokenTree::Group(g) => g.clone(),
        _ => panic!(),
    };
    let replaced = inner.stream();
    assert_eq!(replaced.to_string(), "777");
    assert_ne!(
        replaced.clone().into_iter().next().unwrap().span().start(),
        LineColumn { line: 1, column: 2 }
    );

    proc_macro2::fallback::unforce();
}

#[test]
fn bad_fragment_is_rejected_before_commit() {
    proc_macro2::fallback::force();

    let input = two_none_layers();
    let snapshot = format!("{:?}", input);

    // Unbalanced delimiters: lexing fails.
    let err = edit::replace_at(input.clone(), &[0, 0, 0], "( 1").unwrap_err();
    assert!(matches!(
        err.1,
        EditError::InvalidFragment { ref fragment } if fragment == "( 1"
    ));
    // The original stream comes back untouched.
    assert_eq!(format!("{:?}", err.0), snapshot);
    assert_eq!(err.0.to_string(), "1 2");

    // Whitespace only: parses, but to zero token trees.
    let err = edit::replace_at(input.clone(), &[0, 0, 0], "   ").unwrap_err();
    assert!(matches!(err.1, EditError::InvalidFragment { .. }));
    assert_eq!(format!("{:?}", err.0), snapshot);

    proc_macro2::fallback::unforce();
}

#[test]
fn out_of_bounds_path_is_rejected_before_commit() {
    proc_macro2::fallback::force();

    let input = two_none_layers();
    let snapshot = format!("{:?}", input);

    // Top level only has two token trees.
    let err = edit::replace_at(input.clone(), &[5], "9").unwrap_err();
    assert_eq!(
        err.1,
        EditError::IndexOutOfBounds {
            prefix: vec![],
            index: 5,
            len: 2,
        }
    );
    assert_eq!(format!("{:?}", err.0), snapshot);

    // The inner None group contains exactly one token tree.
    let err = edit::replace_at(input.clone(), &[0, 0, 3], "9").unwrap_err();
    assert_eq!(
        err.1,
        EditError::IndexOutOfBounds {
            prefix: vec![0, 0],
            index: 3,
            len: 1,
        }
    );
    assert_eq!(format!("{:?}", err.0), snapshot);

    // Index 1 at top level is the bare literal 2, not a group, so it can not
    // be descended into.
    let err = edit::replace_at(input.clone(), &[1, 0], "9").unwrap_err();
    assert_eq!(err.1, EditError::NotAGroup { prefix: vec![1] });
    assert_eq!(format!("{:?}", err.0), snapshot);

    proc_macro2::fallback::unforce();
}

#[test]
fn inherent_method_validates_then_commits_and_rolls_back() {
    proc_macro2::fallback::force();

    let original = two_none_layers();
    let snapshot = stringify_snapshot(&original);

    // A valid edit through the inherent method changes the stream in place.
    let mut stream = original.clone();
    stream.replace_at(&[0, 0, 0], "77").unwrap();
    assert_eq!(stream.to_string(), "77 2");

    // A failed edit leaves the receiver holding its pre-call value.
    let mut stream = original.clone();
    let error = stream.replace_at(&[0, 0, 9], "9").unwrap_err();
    assert!(matches!(error, EditError::IndexOutOfBounds { .. }));
    assert_eq!(stringify_snapshot(&stream), snapshot);
    assert_eq!(stream.to_string(), "1 2");

    proc_macro2::fallback::unforce();
}

fn stringify_snapshot(stream: &TokenStream) -> String {
    format!("{stream:?}")
}

#[test]
fn multi_token_fragment_splices_all_its_tokens() {
    proc_macro2::fallback::force();

    let input = TokenStream::from_str("a b c").unwrap();
    let edited = edit::replace_at(input, &[1], "x + y").unwrap();
    assert_eq!(edited.to_string(), "a x + y c");
    assert_eq!(trees(&edited).len(), 5);

    proc_macro2::fallback::unforce();
}

#[test]
fn visible_delimiter_is_preserved_along_the_path() {
    proc_macro2::fallback::force();

    // [ { 1 } ]: path [0, 0, 0] crosses Bracket then Brace.
    let brace = Group::new(
        Delimiter::Brace,
        TokenStream::from(TokenTree::Literal(Literal::u64_unsuffixed(1))),
    );
    let bracket = Group::new(
        Delimiter::Bracket,
        TokenStream::from(TokenTree::Group(brace)),
    );
    let input = TokenStream::from(TokenTree::Group(bracket));

    let edited = edit::replace_at(input, &[0, 0, 0], "2").unwrap();
    let outer = match &trees(&edited)[0] {
        TokenTree::Group(g) => g.clone(),
        _ => panic!(),
    };
    assert_eq!(outer.delimiter(), Delimiter::Bracket);
    let inner = match &trees(&outer.stream())[0] {
        TokenTree::Group(g) => g.clone(),
        _ => panic!(),
    };
    assert_eq!(inner.delimiter(), Delimiter::Brace);
    assert_eq!(inner.stream().to_string(), "2");
    assert_eq!(edited.to_string(), "[{ 2 }]");

    proc_macro2::fallback::unforce();
}
