//! Targeted tests for the structural two-fragment splice entry points:
//! `TokenStream::splice` and the `Add` impl.
//!
//! These tests construct fragments out of token trees directly; no token
//! stream is ever rendered to a string and parsed back. They check that:
//!   1. two `Alone` `>` puncts meeting at the splice boundary do not collapse
//!      into a joint `>>`,
//!   2. the original joint `>>` stays structurally `Joint`,
//!   3. a nested group keeps its delimiter and full inner stream through a
//!      splice.

use proc_macro2::{Delimiter, Group, Punct, Spacing, TokenStream, TokenTree};

// ---------------------------------------------------------------------------
// Three small fragment fixtures.
// ---------------------------------------------------------------------------

// Fragment 1: a single standalone `>` punct.
fn alone_gt() -> TokenStream {
    let mut stream = TokenStream::new();
    stream.extend([TokenTree::Punct(Punct::new('>', Spacing::Alone))]);
    stream
}

// Fragment 2: another single standalone `>` punct, identical in structure.
fn alone_gt_second() -> TokenStream {
    let mut stream = TokenStream::new();
    stream.extend([TokenTree::Punct(Punct::new('>', Spacing::Alone))]);
    stream
}

// Fragment 3: a group `(>> { nested })` containing a joint `>>` punct and a
// nested brace group whose own stream holds an ident.
fn nested_group() -> TokenStream {
    let mut braces_inner = TokenStream::new();
    braces_inner.extend([TokenTree::Ident(proc_macro2::Ident::new(
        "nested",
        proc_macro2::Span::call_site(),
    ))]);
    let braces = Group::new(Delimiter::Brace, braces_inner);

    let mut parens_inner = TokenStream::new();
    parens_inner.extend([
        TokenTree::Punct(Punct::new('>', Spacing::Joint)),
        TokenTree::Punct(Punct::new('>', Spacing::Alone)),
        TokenTree::Group(braces),
    ]);
    let parens = Group::new(Delimiter::Parenthesis, parens_inner);

    let mut stream = TokenStream::new();
    stream.extend([TokenTree::Group(parens)]);
    stream
}

fn punct_shape(stream: &TokenStream) -> Vec<(char, Spacing)> {
    stream
        .clone()
        .into_iter()
        .filter_map(|tt| match tt {
            TokenTree::Punct(punct) => Some((punct.as_char(), punct.spacing())),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Boundary: two Alone `>` must not turn into a joint `>>`.
// ---------------------------------------------------------------------------

#[test]
fn splice_two_alone_gt_remains_two_alone_puncts() {
    let mut spliced = alone_gt();
    spliced.splice(alone_gt_second());

    let trees: Vec<TokenTree> = spliced.clone().into_iter().collect();
    assert_eq!(trees.len(), 2, "splice must keep both token trees");
    assert_eq!(
        punct_shape(&spliced),
        vec![('>', Spacing::Alone), ('>', Spacing::Alone)],
        "both boundary puncts must remain Alone",
    );

    // The Display rendering separates Alone puncts, whereas a joint `>>`
    // would not. This is the externally observable structural distinction.
    assert_eq!(spliced.to_string(), "> >");
}

#[test]
fn add_operator_matches_splice_for_alone_gt() {
    let spliced = alone_gt() + alone_gt_second();
    assert_eq!(
        punct_shape(&spliced),
        vec![('>', Spacing::Alone), ('>', Spacing::Alone)],
    );
    assert_eq!(spliced.to_string(), "> >");
}

// ---------------------------------------------------------------------------
// Structural contrast with a genuine joint `>>`.
// ---------------------------------------------------------------------------

#[test]
fn alone_pair_is_structurally_distinct_from_joint_double_gt() {
    // Reference: a genuine two-character joint punct `>>`.
    let mut joint = TokenStream::new();
    joint.extend([
        TokenTree::Punct(Punct::new('>', Spacing::Joint)),
        TokenTree::Punct(Punct::new('>', Spacing::Alone)),
    ]);

    let mut split = alone_gt();
    split.splice(alone_gt_second());

    assert_ne!(
        punct_shape(&split),
        punct_shape(&joint),
        "Alone + Alone across the boundary differs from Joint + Alone",
    );
    assert_eq!(
        punct_shape(&joint),
        vec![('>', Spacing::Joint), ('>', Spacing::Alone)],
    );

    // Same characters, different spacing structure and rendering.
    assert_eq!(split.to_string(), "> >");
    assert_eq!(joint.to_string(), ">>");
}

// ---------------------------------------------------------------------------
// A nested group passes through the splice unchanged.
// ---------------------------------------------------------------------------

#[test]
fn splice_preserves_nested_group_and_its_delimiter() {
    let group_before = nested_group();

    // Splice the group fragment between the two standalone `>` fragments so
    // that the group sits at a non-edge position in the resulting stream.
    let mut spliced = alone_gt();
    spliced.splice(group_before);
    spliced.splice(alone_gt_second());

    let trees: Vec<TokenTree> = spliced.clone().into_iter().collect();
    assert_eq!(trees.len(), 3, "expected: > ( >> {{ nested }} ) >");

    let group = match &trees[1] {
        TokenTree::Group(group) => group,
        other => panic!("expected middle token tree to be a Group, got {other:?}"),
    };
    assert_eq!(group.delimiter(), Delimiter::Parenthesis);

    // Inner stream: Joint `>`, Alone `>`, brace Group, exactly as built.
    let inner: Vec<TokenTree> = group.stream().into_iter().collect();
    assert_eq!(inner.len(), 3);
    assert!(matches!(
        &inner[0],
        TokenTree::Punct(punct)
            if punct.as_char() == '>' && punct.spacing() == Spacing::Joint
    ));
    assert!(matches!(
        &inner[1],
        TokenTree::Punct(punct)
            if punct.as_char() == '>' && punct.spacing() == Spacing::Alone
    ));
    let nested = match &inner[2] {
        TokenTree::Group(group) => group,
        other => panic!("expected nested brace Group, got {other:?}"),
    };
    assert_eq!(nested.delimiter(), Delimiter::Brace);
    let nested_trees: Vec<TokenTree> = nested.stream().into_iter().collect();
    assert_eq!(nested_trees.len(), 1);
    assert!(matches!(&nested_trees[0], TokenTree::Ident(ident) if ident == "nested"));

    // Round-trip shape comparison against a fresh, untouched fixture.
    assert_eq!(spliced.to_string(), "> (>> { nested }) >");
    let fresh = {
        let mut s = alone_gt();
        s.splice(nested_group());
        s.splice(alone_gt_second());
        s
    };
    assert_eq!(spliced.to_string(), fresh.to_string());
}

#[test]
fn append_empty_fragments_is_a_no_op() {
    let mut stream = nested_group();
    let before = stream.to_string();
    stream.splice(TokenStream::new());
    assert_eq!(stream.to_string(), before);

    let mut empty = TokenStream::new();
    empty.splice(nested_group());
    assert_eq!(empty.to_string(), before);
}
