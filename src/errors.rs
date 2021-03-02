use std::{
    error::Error,
    fmt, io,
    num::{ParseIntError, TryFromIntError},
    str,
};

/// Possible errors that arise from compressing or decompressing a `vpk0` binary
#[derive(Debug)]
#[non_exhaustive]
pub enum VpkError {
    InvalidHeader(String),
    InvalidMethod(u8),
    BadLookBack(usize, usize),
    BadTreeEncoding,
    BadUserTree(EncodeTreeParseErr),
    InputTooBig(TryFromIntError),
    Utf8Error(str::Utf8Error),
    Io(io::Error),
}

impl fmt::Display for VpkError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            VpkError::InvalidHeader(s) => write!(f, "Invalid ascii string '{}' in header", s),
            VpkError::InvalidMethod(n) => {
                write!(f, "VPK method {} is invalid and not supported", n)
            }
            VpkError::BadLookBack(mb, size) => write!(
                f,
                "Bad input file: asked to move back {} bytes in buffer of only {} bytes",
                mb, size
            ),
            VpkError::BadTreeEncoding => write!(f, "Huffman tree value couldn't be read"),
            VpkError::BadUserTree(_) => {
                write!(f, "Issue parsing user-provided huffman code tree string")
            }
            VpkError::InputTooBig(_) => write!(f, "Input file size too big to fit in 32-bit word"),
            VpkError::Utf8Error(_) => write!(f, "Couldn't read magic bytes"),
            VpkError::Io(_) => write!(f, "IO issue"),
        }
    }
}

impl Error for VpkError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            VpkError::BadUserTree(e) => Some(e as &dyn Error),
            VpkError::InputTooBig(e) => Some(e as &dyn Error),
            VpkError::Utf8Error(e) => Some(e as &dyn Error),
            VpkError::Io(e) => Some(e as &dyn Error),
            _ => None,
        }
    }
}

impl From<EncodeTreeParseErr> for VpkError {
    fn from(e: EncodeTreeParseErr) -> Self {
        Self::BadUserTree(e)
    }
}

impl From<io::Error> for VpkError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<TryFromIntError> for VpkError {
    fn from(e: TryFromIntError) -> Self {
        Self::InputTooBig(e)
    }
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
#[derive(Debug)]
#[non_exhaustive]
pub enum EncodeTreeParseErr {
    LexNum(ParseIntError, usize),
    LexUnexp(char, usize),
    ParseUnexp(&'static str, usize),
    ParseUnexpEnd,
}

impl fmt::Display for EncodeTreeParseErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeTreeParseErr::LexNum(_, p) => {
                write!(f, "Issue parsing number in tree string at pos {}", p)
            }
            EncodeTreeParseErr::LexUnexp(c, p) => {
                write!(f, "Unexpected character '{}' at pos {}", c, p)
            }
            EncodeTreeParseErr::ParseUnexp(s, p) => {
                write!(f, "Unexpected token '{}' at pos {}", s, p)
            }
            EncodeTreeParseErr::ParseUnexpEnd => write!(f, "Unexpected end of tokens"),
        }
    }
}

impl Error for EncodeTreeParseErr {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            EncodeTreeParseErr::LexNum(e, _) => Some(e as &dyn Error),
            _ => None,
        }
    }
}
