//! Information and structures for `vpk0` files.
//!
//! There are three structures at the start of a `vpk0` file:
//! 1. Header
//! 2. Offset Bitsize Huffman Tree
//! 3. Length Bitsize Huffman Tree
//!
//! ## Header
//! There is a nine byte header at beginning of the file that includes the sample
//! number for the encoded offsets, as well as the size of the decompressed data.
//! The key data can be extracted into a [`VpkHeader`] by using [`vpk_info()`].
//!
//! | Byte Num | Description |
//! | :------: | ----------- |
//! | 0..4     | magic bytes ("vpk0") |
//! | 4..8     | size in big endian bytes of decompressed data |
//! | 9        | Sample Method (0 => One Sample; 1 => Two Sample ) |
//!
//! ## Huffman Trees
//! After the header, there are two linearly encoded Huffman trees: one for the offsets,
//! and one for the lengths.
//! The two trees can be extracted into their `String` representations as [`TreeInfo`]
//! by using [`vpk_info()`].
//!
//! Tree leafs are encoded as a `0` followed by an eight bit value leaf.
//! Tree nodes are encoded by a `1`, and combine the most recent two nodes/leaves.
//
//! For example, the simple tree `(1, (4, 7))` would be encoded in binary as:
//! ```text
//! 0 00000001 0 00000100 0 00000111 1 1
//! ```
//! and it would give the following Huffman codes:
//!
//! | Bitsize | Code |
//! | ------- | ---- |
//! |    1    |  0   |
//! |    4    |  10  |
//! |    7    |  11  |
//!
//! So, let's say that you wanted to encode an offset of seven with the same tree as above:
//! ```text
//! ┌ read next four bits
//! |  ┌ value
//! 10 0111
//! ```
//! Single leaf trees are valid, but the leaf will have a "zero length" huffman code.
//! Existing decoders return of value of zero for a zero-length tree.
//!
//! Note that the trees do not store the actual offset or length value, but
//! rather they store the number of bits to read for the actual offset or length.
//!
//! ### Offset Tree
//! The offset tree stores the bit sizes for encoding an "LZSS" offset value. The offset tells
//! the decoder how far to move back in the decoded output before copying back.
//! While the `vpk0` format can single or double sample encoded offsets, those differences do not
//! affect the tree.
//!
//! ### Length Tree
//! The length tree stores the bit sizes for encoding an "LZSS" length value. The length tells
//! the decoder how many bytes copy back from decoded output.
//!
//! ## An Example
//! Let's encode the exciting and useful ascii string "YAAAAAAAAAAAAAA" into a one sample `vpk0` file.
//! We'll use an inefficient offset and length tree of `(1, (4, 7))`.
//! The LZSS encoding of the input will be ['Y', 'A', (1, 13)], where (1, 13) is an offset of 1
//! and a length of 13.
//! ```text
//! Header
//! 76706B30 <- "vpk0"
//! 0000000F <- original file size of 15 bytes
//! 00       <- one sample
//!
//! Offset Tree (see above for how this was generated)
//! 0 00000001 0 00000100 0 00000111 1 1
//! Length Tree
//! 0 00000001 0 00000100 0 00000111 1 1
//!
//! Encoded Data
//! 0 01011001 <- uncoded ascii 'Y'
//! 0 01000001 <- uncoded ascii 'A'
//! ┌ encoded
//! | ┌ read next 1 bit
//! | | ┌ offset value (1)
//! | | | ┌ read next 7 bits
//! | | | |  ┌ length value (13)
//! 1 0 1 11 0010011
//! ```
//! [`vpk_info()`]: crate::vpk_info

use crate::errors::VpkError;
use bitstream_io::{BitReader, BitWriter, BE};
use std::convert::TryInto;
use std::fmt;
use std::io::{Read, Write};
use std::str;

// re-export the string representations of the Huffman trees
// makes more sense to be here for users, imho
pub use crate::decode::TreeInfo;

/// Valid lookback methods for a VPK compressed file.
///
/// `OneSample` directly encodes the offset value in stream,
/// while `TwoSample` encodes a modified form of the offset as either one
/// or two values.
///
/// The two sample mode encodes an offset by adding eight to the original value,
/// then dividing that value by four. If there is no remainder,
/// the quotient is stored as a single sample. Otherwise,
/// the `remainder - 1` is stored followed by the quotient
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum VpkMethod {
    OneSample = 0,
    TwoSample = 1,
}

impl fmt::Display for VpkMethod {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::OneSample => write!(f, "Method 0 (One Sample)"),
            Self::TwoSample => write!(f, "Method 1 (Two Sample)"),
        }
    }
}

/// The information stored at the start of a `vpk0` file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VpkHeader {
    /// size of decompressed data
    pub size: u32,
    pub method: VpkMethod,
}
impl VpkHeader {
    /// Parse VPK header from a byte array
    fn from_array(arr: &[u8; 9]) -> Result<Self, VpkError> {
        let name = str::from_utf8(&arr[0..4]).map_err(VpkError::Utf8Error)?;
        if name != "vpk0" {
            return Err(VpkError::InvalidHeader(name.into()));
        }

        let size = u32::from_be_bytes(arr[4..8].try_into().unwrap());
        let method = match arr[8] {
            0 => Ok(VpkMethod::OneSample),
            1 => Ok(VpkMethod::TwoSample),
            unk => Err(VpkError::InvalidMethod(unk)),
        }?;

        Ok(Self { size, method })
    }
    /// Convenience function to read the `vpk0` header from a bitstream
    pub(crate) fn from_bitreader<R: Read>(reader: &mut BitReader<R, BE>) -> Result<Self, VpkError> {
        let mut header = [0u8; 9];
        reader.read_bytes(&mut header)?;

        Self::from_array(&header)
    }
    /// Write out `self` to the big endian `BitWriter` to match the vpk format
    pub(crate) fn write<W: Write>(&self, wtr: &mut BitWriter<W, BE>) -> Result<(), VpkError> {
        wtr.write_bytes(b"vpk0")?; // 0..4
        wtr.write(32, self.size)?; // 4..8
        let method = match self.method {
            VpkMethod::OneSample => 0,
            VpkMethod::TwoSample => 1,
        };
        wtr.write(8, method)?; // 8

        Ok(())
    }
}

/// A Huffman tree node or leaf designed to be stored in an array
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TreeEntry {
    // left and right are indices into a `HufTree` array
    Node { left: usize, right: usize },
    Leaf(u8),
}

/// An array based huffman tree
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VpkTree {
    entries: Vec<TreeEntry>,
}

impl VpkTree {
    /// Create an empty tree.
    /// This will be written to the output buffer as a single true bit (1)
    pub(crate) fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
    pub(crate) fn from_bitreader<R: Read>(bits: &mut BitReader<R, BE>) -> Result<Self, VpkError> {
        let mut entries: Vec<TreeEntry> = Vec::new();
        let mut buf: Vec<usize> = Vec::new();

        loop {
            let new_entry_idx = entries.len();
            // create a Node (1) or Leaf (0)
            if bits.read_bit()? {
                // if there are less than 2 "outstanding" entries, the tree is done
                if buf.len() < 2 {
                    break;
                }

                entries.push(TreeEntry::Node {
                    right: buf.pop().unwrap(),
                    left: buf.pop().unwrap(),
                });
            } else {
                // add a leaf node with an 8-bit value
                entries.push(TreeEntry::Leaf(bits.read(8)?));
            }
            // store a reference to new leaf or node in the buf for later combination
            buf.push(new_entry_idx);
        }

        Ok(Self { entries })
    }
    /// Use `BitReader` `bits` to read a value out from this `HuffTree`
    pub(crate) fn read_value<R: Read>(&self, bits: &mut BitReader<R, BE>) -> Result<u32, VpkError> {
        let tbl = &self.entries;
        let len = tbl.len();
        if len == 0 {
            return Ok(0);
        };
        // tree starts from end
        let mut idx = len - 1;
        while let TreeEntry::Node { left, right } = tbl[idx] {
            if bits.read_bit()? {
                idx = right;
            } else {
                idx = left;
            }
        }
        // make a loop -> match set to just return this?
        match tbl[idx] {
            TreeEntry::Leaf(size) => Ok(bits.read(size as u32)?),
            _ => Err(VpkError::BadTreeEncoding),
        }
    }
    /// Write `self` to the Big Endian `BitWriter` in the expected VPK format
    pub(crate) fn write<W: Write>(&self, wtr: &mut BitWriter<W, BE>) -> Result<(), VpkError> {
        for entry in &self.entries {
            match entry {
                TreeEntry::Leaf(val) => {
                    wtr.write_bit(false)?;
                    wtr.write(8, *val)?;
                }
                TreeEntry::Node { .. } => {
                    wtr.write_bit(true)?;
                }
            }
        }
        // end tree
        wtr.write_bit(true).map_err(Into::into)
    }

    fn _format_entry(&self, entry: usize, f: &mut fmt::Formatter) -> fmt::Result {
        match self.entries[entry] {
            TreeEntry::Leaf(val) => write!(f, "{}", val),
            TreeEntry::Node { left, right } => {
                write!(f, "(")?;
                self._format_entry(left, f)?;
                write!(f, ", ")?;
                self._format_entry(right, f)?;
                write!(f, ")")
            }
        }
    }
}

impl fmt::Display for VpkTree {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.entries.is_empty() {
            write!(f, "()")
        } else {
            self._format_entry(self.entries.len() - 1, f)
        }
    }
}

impl From<Vec<TreeEntry>> for VpkTree {
    fn from(entries: Vec<TreeEntry>) -> Self {
        Self { entries }
    }
}
