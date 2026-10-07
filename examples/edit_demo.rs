//! In-memory acceptance demo for `proc_macro2::edit`.
//!
//! It builds the two-level `Delimiter::None` fixture entirely in memory (no
//! files, no temp directories), performs a literal replacement through both
//! invisible groups, then demonstrates that an invalid fragment and an
//! out-of-bounds path are rejected without touching the input stream.
//!
//! Run with:
//!
//! ```sh
//! cargo run --features span-locations --example edit_demo
//! ```

use proc_macro2::edit::replace_token;
use proc_macro2::{Delimiter, Group, TokenStream, TokenTree};
use std::str::FromStr;

fn main() {
    // Fixture:  before  ∅( middle  ∅( 1 )  )  after
    //                       outer   inner  ^
    // target literal `1` is at path [1, 1, 0].
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
    let mut stream = TokenStream::from_iter([before, TokenTree::Group(outer), after]);

    println!("input        : {stream}");

    // Successful edit: both None groups keep their delimiter and spans.
    replace_token(&mut stream, &[1, 1, 0], "999u32").unwrap();
    println!("replaced     : {stream}");

    // Bad fragment (unbalanced delimiter): rejected, stream unchanged.
    let err = replace_token(&mut stream, &[1, 1, 0], "( oops").unwrap_err();
    println!("bad fragment : {err} -> stream stays: {stream}");

    // Bad path (out of bounds): rejected, stream unchanged.
    let err = replace_token(&mut stream, &[1, 1, 9], "0").unwrap_err();
    println!("bad path     : {err} -> stream stays: {stream}");

    assert_eq!(stream.to_string(), "before middle 999u32 after");
    println!("OK");
}
