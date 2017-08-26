extern crate byteorder;

use std::io::{Read, BufReader};
use std::str;
use byteorder::{BE, ByteOrder};

fn decode<R>(buf: R) -> Vec<u8>
    where R: Read
{
    // convert Reader to BufReader
    let mut buffer = BufReader::new(buf);
    // parse the header
    let mut header = [0u8; 8];
    buffer.read_exact(&mut header).unwrap();
    let header = read_header(&header);

    // retrieve sample length?
    let mut sample_length = [0u8];
    buffer.read_exact(&mut sample_length).unwrap();

    unimplemented!();
}
/// Represents the 8 byte VPK header.
/// "vpk", "mode", u32 size
struct VPKHeader {
    /// Size of decompressed data
    size: u32,
    /// Mode number. Only 0?
    mode: u8
}

// Change to custom result type...
fn read_header(input: &[u8]) -> Option<VPKHeader> {
    if input.len() < 8 { return None }

    let name = str::from_utf8(&input[0..3]).unwrap();
    let mode = input[3] - 48;
    let size = BE::read_u32(&input[4..8]);

    if name != "vpk" { return None }

    Some(VPKHeader{mode, size})
}

fn build_table(bits: &mut BitReader) -> Vec[u8] {
    let table: Vec<u8> = Vec::new();
    let buf: Vec<u8>   = Vec::new();
    // current index
    let idx = 0;
    // final index
    let fin = 0;

    unimplemented!()
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
    }
}
