use crate::errors::VpkError;
use crate::format::{VpkHeader, VpkMethod, VpkTree};
use bitstream_io::{BigEndian, BitReader};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufReader, Cursor, Read, Write},
    path::Path,
};

type LogWtr<'a> = &'a mut dyn Write;
// [offset, length]
type RawTrees = [VpkTree; 2];

/// Textual representations of the offsets and lengths Huffman trees in a `vpk0` file
///
/// The trees are comprised of decimal numbers—the leafs—separated by commas and parentheses—the nodes.
/// The trees also follow the typical Huffman Tree convention of `0` for left nodes
/// and `1` for right nodes. So, if you have `((4, 1), (8, (15, 10))`,
/// `4` would have the Huffman code `00` and `15` would have the Huffman code `110`.
///
/// You can get a `TreeInfo` by using [`vpk_info`] or [`Decoder::trees`].
/// The `String`s can then be used by [`Encoder::with_offsets`] (and related functions)
/// to set the offsets and lengths tree in a new encode.
///
/// [`Encoder::with_offsets`]: crate::Encoder::with_offsets()
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeInfo {
    pub offsets: String,
    pub lengths: String,
}

impl From<&RawTrees> for TreeInfo {
    fn from([offsets, lengths]: &RawTrees) -> Self {
        Self {
            offsets: offsets.to_string(),
            lengths: lengths.to_string(),
        }
    }
}

/// Specify the decoding settings, such as logging, input, and output.
///
/// To create a new `Decoder`, use [`for_reader()`], [`for_bytes()`], or
/// [`for_file()`]. Then, change any of the decoder settings.
/// Finally, decode the input data with [`decode`].
/// ```
/// # use vpk0::{Encoder, Decoder};
/// let original = b"ABBACABBACD";
/// let compressed = Encoder::for_bytes(original)
///     .encode_to_vec()
///     .unwrap();
/// let decompressed = Decoder::for_bytes(&compressed)
///     .decode()
///     .unwrap();
/// assert_eq!(&original[..], decompressed);
/// ```
/// You can use a `Decoder` to get the [`VpkHeader`] with [`header()`]
///  or [`TreeInfo`] with [`trees()`]:
/// ```
/// # use vpk0::{Encoder, Decoder};
/// # let original = b"ABBACABBACD";
/// # let compressed = Encoder::for_bytes(original).encode_to_vec().unwrap();
/// let mut decoder = Decoder::for_bytes(&compressed);
/// let size = decoder.header().unwrap().size as usize;
/// assert_eq!(size, original.len());
/// ```
/// [`for_reader()`]: Decoder::for_reader
/// [`for_bytes()`]: Decoder::for_bytes
/// [`for_file()`]: Decoder::for_file
/// [`decode()`]: Decoder::decode
/// [`header()`]: Decoder::header
/// [`trees()`]: Decoder::trees
pub struct Decoder<'a, R: Read> {
    src: BitReader<R, BigEndian>,
    log: Option<LogWtr<'a>>,
    info: Option<(VpkHeader, RawTrees)>,
}

impl<'a, R: Read> Decoder<'a, R> {
    #[inline]
    pub fn for_reader(rdr: R) -> Self {
        Self {
            src: BitReader::endian(rdr, BigEndian),
            log: None,
            info: None,
        }
    }

    #[inline]
    pub fn with_logging<W: Write>(&mut self, wtr: &'a mut W) -> &mut Self {
        self.log = Some(wtr as LogWtr);
        self
    }

    #[inline]
    pub fn header(&mut self) -> Result<VpkHeader, VpkError> {
        self.get_file_info().map(|(hdr, _)| *hdr)
    }

    #[inline]
    pub fn trees(&mut self) -> Result<TreeInfo, VpkError> {
        self.get_file_info().map(|(_, t)| t.into())
    }

    #[inline]
    pub fn decode(&mut self) -> Result<Vec<u8>, VpkError> {
        do_decode(self)
    }

    fn get_file_info(&mut self) -> Result<&(VpkHeader, RawTrees), VpkError> {
        if let Some(ref info) = self.info {
            Ok(info)
        } else {
            let hdr = VpkHeader::from_bitreader(&mut self.src)?;
            let offsets = VpkTree::from_bitreader(&mut self.src)?;
            let lengths = VpkTree::from_bitreader(&mut self.src)?;

            self.info = Some((hdr, [offsets, lengths]));
            Ok(self.info.as_ref().unwrap())
        }
    }
}

impl<'a> Decoder<'a, Cursor<&'a [u8]>> {
    #[inline]
    pub fn for_bytes(bytes: &'a [u8]) -> Self {
        let rdr = Cursor::new(bytes);
        Self::for_reader(rdr)
    }
}

impl<'a> Decoder<'a, BufReader<File>> {
    #[inline]
    pub fn for_file<P: AsRef<Path>>(p: P) -> Result<Self, VpkError> {
        File::open(p)
            .map(BufReader::new)
            .map(Self::for_reader)
            .map_err(Into::into)
    }
}

/// Decompress `vpk0` data into a `Vec<u8>`
///
/// This is a convenience function to decode a `Read`er without
/// having to import and set up a [`Decoder`]
pub fn decode<R: Read>(rdr: R) -> Result<Vec<u8>, VpkError> {
    Decoder::for_reader(rdr).decode()
}

/// Extract the [`VpkHeader`] and [`TreeInfo`] from `vpk0` data
///
/// This is a convenience function to extract information about `vpk0` data without having
/// to set up a [`Decoder`]
pub fn vpk_info<R: Read>(rdr: R) -> Result<(VpkHeader, TreeInfo), VpkError> {
    let mut decoder = Decoder::for_reader(rdr);

    decoder
        .header()
        .and_then(|hdr| decoder.trees().map(|t| (hdr, t)))
}

fn do_decode<R: Read>(opt: &mut Decoder<R>) -> Result<Vec<u8>, VpkError> {
    let info = if let Some(info) = opt.info.as_ref() {
        info
    } else {
        opt.get_file_info()?;
        opt.info.as_ref().unwrap()
    };
    let &(header, [ref offsets, ref lengths]) = info;
    let Decoder { src, log, .. } = opt;

    // set up the log with a map to store the bitsizes of the offsets and lengths
    let mut log = log.as_mut().map(|l| (l, LogFreq::new()));

    if let Some((wtr, _)) = &mut log {
        writeln!(wtr, "# Header\n{:?}", &header)?;
        writeln!(wtr, "## Offset / Moveback Tree\n{}", offsets)?;
        writeln!(wtr, "###> {:?}", offsets)?;
        writeln!(wtr, "## Length / Size Tree\n{}", lengths)?;
        writeln!(wtr, "###> {:?}", lengths)?;
        writeln!(wtr)?;
    }

    let output_size = header.size as usize;
    let mut output: Vec<u8> = Vec::with_capacity(output_size);

    while output.len() < output_size {
        if src.read_bit()? {
            let initial_move = offsets.read_value(src)? as usize;
            let move_back = match header.method {
                VpkMethod::TwoSample => {
                    if initial_move < 3 {
                        let l = initial_move + 1;
                        let u = offsets.read_value(src)? as usize;

                        if let Some((wtr, _)) = &mut log {
                            writeln!(
                                wtr,
                                "Encoded 2-sample => initial move: {} | second move: {}",
                                initial_move, u
                            )?;
                        }

                        (l + (u << 2)) - 8
                    } else {
                        if let Some((wtr, _)) = &mut log {
                            writeln!(wtr, "Encoded 2-sample => initial move: {}", initial_move)?;
                        }
                        (initial_move << 2) - 8
                    }
                }
                VpkMethod::OneSample => initial_move,
            };

            // get start position in output, and the number of bytes to copy-back
            if move_back > output.len() {
                return Err(VpkError::BadLookBack(move_back, output.len()));
            }

            let start = output.len() - move_back;
            let size = lengths.read_value(src)? as usize;

            if let Some((wtr, map)) = &mut log {
                let size_bits = usize::MAX.count_ones() - size.leading_zeros();
                let mb_bits = usize::MAX.count_ones() - move_back.leading_zeros();
                writeln!(
                    wtr,
                    "{:04x} - Encoded [Copyback]: size: {} ({} bits) mb: {} ({} bits) | start: {:04x}",
                    output.len(),
                    size,
                    size_bits,
                    move_back,
                    mb_bits,
                    start
                )?;
                *map.size.entry(size_bits as u8).or_insert(0) += 1;
                *map.moveback.entry(mb_bits as u8).or_insert(0) += 1;
            }

            for i in start..start + size {
                let byte = output[i];
                output.push(byte);
            }
            if let Some((wtr, _)) = &mut log {
                writeln!(wtr, "\t{:02x?}", &output[start..start + size])?;
            }
        } else {
            let byte = src.read(8)?;
            output.push(byte);

            if let Some((wtr, _)) = &mut log {
                writeln!(wtr, "{:04x} - Uncoded: {:02x}", output.len() - 1, byte)?;
            }
        }
    }

    Ok(output)
}

#[derive(Debug)]
struct LogFreq {
    size: BTreeMap<u8, u32>,
    moveback: BTreeMap<u8, u32>,
}

impl LogFreq {
    fn new() -> Self {
        Self {
            size: BTreeMap::new(),
            moveback: BTreeMap::new(),
        }
    }
}
