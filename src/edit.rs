//! Small edit helper for replacing a single token tree located by a path of
//! child indices.
//!
//! A *path* is a sequence of token-tree indices, interpreted from the
//! outermost stream inward. Each index except the last selects a
//! [`TokenTree::Group`] and descends into its stream; the last index selects
//! the token tree that is replaced.
//!
//! The edit is validated before the input stream is touched: the replacement
//! fragment has to lex as exactly one token tree, and every index of the path
//! has to exist. Groups that the path passes through keep their original
//! delimiter (including [`Delimiter::None`], which is never implicitly
//! parenthesized) and the span of the group delimiters, and every token tree
//! that is not replaced keeps its own span.
//!
//! [`Delimiter::None`]: crate::Delimiter::None

use crate::{Group, TokenStream, TokenTree};
use alloc::vec::Vec;
use core::fmt::{self, Display};
use core::str::FromStr;

/// Error returned when a path-based replacement cannot be performed.
///
/// As long as an error is returned, the `TokenStream` passed in for the edit is
/// left exactly as it was: no token is removed and no delimiter is introduced.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum EditError {
    /// The replacement fragment failed to lex into a token stream.
    ///
    /// This covers unbalanced delimiters and characters that are not part of
    /// the Rust token grammar.
    Lex,
    /// The replacement fragment did not consist of exactly one token tree.
    ///
    /// An empty fragment (zero token trees) and a fragment containing multiple
    /// top-level token trees are both rejected, because the path addresses
    /// exactly one token tree. If several tokens are intended, wrap them in a
    /// group inside the fragment.
    NotSingleToken {
        /// Number of top-level token trees found in the fragment.
        len: usize,
    },
    /// One of the indices of the path does not exist.
    ///
    /// This is returned both when an index is past the end of a stream and
    /// when an intermediate index addresses a token tree that is not a group
    /// and therefore cannot be descended into.
    PathNotFound {
        /// Prefix of the path that was traversed successfully.
        traversed: Vec<usize>,
        /// The index that could not be selected.
        index: usize,
        /// Number of token trees in the stream at which the index failed.
        len: usize,
    },
    /// The replacement path was empty.
    ///
    /// An empty path does not address any token tree; replacing an entire
    /// stream is not an edit this helper performs.
    EmptyPath,
}

impl Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EditError::Lex => f.write_str("replacement fragment is not a valid token stream"),
            EditError::NotSingleToken { len } => {
                write!(
                    f,
                    "replacement fragment must be exactly one token tree, but it contains {len} token trees",
                )
            }
            EditError::PathNotFound {
                traversed,
                index,
                len,
            } => {
                write!(
                    f,
                    "token path {traversed:?} + {index} is out of bounds: stream has {len} token trees",
                )
            }
            EditError::EmptyPath => f.write_str("token path must not be empty"),
        }
    }
}

impl std::error::Error for EditError {}

/// Replace one token tree of `stream`, addressed by `path`, with the single
/// token tree produced by lexing `replacement`.
///
/// The `path` is a sequence of 0-based token-tree indices interpreted from the
/// outermost stream inward. Every index except the last must select a group;
/// the last index selects the token tree being replaced. For example, in
///
/// ```text
/// a  (  b  c  )  d
/// 0   1           2
///     |-------|
///       group, stream: [b, c]
///                      0  1
/// ```
///
/// the path `[1, 0]` addresses `b`.
///
/// # Errors
///
/// Validation happens before `stream` is modified, so on error the input
/// stream is unchanged:
///
/// - [`EditError::Lex`] if `replacement` is not a well-formed token stream;
/// - [`EditError::NotSingleToken`] if `replacement` is empty or contains more
///   than one top-level token tree;
/// - [`EditError::PathNotFound`] if an index is out of bounds or an
///   intermediate index selects a non-group token tree;
/// - [`EditError::EmptyPath`] if `path` is empty.
///
/// # Preservation guarantees
///
/// On a successful replacement the groups passed through by the path retain
/// their original [`Delimiter`] (a [`Delimiter::None`] group is never given
/// visible brackets) and their delimiter spans, and every token tree other
/// than the replaced one keeps its own span and identity.
///
/// [`Delimiter`]: crate::Delimiter
/// [`Delimiter::None`]: crate::Delimiter::None
pub fn replace_token(
    stream: &mut TokenStream,
    path: &[usize],
    replacement: &str,
) -> Result<(), EditError> {
    let (first, rest) = path.split_first().ok_or(EditError::EmptyPath)?;

    // Reject before touching the stream: lex the fragment first.
    let parsed = TokenStream::from_str(replacement).map_err(|_| EditError::Lex)?;
    let mut fragment: Vec<TokenTree> = parsed.into_iter().collect();
    if fragment.len() != 1 {
        return Err(EditError::NotSingleToken {
            len: fragment.len(),
        });
    }
    let replacement = fragment.pop().unwrap();

    // Reject before touching the stream: verify the whole path exists and that
    // every intermediate index selects a group. `stream` is only borrowed and
    // cloned here, never mutated, so a failure leaves the caller's stream
    // bit-for-bit intact.
    validate_path(stream, path)?;

    // Commit the edit, rebuilding only the streams found along the path.
    *stream = rebuild_level(stream.clone(), first, rest, replacement);
    Ok(())
}

fn validate_path(stream: &TokenStream, path: &[usize]) -> Result<(), EditError> {
    let mut current = stream.clone();
    for (depth, &index) in path.iter().enumerate() {
        let tokens: Vec<TokenTree> = current.clone().into_iter().collect();
        if index >= tokens.len() {
            return Err(EditError::PathNotFound {
                traversed: path[..depth].to_vec(),
                index,
                len: tokens.len(),
            });
        }
        let last = depth == path.len() - 1;
        if !last {
            match &tokens[index] {
                TokenTree::Group(group) => current = group.stream(),
                _ => {
                    return Err(EditError::PathNotFound {
                        traversed: path[..=depth].to_vec(),
                        index: path[depth + 1],
                        len: 0,
                    });
                }
            }
        }
    }
    Ok(())
}

// Rebuilds one stream level, replacing the token tree at `(first, rest)`.
// The path is assumed to have been validated by `validate_path`.
fn rebuild_level(
    stream: TokenStream,
    first: &usize,
    rest: &[usize],
    replacement: TokenTree,
) -> TokenStream {
    let mut tokens: Vec<TokenTree> = stream.into_iter().collect();

    let new_token = match rest.split_first() {
        None => replacement,
        // `validate_path` proved that every intermediate index is a group.
        Some((next, further)) => {
            let group = match &tokens[*first] {
                TokenTree::Group(group) => group,
                _ => unreachable!("path was validated to select a group here"),
            };
            let inner = rebuild_level(group.stream(), next, further, replacement);
            // Keep the group's delimiter (None stays None) and the span of its
            // delimiters; only the inner stream changes. `Group::new` would
            // reset the delimiter span to call_site, so use the internal
            // span-preserving constructor instead.
            TokenTree::Group(Group::_with_stream(group, inner))
        }
    };

    tokens[*first] = new_token;
    tokens.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Delimiter, Group, Literal, Span, TokenTree};
    use alloc::format;
    use alloc::string::ToString as _;
    use alloc::vec;

    // Builds `before  ∅( middle  ∅( target )  )  after`, with the two
    // `Delimiter::None` groups carrying real (and distinct) source spans taken
    // from a parsed token stream. Returns the stream and the path of the
    // target literal (`[1, 1, 0]`).
    fn two_level_none_stream(target: &str) -> (TokenStream, Vec<usize>) {
        // Source spans borrowed from ordinary parsed tokens.
        let anchors: TokenStream = "111 222 333".parse().unwrap();
        let mut anchors = anchors.into_iter();
        let outer_span: Span = anchors.next().unwrap().span();
        let inner_span: Span = anchors.next().unwrap().span();
        let before_span: Span = anchors.next().unwrap().span();

        let target_lit: TokenTree = Literal::from_str(target)
            .map(TokenTree::Literal)
            .unwrap_or_else(|_| panic!("literal {target:?} must parse"));
        let mut inner = Group::new(Delimiter::None, TokenStream::from(target_lit));
        inner.set_span(inner_span);

        let middle: TokenTree = TokenTree::Ident(crate::Ident::new("middle", before_span));
        let inner_tt = TokenTree::Group(inner);
        let mut outer = Group::new(Delimiter::None, TokenStream::from_iter([middle, inner_tt]));
        outer.set_span(outer_span);

        let before = TokenTree::Ident(crate::Ident::new("before", before_span));
        let after = TokenTree::Ident(crate::Ident::new("after", before_span));
        let stream = TokenStream::from_iter([before, TokenTree::Group(outer), after]);
        (stream, vec![1, 1, 0])
    }

    // Walks `path` inside `stream`, asserting each intermediate group is a
    // `Delimiter::None` group. Returns the addressed token tree.
    fn get<'a>(stream: &'a TokenStream, path: &[usize]) -> TokenTree {
        let mut tokens: Vec<TokenTree> = stream.clone().into_iter().collect();
        for (depth, index) in path.iter().enumerate() {
            let tt = tokens.remove(*index);
            if depth == path.len() - 1 {
                return tt;
            }
            match tt {
                TokenTree::Group(group) => {
                    assert_eq!(
                        group.delimiter(),
                        Delimiter::None,
                        "group at {} must remain Delimiter::None",
                        depth,
                    );
                    tokens = group.stream().into_iter().collect();
                }
                other => panic!("expected group at {depth}, found {other:?}"),
            }
        }
        unreachable!()
    }

    #[test]
    fn replaces_literal_through_two_none_groups() {
        let (mut stream, path) = two_level_none_stream("1");

        // Record delimiter spans before the edit.
        #[cfg(span_locations)]
        let spans_before = {
            let outer = get(&stream, &path[..1]);
            let inner = get(&stream, &path[..2]);
            let (outer, inner) = match (outer, inner) {
                (TokenTree::Group(o), TokenTree::Group(i)) => (o, i),
                _ => panic!("both levels must be groups"),
            };
            (
                (outer.span_open().start(), outer.span_close().end()),
                (inner.span_open().start(), inner.span_close().end()),
            )
        };

        replace_token(&mut stream, &path, "999u32").expect("valid edit must succeed");

        // The addressed token is the replacement literal.
        let replaced = get(&stream, &path);
        assert_eq!(replaced.to_string(), "999u32");

        // None groups stay None at both levels; `get` asserts the delimiter.
        let _ = get(&stream, &path);

        // Delimiter spans of both groups are unchanged.
        #[cfg(span_locations)]
        {
            let outer = get(&stream, &path[..1]);
            let inner = get(&stream, &path[..2]);
            let (outer, inner) = match (outer, inner) {
                (TokenTree::Group(o), TokenTree::Group(i)) => (o, i),
                _ => panic!("both levels must still be groups"),
            };
            assert_eq!(
                (outer.span_open().start(), outer.span_close().end()),
                spans_before.0,
                "outer None-group span changed",
            );
            assert_eq!(
                (inner.span_open().start(), inner.span_close().end()),
                spans_before.1,
                "inner None-group span changed",
            );

            // The untouched surrounding tokens keep their spans too.
            let tokens: Vec<TokenTree> = stream.clone().into_iter().collect();
            let anchors: TokenStream = "111 222 333".parse().unwrap();
            let before_span = anchors.into_iter().nth(2).unwrap().span();
            assert_eq!(tokens[0].span().start(), before_span.start());
            assert_eq!(tokens[2].span().start(), before_span.start());
        }

        // The textual shape still reads as the None-group contents.
        assert_eq!(stream.to_string(), "before middle 999u32 after");
    }

    #[test]
    fn invalid_fragment_is_rejected_before_commit() {
        let (mut stream, path) = two_level_none_stream("1");
        let snapshot = stream.to_string();
        let debug_snapshot = format!("{stream:?}");

        // Unbalanced delimiters: lex error.
        let err = replace_token(&mut stream, &path, "( unclosed").unwrap_err();
        assert!(matches!(err, EditError::Lex));
        assert_eq!(stream.to_string(), snapshot);
        assert_eq!(format!("{stream:?}"), debug_snapshot);

        // Zero token trees.
        let err = replace_token(&mut stream, &path, "   ").unwrap_err();
        assert!(matches!(err, EditError::NotSingleToken { len: 0 }));
        assert_eq!(stream.to_string(), snapshot);

        // Two token trees.
        let err = replace_token(&mut stream, &path, "1 2").unwrap_err();
        assert!(matches!(err, EditError::NotSingleToken { len: 2 }));
        assert_eq!(stream.to_string(), snapshot);
        assert_eq!(format!("{stream:?}"), debug_snapshot);

        // The original target is still the original literal.
        assert_eq!(get(&stream, &path).to_string(), "1");
    }

    #[test]
    fn out_of_bounds_path_is_rejected_before_commit() {
        let (mut stream, path) = two_level_none_stream("1");
        let debug_snapshot = format!("{stream:?}");

        // First level out of bounds: only 3 top-level token trees.
        let err = replace_token(&mut stream, &[7], "2").unwrap_err();
        match err {
            EditError::PathNotFound {
                traversed,
                index,
                len,
            } => {
                assert!(traversed.is_empty());
                assert_eq!(index, 7);
                assert_eq!(len, 3);
            }
            other => panic!("expected PathNotFound, got {other:?}"),
        }
        assert_eq!(format!("{stream:?}"), debug_snapshot);

        // Inner level out of bounds: the inner None group holds one token.
        let mut deep = path.clone();
        deep[2] = 4;
        let err = replace_token(&mut stream, &deep, "2").unwrap_err();
        match err {
            EditError::PathNotFound {
                traversed,
                index,
                len,
            } => {
                assert_eq!(traversed, &[1, 1]);
                assert_eq!(index, 4);
                assert_eq!(len, 1);
            }
            other => panic!("expected PathNotFound, got {other:?}"),
        }
        assert_eq!(format!("{stream:?}"), debug_snapshot);

        // Descending into a non-group: path [1, 0, 0] targets `middle`.
        let err = replace_token(&mut stream, &[1, 0, 0], "2").unwrap_err();
        assert!(matches!(
            err,
            EditError::PathNotFound {
                traversed,
                ..
            } if traversed == [1, 0]
        ));
        assert_eq!(format!("{stream:?}"), debug_snapshot);

        // Empty path.
        let err = replace_token(&mut stream, &[], "2").unwrap_err();
        assert!(matches!(err, EditError::EmptyPath));
        assert_eq!(format!("{stream:?}"), debug_snapshot);
    }

    #[test]
    #[cfg(span_locations)]
    fn visible_delimiters_are_preserved_through_none_and_elsewhere() {
        // [ a  ∅( b )  c ]  : the outer bracket group must keep its brackets,
        // and the nested None group its invisibility, after replacing `b`.
        let anchors: TokenStream = "10 20".parse().unwrap();
        let mut anchors = anchors.into_iter();
        let bracket_span = anchors.next().unwrap().span();
        let none_span = anchors.next().unwrap().span();

        let b = TokenTree::Ident(crate::Ident::new("b", none_span));
        let mut none_group = Group::new(Delimiter::None, TokenStream::from(b));
        none_group.set_span(none_span);
        let a = TokenTree::Ident(crate::Ident::new("a", bracket_span));
        let c = TokenTree::Ident(crate::Ident::new("c", bracket_span));
        let mut bracket = Group::new(
            Delimiter::Bracket,
            TokenStream::from_iter([a, TokenTree::Group(none_group), c]),
        );
        bracket.set_span(bracket_span);

        let mut stream = TokenStream::from(TokenTree::Group(bracket));
        replace_token(&mut stream, &[0, 1, 0], "TOKEN").unwrap();

        let tokens: Vec<TokenTree> = stream.into_iter().collect();
        let outer = match &tokens[0] {
            TokenTree::Group(g) => g,
            other => panic!("expected group, got {other:?}"),
        };
        assert_eq!(outer.delimiter(), Delimiter::Bracket);
        assert_eq!(outer.span().start(), bracket_span.start());
        let inner_tokens: Vec<TokenTree> = outer.stream().into_iter().collect();
        let inner = match &inner_tokens[1] {
            TokenTree::Group(g) => g,
            other => panic!("expected group, got {other:?}"),
        };
        assert_eq!(inner.delimiter(), Delimiter::None);
        assert_eq!(inner.span().start(), none_span.start());
        let replaced: Vec<TokenTree> = inner.stream().into_iter().collect();
        assert_eq!(replaced[0].to_string(), "TOKEN");
        assert_eq!(outer.to_string(), "[a TOKEN c]");
    }
}
