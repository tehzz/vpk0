use std::io;
use std::str;

/// Possible errors that arise from attempting to convert a vpk0 binary into its
/// decompressed data, or vise-versa.
#[derive(Fail, Debug)]
pub enum VpkError {
    #[fail(display = "Invalid header for vpk file")]
    InvalidHeader,

    #[fail(display = "VPK method {} is invalid and not supported", _0)]
    InvalidMethod(u8),

    #[fail(display = "Input was bad; check log for more info")]
    BadInput,
    
    #[fail(display = "{}", _0)]
    Utf8Error(#[cause] str::Utf8Error),

    #[fail(display = "{}", _0)]
    Io(#[cause] io::Error),
}

impl From<io::Error> for VpkError {
    fn from(error: io::Error) -> Self {
        VpkError::Io(error)
    }
}

impl From<str::Utf8Error> for VpkError {
    fn from(error: str::Utf8Error) -> Self {
        VpkError::Utf8Error(error)
    }
}
