extern crate byteorder;
extern crate bitstream_io;
#[macro_use] extern crate failure;
#[macro_use] extern crate log;

mod errors;
mod decode;

pub use decode::decode as decode;
pub use errors::VpkError as VpkError;
