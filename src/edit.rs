//! Small path-based editing helper for [`TokenStream`]s.
//!
//! This module provides a constrained, transactional way of replacing a single
//! subtree of a token stream: the replacement is addressed by a *path* of
//! token-tree indices, where every index except the last one is required to
//! point at a [`Group`] whose enclosed stream is entered.
//!
//! The operation is validated before it is committed:
//!
//! - the replacement fragment must be a syntactically valid, nonempty token
//!   stream;
//! - every index of the path must exist, and every intermediate index must
//!   select a [`Group`].
//!
//! If either check fails, [`replace_at`] (and [`TokenStream::replace_at`])
//! return an [`EditError`] and leave the input stream exactly as it was. In
//! particular, the input stream is never partially rewritten.
//!
//! While descending into groups the delimiter kind and the delimiter span of
//! every traversed group are preserved. A [`crate::Delimiter::None`] group is
//! rebuilt with `Delimiter::None`; it is never implicitly re-parenthesized.
//! Token trees that are not targeted by the path are cloned with their
//! original spans.

use crate::Group;
use crate::TokenStream;
use crate::TokenTree;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::{self, Display};
use core::iter;
use core::str::FromStr;
use std::error::Error;

/// Error returned by [`TokenStream::replace_at`] and [`replace_at`] when an
/// edit cannot be performed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditError {
    /// The replacement fragment could not be parsed as Rust tokens, or parsed
    /// to zero token trees. The carried string is the fragment that was
    /// rejected.
    InvalidFragment {
        /// The fragment string that failed to parse or was empty.
        fragment: String,
    },

    /// The path addressed a token-tree index that does not exist. The carried
    /// index is the first index of the path that was out of bounds.
    IndexOutOfBounds {
        /// The path prefix that was valid before the bad index.
        prefix: Vec<usize>,
        /// The index that could not be resolved.
        index: usize,
        /// The number of token trees available at that level.
        len: usize,
    },

    /// An intermediate path index selected a token tree that is not a
    /// [`Group`], so the path could not be descended any further.
    NotAGroup {
        /// The path prefix up to and including the offending index.
        prefix: Vec<usize>,
    },
}

impl Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EditError::InvalidFragment { fragment } => write!(
                f,
                "invalid replacement fragment {fragment:?}: expected a parseable, nonempty token stream",
            ),
            EditError::IndexOutOfBounds { prefix, index, len } => {
                write!(
                    f,
                    "token path {:?} does not exist: index {index} is out of bounds for a stream of {len} token trees",
                    prefix_with(prefix, *index),
                )
            }
            EditError::NotAGroup { prefix } => write!(
                f,
                "token path {:?} does not exist: intermediate token tree is not a group",
                prefix,
            ),
        }
    }
}

fn prefix_with(prefix: &[usize], index: usize) -> Vec<usize> {
    let mut full = prefix.to_vec();
    full.push(index);
    full
}

impl Error for EditError {}

/// Parses the replacement fragment ahead of touching the input.
///
/// Keeping this separate from the traversal is what makes the operation
/// transactional: a bad fragment is rejected before the input stream is
/// rebuilt in any way.
fn parse_fragment(fragment: &str) -> Result<TokenStream, EditError> {
    let parsed = TokenStream::from_str(fragment).map_err(|_| EditError::InvalidFragment {
        fragment: String::from(fragment),
    })?;
    if parsed.is_empty() {
        return Err(EditError::InvalidFragment {
            fragment: String::from(fragment),
        });
    }
    Ok(parsed)
}

/// Walks `path` through `tokens` and returns an error describing the first
/// index that cannot be resolved, without modifying anything.
fn locate(tokens: &TokenStream, path: &[usize]) -> Result<(), EditError> {
    if path.is_empty() {
        return Err(EditError::IndexOutOfBounds {
            prefix: Vec::new(),
            index: 0,
            len: tokens.clone().into_iter().count(),
        });
    }

    let mut current = tokens.clone();
    let mut current_len = current.clone().into_iter().count();
    let mut prefix = Vec::new();

    for (depth, &index) in path.iter().enumerate() {
        if index >= current_len {
            return Err(EditError::IndexOutOfBounds {
                prefix,
                index,
                len: current_len,
            });
        }

        // The last index does not need to lead anywhere; every earlier index
        // must select a group whose stream the path continues into.
        if depth + 1 == path.len() {
            break;
        }

        let selected = current.into_iter().nth(index).unwrap();
        match selected {
            TokenTree::Group(group) => {
                prefix.push(index);
                current = group.stream();
                current_len = current.clone().into_iter().count();
            }
            _ => {
                prefix.push(index);
                return Err(EditError::NotAGroup { prefix });
            }
        }
    }

    Ok(())
}

/// Rebuilds `tokens`, recursing through the group addressed by `path` and
/// splicing `replacement` where the path ends.
///
/// Untouched token trees are passed through unchanged, so their spans are
/// preserved bit-for-bit. Traversed groups are reconstructed with their
/// original delimiter and delimiter span; in particular a
/// `Delimiter::None` group stays invisible instead of gaining parentheses.
fn replace_in_stream(
    tokens: TokenStream,
    path: &[usize],
    replacement: &TokenStream,
) -> TokenStream {
    let (&index, rest) = path.split_first().expect("locate() rejects empty paths");

    let mut output = TokenStream::new();
    for (i, tree) in tokens.into_iter().enumerate() {
        if i != index {
            // Untouched token tree: keeps its original span verbatim.
            output.extend(iter::once(tree));
            continue;
        }

        if rest.is_empty() {
            // Splice the fragment in place of the targeted tree. Each token of
            // the fragment keeps the span it was parsed/constructed with.
            output.extend(replacement.clone());
            continue;
        }

        match tree {
            TokenTree::Group(group) => {
                let rebuilt = rebuild_group(group, rest, replacement);
                output.extend(iter::once(TokenTree::Group(rebuilt)));
            }
            // locate() proved that intermediate indices always select groups.
            other => output.extend(iter::once(other)),
        }
    }
    output
}

/// Recreates a traversed group around its edited contents, holding fast to its
/// original delimiter kind and delimiter span.
fn rebuild_group(group: Group, rest: &[usize], replacement: &TokenStream) -> Group {
    let delimiter = group.delimiter();
    let delim_span = group.delim_span();
    let inner = replace_in_stream(group.stream(), rest, replacement);

    // Rebuild with the original delimiter kind so that Delimiter::None is not
    // implicitly parenthesized...
    let mut rebuilt = Group::new(delimiter, inner);
    // ...and restore the original span of the delimiters themselves.
    rebuilt.set_span(delim_span.join());
    rebuilt
}

/// Replaces the token tree at `path` in `tokens` with the token trees parsed
/// from `replacement`.
///
/// `path` is a sequence of token-tree indices starting at the top level of the
/// stream; every index except the last must point at a [`Group`], which is
/// entered. For example, in `a (b c) d` the path `[1, 0]` addresses the `b`
/// inside the parentheses.
///
/// On success the rewritten stream is returned. Both the delimiters and the
/// delimiter spans of every group that the path passes through are preserved
/// (a [`crate::Delimiter::None`] group is never implicitly parenthesized), and
/// every token tree outside the addressed subtree keeps its span.
///
/// On failure the original `tokens` is returned untouched alongside the
/// [`EditError`]: an unparseable or empty fragment and a nonexistent path are
/// both rejected before anything is committed.
pub fn replace_at(
    tokens: TokenStream,
    path: &[usize],
    replacement: &str,
) -> Result<TokenStream, (TokenStream, EditError)> {
    let parsed = match parse_fragment(replacement) {
        Ok(parsed) => parsed,
        Err(error) => return Err((tokens, error)),
    };

    if let Err(error) = locate(&tokens, path) {
        return Err((tokens, error));
    }

    Ok(replace_in_stream(tokens, path, &parsed))
}
