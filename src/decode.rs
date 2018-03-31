use errors::{VpkError};

use std::io::{Read};
use std::str;
use byteorder::{BE, ByteOrder};
use bitstream_io::{BE as bBE, BitReader};

/// Decode a Reader of vpk0 data into a Vec of the decompressed data
pub fn decode<R>(mut buf: R) -> Result<Vec<u8>, VpkError>
    where R: Read
{
    // convert Reader to BitReader
    let mut bit_reader = BitReader::<bBE>::new(&mut buf);
    // parse the header
    let mut header = [0u8; 8];
    bit_reader.read_bytes(&mut header)?;
    let header = parse_header(&header)?;

    // currently only work with "mode 0" files (are there other modes?)
    if header.mode != 0 { 
        return Err(VpkError::UnsupportedMode(header.mode))
    }

    // retrieve sample length?
    let sample_length: u8 = bit_reader.read(8)?;
    println!("sample_length: {}", sample_length);
    // build first huffman tree
    let movetree = build_table(&mut bit_reader)?;
    // build second huffman tree
    let sizetree = build_table(&mut bit_reader)?;

    // finally, start decoding the input buffer
    let output_size = header.size as usize;
    let mut output: Vec<u8> = Vec::with_capacity(output_size);

    while output.len() < output_size {
        if bit_reader.read_bit()? {
            // copy bytes from output
            let mut u = tbl_select(&mut bit_reader, &movetree)? as usize;
            let p = if sample_length > 0 {
                // two-sample backtrack lengths
                let mut l = 0;

                if u < 3 {
                    l = u + 1;
                    u = tbl_select(&mut bit_reader, &movetree)? as usize;
                }
                output.len() - (u << 2) - l + 8
            } else {
                // one-sample backtrack lengths
                output.len() - u
            };
            // have position in output, now grab length of bytes to copy
            let n = tbl_select(&mut bit_reader, &sizetree)? as usize;
            // append bytes from output to output?
            
            for i in p..p+n {
                let byte = output[i];
                output.push(byte);
            }

        } else {
            // push next byte to output
            output.push(bit_reader.read(8)?);
        }
    }

    Ok(output)
}
/// Represents the 8 byte VPK header.
/// "vpk", "mode", u32 size
#[allow(dead_code)]
struct VPKHeader {
    /// Size of decompressed data
    size: u32,
    /// Mode number. Only 0?
    mode: u8
}

///This functions checks for a proper vpk0 header, and if valid, parses the header
fn parse_header(input: &[u8]) -> Result<VPKHeader, VpkError> {
    if input.len() < 8 { return Err(VpkError::InvalidHeader) }

    let name = str::from_utf8(&input[0..3])?;
    let mode = input[3] - 48;
    let size = BE::read_u32(&input[4..8]);

    if name != "vpk" { return Err(VpkError::InvalidHeader) }

    Ok(VPKHeader{mode, size})
}

/// A Huffman table entry?
struct TBLentry {
    /// left? (0)
    left: u32,
    /// right? (1)
    right: u32,
    value: Option<u8>,
}

///Build a Huffman table?
fn build_table(bits: &mut BitReader<bBE>) -> Result<Vec<TBLentry>, VpkError> 
{
    let mut table: Vec<TBLentry> = Vec::new();
    let mut buf: Vec<u32>   = Vec::new();
    // current index
    let mut idx = 0;
    // final index
    let mut fin = 0;

    // main loop?
    loop {
        // read one bit from the stream
        if bits.read_bit()? {
            if idx < 2 {
                break;
            }
            // a node in the tree
            table.push(TBLentry{
                left: buf[idx-2],
                right: buf[idx-1],
                value: None
            });
            buf[idx-2] = fin;
            fin += 1;
            idx -= 1;
        } else {
            // leaf on the tree
            let v: u8 = bits.read(8)?;

            table.push(TBLentry{
                left: 0,
                right: 0,
                value: Some(v),
            });

            if buf.len() <= idx {
                buf.push(fin);
            } else {
                buf[idx] = fin;
            }

            fin += 1;
            idx += 1;
        }
    }

    Ok(table)
}

// Find "non-reference" entry in the table? Return the width of that entry?
fn tbl_select(bits: &mut BitReader<bBE>, tbl: &[TBLentry]) -> Result<u32, VpkError>
{
    // start at final entry
    let len = tbl.len();
    if len == 0 { return Ok(0) };

    let mut idx = len - 1;

    // loop from end of the table to the beginning;
    while tbl[idx].value.is_none() {
        if bits.read_bit()? {
            idx = tbl[idx].right as usize;
        } else {
            idx = tbl[idx].left as usize;
        }
    }

    let output: u32 = bits.read(tbl[idx].value.unwrap() as u32)?;
    Ok(output)
}
