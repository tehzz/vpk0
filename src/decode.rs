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
    let mut header = [0u8; 9];
    bit_reader.read_bytes(&mut header)?;
    let header = VpkHeader::from_array(&header)?;

    // retrieve sample length?
    let sample_length = header.method;
    println!("sample_length: {:?}", sample_length);
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
            let p = match sample_length {
                VpkMethod::TwoSample => {
                    let mut l = 0;

                    if u < 3 {
                        l = u + 1;
                        u = tbl_select(&mut bit_reader, &movetree)? as usize;
                    }
                    output.len() - (u << 2) - l + 8
                },
                VpkMethod::OneSample => {
                    output.len() - u
                },
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
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum VpkMethod {
    TwoSample,
    OneSample
}
/// Represents the 8 byte VPK header.
/// "vpk", "mode", u32 size
#[derive(Debug)]
struct VpkHeader {
    /// Size of decompressed data
    size: u32,
    /// Mode number. Only 0?
    mode: u8,
    /// Sample length
    method: VpkMethod,
}
impl VpkHeader {
    /// Create a VPK0 header from an byte array
    fn from_array(arr: &[u8; 9]) -> Result<Self, VpkError> {
        let name = str::from_utf8(&arr[0..3])?;
        if name != "vpk" { return Err(VpkError::InvalidHeader) }
        // mode is encoded as ascii number
        let mode = arr[3] - 48;
        if mode != 0 { return Err(VpkError::UnsupportedMode(mode)) }

        let size = BE::read_u32(&arr[4..8]);
        let method = match arr[8] {
            0 => VpkMethod::OneSample,
            1 => VpkMethod::TwoSample,
            err @ _ => return Err(VpkError::InvalidMethod(err))
        };

        Ok( Self{mode, size, method} )
    }
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
