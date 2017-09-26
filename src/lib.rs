extern crate byteorder;
extern crate bitstream_io;
#[macro_use]
extern crate error_chain;
mod errors;
mod decode;

pub use decode::decode as decode;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
    }
}
