use err_derive::Error;
use std::num::TryFromIntError;
use std::str;
use std::{io, num::ParseIntError};

/// Possible errors that arise from compressing or decompressing a `vpk0` binary
#[derive(Debug, Error)]
pub enum VpkError {
    #[error(display = "Invalid ascii string '{}' in header", _0)]
    InvalidHeader(String),
    #[error(display = "VPK method {} is invalid and not supported", _0)]
    InvalidMethod(u8),
    #[error(
        display = "Bad input file: asked to move back {} bytes in buffer of only {} bytes",
        _0,
        _1
    )]
    BadLookBack(usize, usize),
    #[error(display = "Huffman tree value couldn't be read")]
    BadTreeEncoding,
    #[error(display = "Issue parsing user-provided huffman code tree string")]
    BadUserTree(#[source] EncodeTreeParseErr),
    #[error(display = "Input file size too big to fit in 32-bit word")]
    InputTooBig(#[source] TryFromIntError),
    #[error(display = "{}", _0)]
    Utf8Error(#[source] str::Utf8Error),
    #[error(display = "{}", _0)]
    Io(#[source] io::Error),
}

/// Possible errors from parsing a user provided Huffman Tree
///
/// These errors can occur when a user passes a bad Huffman Tree to
/// the [`with_offsets`], [`with_lengths`], [`optional_offsets`], or [`optional_lengths`]
/// methods of a [`EncoderBuilder`](crate::EncoderBuilder).
///
/// [`with_offsets`]: crate::EncoderBuilder::with_offsets
/// [`with_lengths`]: crate::EncoderBuilder::with_lengths
/// [`optional_offsets`]: crate::EncoderBuilder::optional_offsets
/// [`optional_lengths`]: crate::EncoderBuilder::optional_lengths
#[derive(Debug, Error)]
pub enum EncodeTreeParseErr {
    #[error(display = "Issue parsing number in tree string at pos {}", _1)]
    LexNum(#[source] ParseIntError, usize),
    #[error(display = "Unexpected character '{}' at pos {}", _0, _1)]
    LexUnexp(char, usize),
    #[error(display = "Unexpected token '{}' at pos {}", _0, _1)]
    ParseUnexp(&'static str, usize),
    #[error(display = "Unexpected end of tokens")]
    ParseUnexpEnd,
}
