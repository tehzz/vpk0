use errors::{VpkError};

use std::io::{Read};
use std::fmt;
use std::str;
use byteorder::{BE, ByteOrder};
use bitstream_io::{BE as bBE, BitReader};
use log::Level::{Info};

/// Decode a Reader of vpk0 data into a Vec of the decompressed data
pub fn decode<R>(mut buf: R) -> Result<Vec<u8>, VpkError>
    where R: Read
{
    let mut vpk0_bits = BitReader::<bBE>::new(&mut buf);

    let header = VpkHeader::from_bitreader(&mut vpk0_bits)?;
    // read huffman trees from beginning of compressed input 
    let movetree = HuffTree::from_bitreader(&mut vpk0_bits)?;
    let sizetree = HuffTree::from_bitreader(&mut vpk0_bits)?;

    if log_enabled!(Info) {
        info!("\n**** vpk0 header ****\n{:?}", &header);
        info!("\n**** Move Tree ****\n{}", movetree);
        info!("\n**** Size Tree ****\n{}", sizetree);
    }


    // start decoding the compressed input buffer
    let output_size = header.size as usize;
    let mut output: Vec<u8> = Vec::with_capacity(output_size);

    while output.len() < output_size {
        if vpk0_bits.read_bit()? {
            // copy bytes from inside the output back at the end of the output
            let initial_move = movetree.read_value(&mut vpk0_bits)? as usize;
            let move_back    = match header.method {
                VpkMethod::TwoSample => {
                    if initial_move < 3 {
                        let l = initial_move + 1;
                        let u = movetree.read_value(&mut vpk0_bits)? as usize;
                        (l + (u << 2)) - 8
                    } else {
                        (initial_move << 2) - 8
                    }
                },
                VpkMethod::OneSample => initial_move,
            };

            // get start position in output, and the number of bytes to copy-back
            if move_back > output.len() {
                error!("Bad input file: asked to copy back bytes from outside of decoded output buffer");
                error!("move back: {} | output length: {}", move_back, output.len());
                return Err(VpkError::BadInput)
            }
            let p = output.len() - move_back;
            let n = sizetree.read_value(&mut vpk0_bits)? as usize;
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
        if name != "vpk0" { 
            error!("Expected 'vpk0' in header, saw '{}'", name);
            return Err(VpkError::InvalidHeader) 
        }

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

/// A Huffman tree node or leaf designed to be stored in an array
#[derive(Debug)]
struct TreeNode {
    // left and right are offsets into the array  
    left: usize,
    right: usize,
    // if None, entry is a node; if Some, entry is a leaf
    value: Option<u8>,
}

/// An array based huffman tree
#[derive(Debug)]
struct HuffTree {
    nodes: Vec<TreeNode>
}

impl HuffTree {
    fn from_bitreader(bits: &mut BitReader<bBE>) -> Result<Self, VpkError> {
        let mut nodes: Vec<TreeNode> = Vec::new();
        let mut buf: Vec<usize>      = Vec::new();
        
        let mut idx = 0;        // free node count
        let mut fin = 0;        // most recently added node

        loop {
            if bits.read_bit()? {
                // create node at a higher level,
                // if there are more than two leaves/nodes to combine 
                if idx < 2 { break; }

                nodes.push( TreeNode {
                    left:  buf[idx-2],
                    right: buf[idx-1],
                    value: None,
                });
                buf[idx - 2] = fin;
                fin += 1;
                idx -= 1;
            } else {
                // add a leaf node with an 8-bit value
                nodes.push( TreeNode {
                    left: 0, right: 0,
                    value: Some(bits.read(8)?)
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

        Ok(Self{nodes})
    }

    fn read_value(&self, bits: &mut BitReader<bBE>) -> Result<u32, VpkError> {
        let tbl = &self.nodes;
        let len = tbl.len();
        if len == 0 { return Ok(0) };
        let mut idx = len - 1;

        while tbl[idx].value.is_none() {
            if bits.read_bit()? {
                idx = tbl[idx].right;
            } else {
                idx = tbl[idx].left;
            }
        }
        let output = bits.read(tbl[idx].value.unwrap() as u32)?;
        Ok(output) 
    }

    fn _format_entry(&self, entry: usize, f: &mut fmt::Formatter) -> fmt::Result {
        let node = &self.nodes[entry];
        if let Some(val) = node.value {
            write!(f, "{}", val)
        } else {
            write!(f, "(")?;
            self._format_entry(node.left, f)?;
            write!(f, ", ")?;
            self._format_entry(node.right, f)?;
            write!(f, ")")
        }
    }
}

impl fmt::Display for HuffTree {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self._format_entry(self.nodes.len() - 1, f)
    }
}