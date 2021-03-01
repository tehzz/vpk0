use crate::{
    errors::VpkError,
    format::{VpkHeader, VpkMethod},
};
use bitstream_io::{BigEndian, BitWriter};
use std::{
    fs::File,
    io::Write,
    io::{BufReader, BufWriter, Cursor, Read},
    path::Path,
};

mod huffman;
pub(crate) mod lzss;

use self::{
    huffman::{EncodedMaps, MapTree},
    lzss::{LzssByte, LzssPass, LzssSettings},
};

type BitSize = u8;
type Frequency = u64;
type LogWtr<'a> = &'a mut dyn Write;

/// The algorithm used to find matches when encoding a `vpk0` file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LzssBackend {
    /// Naive, brute force search. Works well for matching Nintendo
    Brute,
    /// Search with the Knuth–Morris–Pratt algorithm.
    Kmp,
    /// Nintendo matching search with a modified, slower Knuth–Morris–Pratt algorithm
    KmpAhead,
}

/// Specify the encoding settings, such as window size, logging, input, and output
///
/// To create a new `EncoderBuilder`, use [`for_reader()`], [`for_file()`], or [`for_bytes()`].
/// Then, change any of the encoding settings with `EncoderBuilder`'s helper methods.
/// Finally, encode the input data with [`encode_to_writer()`], [`encode_to_file()`], or [`encode_to_vec()`].
/// ```
/// # use vpk0::{EncoderBuilder, LzssBackend};
/// let input = b"ABBACABBCADFEGABA";
/// let compressed = EncoderBuilder::for_bytes(input)
///     .two_sample()
///     .lzss_backend(LzssBackend::Kmp)
///     .with_logging(&mut ::std::io::stdout())
///     .encode_to_vec();
/// ```
///
/// The default encoding settings are as follows:
/// * One Sample encoding
/// * No user offset or length values
/// * No logging
/// * LZSS settings:
///   * 16 bit window (65536 bytes)
///   * 8 bit lookahead (256 bytes)
///   * Minimum match of 3 bytes
///   * [`Brute`] match searching
///
/// [`for_reader()`]: EncoderBuilder::for_reader
/// [`for_file()`]: EncoderBuilder::for_file
/// [`for_bytes()`]: EncoderBuilder::for_bytes
/// [`encode_to_writer()`]: EncoderBuilder::encode_to_writer
/// [`encode_to_file()`]: EncoderBuilder::encode_to_file
/// [`encode_to_vec()`]: EncoderBuilder::encode_to_vec
/// [`Brute`]: LzssBackend::Brute
pub struct EncoderBuilder<'a, R> {
    rdr: R,
    method: VpkMethod,
    settings: LzssSettings,
    backend: LzssBackend,
    log: Option<LogWtr<'a>>,
    offsets: Option<&'a str>,
    lengths: Option<&'a str>,
}

impl<'a, R: Read> EncoderBuilder<'a, R> {
    /// Create a new `EncoderBuilder` for the data in `rdr`.
    #[inline]
    pub fn for_reader(rdr: R) -> Self {
        Self {
            rdr,
            method: VpkMethod::OneSample,
            settings: LzssSettings::default(),
            backend: LzssBackend::Brute,
            log: None,
            offsets: None,
            lengths: None,
        }
    }

    /// Set the encoded VPK file to use either a one sample offset lookback,
    /// or a two sample lookback.
    ///
    /// In one sample mode, the offset value is directly encoded into the output.
    /// In two sample mode, the offset value is divided by four. Then the
    /// remainder (if necessary) and quotient are stored in the output.
    #[inline]
    pub fn method(&mut self, method: VpkMethod) -> &mut Self {
        self.method = method;
        self
    }

    /// Conveince method to set one sample encoding without importing [`VpkMethod`].
    #[inline]
    pub fn one_sample(&mut self) -> &mut Self {
        self.method = VpkMethod::OneSample;
        self
    }

    /// Conveince method to set two sample encoding without importing [`VpkMethod`].
    #[inline]
    pub fn two_sample(&mut self) -> &mut Self {
        self.method = VpkMethod::TwoSample;
        self
    }

    /// Set the settings used for the underyling lzss compression. See [`LzssSettings`] for more details.
    #[inline]
    pub fn with_lzss_settings(&mut self, settings: LzssSettings) -> &mut Self {
        self.settings = settings;
        self
    }

    /// Set the algorithm used to search for LZSS matches when encoding
    #[inline]
    pub fn lzss_backend(&mut self, backend: LzssBackend) -> &mut Self {
        self.backend = backend;
        self
    }

    /// Manually set the offset Huffman Tree with a text based representation of a tree.
    /// This representation can be extracted from a `vpk0` file by [`vpk_info`](crate::vpk_info)
    /// or [`DecoderBuilder::trees`](crate::DecoderBuilder::trees).
    /// ```
    /// # use vpk0::EncoderBuilder;
    /// let compressed = EncoderBuilder::for_bytes(b"sam I am I am sam")
    ///     .with_offsets("(3, (7, 10))")
    ///     .encode_to_vec();
    /// ```
    /// Note that the encoding will fail if there is an offset whose size in bits is larger
    /// than the largest provided offset.
    #[inline]
    pub fn with_offsets(&mut self, o: &'a str) -> &mut Self {
        self.offsets = Some(o);
        self
    }

    /// Set the offset Huffman Tree if `offsets.is_some()`,
    /// else create the offset tree from the input data.
    #[inline]
    pub fn optional_offsets(&mut self, offsets: Option<&'a str>) -> &mut Self {
        self.offsets = offsets;
        self
    }

    /// Manually set the length Huffman Tree with a text based representation of a tree.
    /// This representation can be extracted from a `vpk0` file by [`vpk_info`](crate::vpk_info)
    /// or [`DecoderBuilder::trees`](crate::DecoderBuilder::trees).
    /// ```
    /// # use vpk0::EncoderBuilder;
    /// let compressed = EncoderBuilder::for_bytes(b"sam I am I am sam")
    ///     .with_lengths("((3, 5), (7, (12, 16))")
    ///     .encode_to_vec();
    /// ```
    /// Note that the encoding will fail if there is an offset whose size in bits is larger
    /// than the largest provided offset.
    #[inline]
    pub fn with_lengths(&mut self, l: &'a str) -> &mut Self {
        self.lengths = Some(l);
        self
    }

    /// Set the length Huffman Tree if `offsets.is_some()`,
    /// else create the offset tree from the input data.
    #[inline]
    pub fn optional_lengths(&mut self, lengths: Option<&'a str>) -> &mut Self {
        self.lengths = lengths;
        self
    }

    /// Write debugging and diagnotic information to `log` while the input is
    /// being encoded.
    #[inline]
    pub fn with_logging<L: Write>(&mut self, log: &'a mut L) -> &mut Self {
        let log = Some(log as &'a mut dyn Write);
        self.log = log;
        self
    }

    /// Start the encoding and write the compressed data out to `wtr`
    #[inline]
    pub fn encode_to_writer<W: Write>(&mut self, wtr: W) -> Result<(), VpkError> {
        do_encode(self, wtr)
    }

    /// Start the encoding and write the compressed data out to the newly created
    /// `File` `f`
    #[inline]
    pub fn encode_to_file<P: AsRef<Path>>(&mut self, f: P) -> Result<(), VpkError> {
        let wtr = BufWriter::new(File::create(f)?);
        self.encode_to_writer(wtr)
    }

    /// Start the encoding and return the compressed data in a `Vec<u8>`.
    #[inline]
    pub fn encode_to_vec(&mut self) -> Result<Vec<u8>, VpkError> {
        let data = Vec::new();
        let mut csr = Cursor::new(data);
        self.encode_to_writer(&mut csr).map(|_| csr.into_inner())
    }
}

impl<'a> EncoderBuilder<'a, BufReader<File>> {
    /// Create a new `EncoderBuilder` for the file at `p`.
    #[inline]
    pub fn for_file<P: AsRef<Path>>(p: P) -> Result<Self, VpkError> {
        let rdr = BufReader::new(File::open(p)?);
        Ok(Self::for_reader(rdr))
    }
}

impl<'a> EncoderBuilder<'a, Cursor<&'a [u8]>> {
    /// Create a new `EncoderBuilder` for the data the `bytes` slice.
    #[inline]
    pub fn for_bytes(bytes: &'a [u8]) -> Self {
        let rdr = Cursor::new(bytes);
        Self::for_reader(rdr)
    }
}

/// Compress data into a `vpk0` `Vec<u8>`
///
/// This is a convenience function to encode a `Read`er without having to
/// import and set up an [`EncoderBuilder`].
pub fn encode<R: Read>(rdr: R) -> Result<Vec<u8>, VpkError> {
    EncoderBuilder::for_reader(rdr).encode_to_vec()
}

fn do_encode<R: Read, W: Write>(
    opts: &mut EncoderBuilder<'_, R>,
    mut wtr: W,
) -> Result<(), VpkError> {
    let EncoderBuilder {
        rdr,
        method,
        settings,
        ref mut log,
        offsets,
        lengths,
        backend,
    } = opts;

    let lzss = lzss::compress_rdr(rdr, *settings, *method, *backend, log)?;
    let huff_maps = huffman::EncodedMaps::new(*offsets, *lengths, &lzss)?;

    if let Some(wtr) = log.as_mut() {
        writeln!(wtr, "Huff Offsets / Movebacks\n{}", huff_maps.offsets)?;
        writeln!(wtr, "Huff Lengths / Size\n{}", huff_maps.lengths)?;
        //writeln!(info_wtr, "{}", &lzss)?;
    }

    write_file(&mut wtr, *method, &lzss, &huff_maps)
}

fn write_file(
    wtr: &mut dyn Write,
    method: VpkMethod,
    encoded_data: &LzssPass,
    trees: &EncodedMaps,
) -> Result<(), VpkError> {
    let mut out = BitWriter::endian(wtr, BigEndian);
    let header = VpkHeader {
        // TODO: error, or maybe remove the option from here...
        size: encoded_data.decompressed_size.unwrap(),
        method,
    };

    header.write(&mut out)?;
    trees.offsets.tree.write(&mut out)?;
    trees.lengths.tree.write(&mut out)?;

    for code in &encoded_data.buf {
        // Can these be an `if let else` block?
        match *code {
            LzssByte::Uncoded(byte) => {
                out.write_bit(LzssSettings::UNCODED)?;
                out.write(8, byte)?;
            }
            LzssByte::Encoded(length, offset) => {
                let maps = &[(offset, &trees.offsets), (length, &trees.lengths)];

                out.write_bit(LzssSettings::ENCODED)?;
                for &set in maps {
                    write_encoded_val(&mut out, set)?;
                }
            }
            LzssByte::EncTwoSample(length, sample) => {
                let one_arr;
                let two_arr;

                let offsets = match sample {
                    TwoSample::One(offset) => {
                        one_arr = [(offset, &trees.offsets)];
                        &one_arr[..]
                    }
                    TwoSample::Two { first, second } => {
                        two_arr = [(first, &trees.offsets), (second, &trees.offsets)];
                        &two_arr[..]
                    }
                };
                let length = [(length, &trees.lengths)];

                out.write_bit(LzssSettings::ENCODED)?;
                for &set in offsets.iter().chain(&length) {
                    write_encoded_val(&mut out, set)?;
                }
            }
        }
    }

    out.byte_align()?;

    Ok(())
}

fn write_encoded_val(
    out: &mut BitWriter<&mut dyn Write, BigEndian>,
    (val, map): (usize, &MapTree),
) -> Result<(), VpkError> {
    let needed_bits = count_needed_bits(val);
    // TODO: replace unwrap with custom error
    let (encoded_bits, code) = map.get(needed_bits).unwrap();
    out.write(code.bitlen(), code.code)?;
    out.write(encoded_bits as u32, val as u32)?;

    Ok(())
}

/// calculate how many bits are needed to represent `val`
const fn count_needed_bits(val: usize) -> BitSize {
    (usize::MAX.count_ones() - val.leading_zeros()) as u8
}

/// Devolve an offset/moveback value into two separate, smaller values
/// This matches Nintendo's two-sample which uses
/// One => (move * 4) - 8
/// Two => (first + 1 + (second * 4)) - 8
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TwoSample {
    One(usize),
    Two { first: usize, second: usize },
}

impl TwoSample {
    const LIMIT: usize = 4;
}

impl From<usize> for TwoSample {
    fn from(val: usize) -> Self {
        let val = val + 8;
        let quot = val / Self::LIMIT;
        let rem = val % Self::LIMIT;

        if rem != 0 {
            let first = rem - 1;
            let second = quot;

            Self::Two { first, second }
        } else {
            Self::One(quot)
        }
    }
}
