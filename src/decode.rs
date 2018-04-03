use errors::{VpkError};

use std::io::{Read};
use std::fmt::Write;
use std::str;
use byteorder::{BE, ByteOrder};
use bitstream_io::{BE as bBE, BitReader};
use log::Level::{*};

/// Decode a Reader of vpk0 data into a Vec of the decompressed data
pub fn decode<R>(mut buf: R) -> Result<Vec<u8>, VpkError>
    where R: Read
{
    let mut vpk0_bits = BitReader::<bBE>::new(&mut buf);

    let header = VpkHeader::from_bitreader(&mut vpk0_bits)?;
    info!("\n**** vpk0 header ****\n{:?}", header);
    // read huffman trees from beginning of compressed input 
    let movetree = build_table(&mut vpk0_bits)?;
    let sizetree = build_table(&mut vpk0_bits)?;
    if log_enabled!(Info) {
        let mut mt = String::new();
        let mut st = String::new();
        print_huffman_table(&movetree, movetree.len() - 1, &mut mt);
        print_huffman_table(&sizetree, sizetree.len() - 1, &mut st);
        info!("\n**** Move Tree ****\n{}", mt);
        info!("\n**** Size Tree ****\n{}", st);
    }

    // start decoding the compressed input buffer
    let output_size = header.size as usize;
    let mut output: Vec<u8> = Vec::with_capacity(output_size);

    while output.len() < output_size {
        if vpk0_bits.read_bit()? {
            // copy bytes from inside the output back at the end of the output
            let initial_move = tbl_select(&mut vpk0_bits, &movetree)? as usize;
            let move_back    = match header.method {
                VpkMethod::TwoSample => {
                    if initial_move < 3 {
                        let l = initial_move + 1;
                        let u = tbl_select(&mut vpk0_bits, &movetree)? as usize;
                        (l + (u << 2)) - 8
                    } else {
                        (initial_move << 2) - 8
                    }
                },
                VpkMethod::OneSample => {
                    initial_move
                },
            };

            // get start position in output, and the number of bytes to copy-back
            if move_back > output.len() {
                error!("Bad input file: asked to copy back bytes from outside of decoded output buffer");
                error!("move back: {} | output length: {}", move_back, output.len());
                return Err(VpkError::BadInput)
            }
            let p = output.len() - move_back;
            let n = tbl_select(&mut vpk0_bits, &sizetree)? as usize;
            debug!("start: {} | size: {} | length: {}", p, n, output.len());
            
            // append bytes from somewhere in output to the end of output
            // this needs to be done byte-by-byte, as the range can include 
            // newly added bytes 
            for i in p..p+n {
                let byte = output[i];
                trace!("\t{}: {}", i, byte);
                output.push(byte);
            }

        } else {
            // push next byte from compressed input to output
            let byte = vpk0_bits.read(8)?;
            trace!("{}", byte);
            output.push(byte);
        }
    }

    Ok(output)
}
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum VpkMethod {
    OneSample,
    TwoSample,
}
/// Represents the important info contained within the VPK0 header
#[derive(Debug)]
struct VpkHeader {
    /// Size of decompressed data
    size: u32,
    /// Sample length
    method: VpkMethod,
}
impl VpkHeader {
    /// Create a VPK0 header from an byte array
    fn from_array(arr: &[u8; 9]) -> Result<Self, VpkError> {
        let name = str::from_utf8(&arr[0..4])?;
        if name != "vpk0" { return Err(VpkError::InvalidHeader) }

        let size = BE::read_u32(&arr[4..8]);
        let method = match arr[8] {
            0 => VpkMethod::OneSample,
            1 => VpkMethod::TwoSample,
            err @ _ => return Err(VpkError::InvalidMethod(err))
        };

        Ok( Self{size, method} )
    }
    /// Convenience function to read the vpk0 header from a bitstream
    fn from_bitreader(reader: &mut BitReader<bBE>) -> Result<Self, VpkError> {
        let mut header = [0u8; 9];
        reader.read_bytes(&mut header)?;

        Self::from_array(&header)
    }
}

/// A Huffman tree node or leaf designed to be made in an array
struct TBLentry {
    // left and right are offsets into the array  
    left: usize,
    right: usize,
    // if None, entry is a node; if Some, entry is a leaf
    value: Option<u8>,
}

///Build a Huffman table?
fn build_table(bits: &mut BitReader<bBE>) -> Result<Vec<TBLentry>, VpkError> 
{
    let mut table: Vec<TBLentry> = Vec::new();
    let mut buf: Vec<usize>      = Vec::new();
    // current and final index
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
            idx = tbl[idx].right;
        } else {
            idx = tbl[idx].left;
        }
    }

    let output: u32 = bits.read(tbl[idx].value.unwrap() as u32)?;
    Ok(output)
}

/// Print a Huffman tree for debugging purposes. Each node is represented by a
/// set of `(left, right)`, while the leaves are the values within the parentheses
fn print_huffman_table<W>(table: &[TBLentry], entry: usize, mut buf: &mut W) 
    where W: Write 
{
    let entry = &table[entry];
    if let Some(val) = entry.value {
        write!(&mut buf, "{}", val).unwrap();
    } else {
        write!(&mut buf, "(").unwrap();
        print_huffman_table(table, entry.left, buf);
        write!(&mut buf, ",").unwrap();
        print_huffman_table(table, entry.right, buf);
        write!(&mut buf, ")").unwrap();
    }
}