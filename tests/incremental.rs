#![allow(clippy::assertions_on_result_states, clippy::uninlined_format_args)]

use proc_macro2::{IncrementalParser, PartialParse, TokenStream, TokenTree};
use std::fs;
use std::path::PathBuf;

fn residual_text(parser: &IncrementalParser, partial: &PartialParse) -> String {
    parser.source()[partial.residual()].to_owned()
}

// Three short inputs, each split into two chunks with the cut landing inside
// a block comment, a string literal, and a doc comment respectively.
const SPLIT_CASES: &[(&[&str], &str, &str)] = &[
    // (chunks, expected completed tokens after first chunk, expected residual)
    (&["alpha /* co", "mment */ beta"], "alpha", "/* co"),
    (&["let s = \"hel", "lo world\";"], "let s =", "\"hel"),
    (&["/// tit", "le\nx"], "", "/// tit"),
];

#[test]
fn split_inside_comment_and_string() {
    for &(chunks, expected_partial_tokens, expected_residual) in SPLIT_CASES {
        let source = chunks.concat();
        let one_shot = source.parse::<TokenStream>().unwrap();

        let mut parser = IncrementalParser::new();
        parser.push(chunks[0]);

        // The first chunk ends inside a comment or string literal: parsing
        // fails, reporting the completed top-level tokens and the range of
        // the uncompleted original text.
        let partial = parser.parse().unwrap_err();
        assert!(partial.is_incomplete(), "not incomplete: {:?}", chunks);
        assert_eq!(partial.tokens().to_string(), expected_partial_tokens);
        assert_eq!(residual_text(&parser, &partial), expected_residual);

        // After the second chunk the input parses along the ordinary
        // FromStr path, identical to the one-shot result.
        parser.push(chunks[1]);
        let stream = parser.parse().unwrap();
        assert_eq!(stream.to_string(), one_shot.to_string());
        assert_eq!(parser.finish().unwrap().to_string(), one_shot.to_string());
    }
}

#[test]
fn unclosed_group_at_eof() {
    let mut parser = IncrementalParser::new();
    parser.push("foo(bar");

    let partial = parser.parse().unwrap_err();
    assert!(partial.is_incomplete());
    assert_eq!(residual_text(&parser, &partial), "(bar");

    // The half-received group is not treated as a complete Group: only the
    // ident before it is reported as a completed top-level token.
    let tokens = partial.tokens().clone().into_iter().collect::<Vec<_>>();
    assert_eq!(tokens.len(), 1, "tokens: {:?}", tokens);
    match &tokens[0] {
        TokenTree::Ident(ident) => assert_eq!(ident.to_string(), "foo"),
        other => panic!("expected Ident, got {:?}", other),
    }

    // The one-shot API rejects the same input, and so does finish().
    assert!("foo(bar".parse::<TokenStream>().is_err());
    assert!(parser.finish().is_err());

    // Pushing the rest completes the group along the ordinary FromStr path.
    parser.push(")");
    let stream = parser.parse().unwrap();
    assert_eq!(stream.to_string(), "foo (bar)");
    assert_eq!(
        stream.to_string(),
        "foo(bar)".parse::<TokenStream>().unwrap().to_string()
    );
}

#[test]
fn genuine_error_is_not_incomplete() {
    let mut parser = IncrementalParser::new();
    parser.push("a )");

    let partial = parser.parse().unwrap_err();
    assert!(!partial.is_incomplete());
    assert_eq!(partial.tokens().to_string(), "a");
    assert_eq!(residual_text(&parser, &partial), ")");
    assert!(parser.finish().is_err());
}

// Small temp-file fixture: each case is materialized as chunk files on disk,
// read back, and fed through the incremental parser.
#[test]
fn temp_file_fixture() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("incremental-fixture");
    fs::create_dir_all(&dir).unwrap();

    for (i, &(chunks, _, _)) in SPLIT_CASES.iter().enumerate() {
        let case_dir = dir.join(format!("case{}", i));
        fs::create_dir_all(&case_dir).unwrap();

        let mut parser = IncrementalParser::new();
        let mut stream = None;
        for (j, chunk) in chunks.iter().enumerate() {
            let path = case_dir.join(format!("chunk{}.txt", j));
            fs::write(&path, chunk).unwrap();
            parser.push(&fs::read_to_string(&path).unwrap());
            match parser.parse() {
                Ok(done) => {
                    println!("case{} chunk{}: complete => {:?}", i, j, done.to_string());
                    stream = Some(done);
                }
                Err(partial) => {
                    let residual = &parser.source()[partial.residual()];
                    println!(
                        "case{} chunk{}: partial => tokens={:?} residual={:?}",
                        i,
                        j,
                        partial.tokens().to_string(),
                        residual,
                    );
                }
            }
        }

        let one_shot = chunks.concat().parse::<TokenStream>().unwrap();
        assert_eq!(stream.unwrap().to_string(), one_shot.to_string());
    }

    fs::remove_dir_all(&dir).unwrap();
}
