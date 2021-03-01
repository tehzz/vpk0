use std::{
    collections::HashMap,
    convert::TryInto,
    fmt,
    io::{self, Read, Write},
};

use slice_deque::SliceDeque;

use crate::{errors::VpkError, format::VpkMethod};

use super::{count_needed_bits, BitSize, Frequency, LzssBackend, TwoSample};

/// Configure the LZSS encoding that underlies `vpk0` compression
///
/// You can set the three key [LZSS parameters]: dictionary size, maximum match size,
/// and minimum match size. When using [`new`](LzssSettings::new) or `struct`
/// literals, you are setting the total number of bits for the dictionary or max match,
/// but the minimum match size is in bytes. If you'd prefer to set everything in terms
/// of bytes, you can use [`byte_sized`](LzssSettings::byte_sized). Note that any
/// non-power-of-two byte sizes will be rounded up for the dictionary and max match.
///
/// By [`default`](LzssSettings::default):
///
/// | Parameter  | Field       | Bit Size | Bytes |
/// | ---------- | ----------- | :------: | :---: |
/// | Dictionary | offset_bits | 16       | 65536 |
/// | Max Match  | length_bits | 8        | 256   |
/// | Min Match  | max_uncoded |          | 2     |
///
/// These settings were used by Nintendo when compressing the files
/// in **Super Smash Bros. 64**.
///
/// [LZSS parameters]: https://michaeldipperstein.github.io/lzss.html
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct LzssSettings {
    /// number of bits for move back (window size)
    pub offset_bits: usize,
    /// number of bits for copying (max match encoded)
    pub length_bits: usize,
    /// max number of bytes not encoded
    pub max_uncoded: usize,
}

impl LzssSettings {
    pub(crate) const ENCODED: bool = true;
    pub(crate) const UNCODED: bool = false;

    pub const fn new(offset_bits: usize, size_bits: usize, max_uncoded: usize) -> Self {
        Self {
            offset_bits,
            length_bits: size_bits,
            max_uncoded,
        }
    }

    pub const fn byte_sized(dictionary: usize, max_match: usize, min_match: usize) -> Self {
        let offset_bits = count_needed_bits(dictionary) as usize;
        let size_bits = count_needed_bits(max_match) as usize;
        Self {
            offset_bits,
            length_bits: size_bits,
            max_uncoded: min_match,
        }
    }

    const fn window_size(&self) -> usize {
        // overflow assert?
        (1 << self.offset_bits) - 1
    }
    /// maximum number of bytes that can be encoded
    /// note that Nintendo's VPK encoder uses the extra `max_uncoded` bits for
    /// encoding a TwoSample vpk file, so you cannot use them here to encode longer matches
    const fn max_encoded(&self) -> usize {
        // overflow assert?
        (1 << self.length_bits) - 1
    }
}

impl Default for LzssSettings {
    fn default() -> Self {
        Self {
            offset_bits: 16,
            length_bits: 8,
            max_uncoded: 2,
        }
    }
}

#[derive(Debug)]
pub(super) struct LzssPass {
    pub buf: Vec<LzssByte>,
    pub decompressed_size: Option<u32>,
    // for the bit size of copy back size (lzss "length")
    pub size_bitfreq: HashMap<BitSize, Frequency>,
    // for the bit size of moveback (lzss "offset" or "distance")
    pub moveback_bitfreq: HashMap<BitSize, Frequency>,
}

impl LzssPass {
    fn new(input_size: usize, settings: &LzssSettings) -> Self {
        let buf = Vec::with_capacity(input_size);
        let max_size_bits = count_needed_bits(settings.max_encoded()) as usize;
        let size_bitfreq = HashMap::with_capacity(max_size_bits);
        let max_mb_bits = count_needed_bits(settings.window_size()) as usize;
        let moveback_bitfreq = HashMap::with_capacity(max_mb_bits);

        Self {
            buf,
            decompressed_size: None,
            size_bitfreq,
            moveback_bitfreq,
        }
    }

    fn add_uncoded(&mut self, byte: u8) {
        self.buf.push(LzssByte::Uncoded(byte))
    }

    fn add(&mut self, byte: LzssByte) {
        // count new length/size and offset/moveback bitwidths
        match byte {
            LzssByte::Encoded(size, offset) => {
                let size_bits = count_needed_bits(size);
                let mb_bits = count_needed_bits(offset);

                *self.size_bitfreq.entry(size_bits).or_insert(0) += 1;
                *self.moveback_bitfreq.entry(mb_bits).or_insert(0) += 1;
            }
            LzssByte::EncTwoSample(size, offset) => {
                let size_bits = count_needed_bits(size);
                *self.size_bitfreq.entry(size_bits).or_insert(0) += 1;

                match offset {
                    TwoSample::One(o) => {
                        let bits = count_needed_bits(o);
                        *self.moveback_bitfreq.entry(bits).or_insert(0) += 1;
                    }
                    TwoSample::Two { first, second } => {
                        for &o in &[first, second] {
                            let bits = count_needed_bits(o);
                            *self.moveback_bitfreq.entry(bits).or_insert(0) += 1;
                        }
                    }
                }
            }
            LzssByte::Uncoded(..) => {}
        };
        // add byte to buffer
        self.buf.push(byte);
    }
}

impl fmt::Display for LzssPass {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "# Moveback Bit Frequencies")?;
        writeln!(f, "{:?}", &self.moveback_bitfreq)?;
        writeln!(f, "# Size Bit Frequencies")?;
        writeln!(f, "{:?}", &self.size_bitfreq)?;
        writeln!(f)?;
        writeln!(f, "# LZSS Encoded File")?;
        let mut position = 0;
        for point in &self.buf {
            use LzssByte::*;
            write!(f, "{:04x} - ", position)?;
            match point {
                Uncoded(b) => writeln!(f, "Uncoded: {:02x}", b),
                Encoded(length, offset) => {
                    writeln!(f, "Encoded [Copyback]: size: {} mb: {}", length, offset)
                }
                EncTwoSample(length, sample) => {
                    writeln!(f, "Two Sample Encoded: size: {}, mb: {:?}", length, sample)
                }
            }?;
            position += point.size();
        }

        Ok(())
    }
}

/// Compress the data in `input` with `settings` into Vec of either coded or uncoded `LzssByte`s.
/// Debugging information will be printed to `log` if present.
pub(super) fn compress_rdr<R: Read>(
    input: R,
    settings: LzssSettings,
    method: VpkMethod,
    backend: LzssBackend,
    log: &mut Option<&mut dyn Write>,
) -> Result<LzssPass, VpkError> {
    let mut dict = SlidingDict::new(input, &settings)?;
    let mut compressed = LzssPass::new(dict.total_read, &settings);

    let lzss_algo = match backend {
        LzssBackend::Brute => &NaiveBrute as &dyn MatchFinder,
        LzssBackend::Kmp => &KmpStandard as &dyn MatchFinder,
        LzssBackend::KmpAhead => &KmpLookAhead as &dyn MatchFinder,
    };

    while dict.remaining() > 0 {
        let bytes_matched = match look_for_nearby_best_match(&dict, &settings, log, lzss_algo) {
            LookAhead::Match(skipped, m) => add_match(m, skipped, method, &mut compressed, log),
            LookAhead::Uncoded => {
                compressed.add_uncoded(dict.next_uncoded_byte().unwrap());
                1
            }
        };

        dict.advance_by(bytes_matched)?;
    }

    compressed.decompressed_size = Some(dict.total_read.try_into()?);

    /*
    if let Some(wtr) = log.as_mut() {
        writeln!(wtr, "{}", &compressed)?;
    }
    */
    Ok(compressed)
}

/// Add found `MoveBack` to `Pass1` output, and return how many bytes have been added
fn add_match(
    mat: MoveBack,
    skipped: &[u8],
    method: VpkMethod,
    output: &mut LzssPass,
    log: &mut Option<&mut dyn Write>,
) -> usize {
    let total_bytes = mat.size + skipped.len();

    if let Some(wtr) = log {
        writeln!(wtr, "adding match: {:?} then {:?}", skipped, &mat).unwrap();
    }

    for &byte in skipped {
        output.add_uncoded(byte);
    }

    let encoded = match method {
        VpkMethod::OneSample => LzssByte::Encoded(mat.size, mat.moveback),
        VpkMethod::TwoSample => LzssByte::EncTwoSample(mat.size, mat.moveback.into()),
    };

    output.add(encoded);

    total_bytes
}

#[derive(Debug, Copy, Clone)]
enum LookAhead<'a> {
    Match(&'a [u8], MoveBack),
    Uncoded,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct MoveBack {
    size: usize,     // length
    moveback: usize, // offset
}

impl MoveBack {
    fn new(size: usize, moveback: usize) -> Self {
        Self { size, moveback }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum LzssByte {
    Encoded(usize, usize),          // length, offset
    EncTwoSample(usize, TwoSample), // length, two sample offset
    Uncoded(u8),
}

impl LzssByte {
    // total number of bytes this byte encodes from the uncoded input
    fn size(&self) -> usize {
        match self {
            Self::Encoded(size, _) => *size,
            Self::EncTwoSample(size, _) => *size,
            Self::Uncoded(..) => 1,
        }
    }
}

const MAX_AHEAD_CHECK: usize = 10;

#[derive(Debug)]
struct SlidingDict<R> {
    /// size of the look-behind dictionary window
    window: usize,
    /// size of the lookahead window
    lookahead: usize,
    /// size of butter without peek bytes
    buf_size: usize,
    /// max possible size of lookahead + peek bytes
    max_ahead: usize,
    /// current position in `buf` for start of lookahead
    csr: usize,
    buf: SliceDeque<u8>,
    rdr: R,
    /// is there any more data to be read from `rdr`
    more_to_read: bool,
    /// total bytes read
    total_read: usize,
}

impl<R: Read> SlidingDict<R> {
    const MAX_PEEK: usize = MAX_AHEAD_CHECK;

    fn new(mut rdr: R, settings: &LzssSettings) -> io::Result<Self> {
        // total size of the buffer is the size of the lookback window
        // plus the size of the lookahead
        let window = settings.window_size();
        let lookahead = settings.max_encoded();
        let buf_size = window + lookahead;
        let max_ahead = lookahead + Self::MAX_PEEK;
        /*
        info!(
            "Window: {}, Ahead: {}, Capacity: {}",
            window,
            lookahead,
            buf_size + Self::MAX_PEEK,
        );
        */
        // at the start, everything is in the lookahead
        let csr = 0;
        let mut buf = SliceDeque::with_capacity(buf_size + Self::MAX_PEEK);
        buf.resize(max_ahead, 0);
        // TODO: read another way here? like the copied read_exact implementation?
        let total_read = rdr.read(&mut buf[csr..max_ahead])?;
        let more_to_read = total_read >= max_ahead;
        /*
        debug!(
            "Seting up: read {} of max {} possible => {}",
            total_read, max_ahead, more_to_read
        );
        */
        // if the rdr was too small to even fill the lookahead buffer
        // truncate the buffer back to only what was read
        buf.truncate_back(total_read);

        Ok(Self {
            window,
            lookahead,
            buf_size,
            max_ahead,
            csr,
            buf,
            rdr,
            more_to_read,
            total_read,
        })
    }
    /// get the lookahead window, ignoring any peek bytes
    fn ahead(&self) -> &[u8] {
        let end = self.buf.len().min(self.buf_size);
        &self.buf[self.csr..end]
    }

    /// get the (behind, ahead, full) buffers offset by `n` for performing ahead matches
    /// without reading new data
    fn offset_csr(&self, n: usize) -> Bufs {
        assert!(n <= Self::MAX_PEEK);
        let offset_end = self.buf.len().min(self.buf_size + n);
        let w_end = self.csr + n;
        let w_start = w_end.saturating_sub(self.window);

        let ahead = &self.buf[w_end..offset_end];
        let behind = &self.buf[w_start..w_end];
        let full = &self.buf[w_start..offset_end];

        Bufs {
            ahead,
            behind,
            full,
        }
    }

    fn next_uncoded_byte(&self) -> Option<u8> {
        self.ahead().first().copied()
    }

    /// Get the minimum number of bytes remaining. Not great as `Read` does not
    /// have a length argument
    fn remaining(&self) -> usize {
        self.ahead().len()
    }

    fn advance_by(&mut self, n: usize) -> io::Result<()> {
        // move the cursor up if needed, and record how many excess
        // bytes need to be removed from the front
        let (new_csr, excess) = {
            let p = self.csr + n;
            let m = self.window;
            (p.min(m), p.saturating_sub(m))
        };

        // println!("csr: {} -> new: {} | excess: {} | n: {}", self.csr, new_csr, excess, n);

        // remove any extra bytes from the front of the ring
        if excess > 0 {
            // trace!("draining {}", excess);
            self.buf.drain(..excess);
        }
        // advance the cursor
        self.csr = new_csr;
        // fill the back fo the ring buffer with `n` new bytes from `rdr`
        if self.more_to_read {
            let len = self.buf.len();
            self.buf.resize(len + n, 0);

            // based on `read_exact` default implementation
            let mut buf = &mut self.buf[len..len + n];
            let mut bytes_read = 0;
            while !buf.is_empty() {
                match self.rdr.read(buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let tmp = buf;
                        buf = &mut tmp[n..];
                        bytes_read += n;
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
                    e @ Err(_) => {
                        e?;
                    }
                }
            }
            //println!("try to read {} bytes => {} total read", n, bytes_read);
            self.total_read += bytes_read;

            if bytes_read < n {
                // debug!("truncating buffer back {} - {}", n, bytes_read);
                self.buf.truncate_back(len + bytes_read);
                self.more_to_read = false;
            }

            if bytes_read == 0 {
                // debug!("rdr exhausted");
                self.more_to_read = false;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct Bufs<'a> {
    ahead: &'a [u8],
    behind: &'a [u8],
    full: &'a [u8],
}

trait MatchFinder {
    fn find(
        &self,
        bufs: Bufs,
        settings: &LzssSettings,
        log: &mut Option<&mut dyn Write>,
    ) -> Option<MoveBack>;
}

#[derive(Debug, Clone, Copy)]
struct KmpStandard;
impl MatchFinder for KmpStandard {
    fn find(
        &self,
        bufs: Bufs,
        settings: &LzssSettings,
        _log: &mut Option<&mut dyn Write>,
    ) -> Option<MoveBack> {
        find_kmp(bufs, settings.max_encoded(), false)
    }
}

#[derive(Debug, Clone, Copy)]
struct KmpLookAhead;
impl MatchFinder for KmpLookAhead {
    fn find(
        &self,
        bufs: Bufs,
        settings: &LzssSettings,
        _log: &mut Option<&mut dyn Write>,
    ) -> Option<MoveBack> {
        find_kmp(bufs, settings.max_encoded(), true)
    }
}

#[derive(Debug, Clone, Copy)]
struct NaiveBrute;
impl MatchFinder for NaiveBrute {
    fn find(
        &self,
        bufs: Bufs,
        settings: &LzssSettings,
        _log: &mut Option<&mut dyn Write>,
    ) -> Option<MoveBack> {
        brute_find_match(bufs, settings)
    }
}

/// Check for the "best" match in behind window of `dict` by using `method`
/// This will keep look at the next offset for matches until either
/// (a) no match is found, or
/// (b) the found match is smaller than the previous match.
/// "Best" is, I assume, highly debateable, but this seems to match what Nintendo did.
fn look_for_nearby_best_match<'a, R>(
    dict: &'a SlidingDict<R>,
    settings: &LzssSettings,
    log: &mut Option<&mut dyn Write>,
    lzss_algo: &dyn MatchFinder,
) -> LookAhead<'a>
where
    R: Read,
{
    let m = dict
        .ahead()
        .iter()
        .enumerate()
        .take(MAX_AHEAD_CHECK)
        .scan(0, |best, (offset, _byte)| {
            if let Some(wtr) = log.as_mut() {
                writeln!(wtr, "\tlooking at offset {}", offset).unwrap();
            }
            let bufs = dict.offset_csr(offset);

            lzss_algo
                .find(bufs, settings, log)
                .filter(|m| m.size > settings.max_uncoded)
                .filter(|m| m.size > *best)
                .map(|m| {
                    *best = m.size;
                    (offset, m)
                })
        })
        .last()
        .map(|(o, m)| LookAhead::Match(&dict.ahead()[..o], m));

    if let Some(wtr) = log.as_mut() {
        writeln!(wtr, "\tfound {:?}", m).unwrap();
    }

    m.unwrap_or(LookAhead::Uncoded)
}

/// Naive search to find `bufs.ahead` in `buf.behind`.
/// This also checks for "self-matches" for patterns that start in `behind`,
/// but end in `ahead` by using `buf.full`
fn brute_find_match(bufs: Bufs, settings: &LzssSettings) -> Option<MoveBack> {
    let Bufs {
        behind,
        ahead,
        full,
    } = bufs;
    let window_size = behind.len();
    let longest_match = settings.max_encoded();
    let shortest_match = settings.max_uncoded + 1;

    (0..window_size)
        .map(|i| (i, &full[i..]))
        .filter_map(|(i, src)| {
            let length = src
                .iter()
                .zip(ahead)
                .take_while(|(s, d)| s == d)
                .count()
                .min(longest_match);

            if length >= shortest_match {
                Some(MoveBack::new(length, window_size - i))
            } else {
                None
            }
        })
        .fold(None, |best, cur| {
            best.filter(|best| best.size > cur.size || best.moveback < cur.moveback)
                .or(Some(cur))
        })
}

/* https://towardsdatascience.com/pattern-search-with-the-knuth-morris-pratt-kmp-algorithm-8562407dba5b */
fn find_kmp(bufs: Bufs, max: usize, check_rl: bool) -> Option<MoveBack> {
    let Bufs {
        ahead,
        behind,
        full,
    } = bufs;
    let lps = compute_lps(ahead);
    let window_size = behind.len();
    let pattern_size = ahead.len();

    let mut best: Option<MoveBack> = None;
    let mut target_idx = 0;
    let mut pat_idx = 0;
    while pat_idx < pattern_size && target_idx < window_size {
        let target = &full[target_idx..];
        let pattern = &ahead[pat_idx..];

        let newly_matched = target
            .iter()
            .zip(pattern)
            .take_while(|(t, p)| t == p)
            .count()
            .min(max - pat_idx);

        let match_size = newly_matched + pat_idx;

        // replace current best match with a new match
        // even if the sizes are the same in order to prefer closer matches
        best = best.filter(|b| b.size > match_size).or_else(|| {
            Some(MoveBack::new(
                match_size,
                window_size - (target_idx - pat_idx),
            ))
        });

        // use some form of KMP to advance the window/target index
        let lps_idx = match_size.saturating_sub(1);

        if check_rl {
            // only jump to the start of the next subpattern
            // hopefully this will
            let nearest_miss = lps_partial_skip(&lps[..lps_idx]);

            target_idx += nearest_miss + 1;
            pat_idx = 0;
        } else {
            // the target/window is only guarenteed to advance if
            // there is not a prefix/suffix match in the target (e.g., if pat_idx is 0)
            let advance = if pat_idx == 0 {
                match_size.max(1)
            } else {
                newly_matched
            };
            target_idx += advance;
            pat_idx = lps.get(lps_idx).copied().unwrap_or(0);
        }
    }

    best
}

/// Only move to the nearest zero to not skip internal partial matches...?
fn lps_partial_skip(limited: &[usize]) -> usize {
    limited
        .iter()
        .enumerate()
        .rev()
        .find(|(_, &x)| x == 0)
        .map(|(i, _)| i)
        .unwrap_or(0)
}

// Longest Proper prefix which is Suffix
fn compute_lps(pattern: &[u8]) -> Box<[usize]> {
    let mut lps = vec![0; pattern.len()];
    let mut prefix_idx = 0;

    for (i, &ch) in pattern.iter().enumerate().skip(1) {
        while prefix_idx > 0 && ch != pattern[prefix_idx] {
            prefix_idx = lps[prefix_idx - 1];
        }

        if pattern[prefix_idx] == ch {
            prefix_idx += 1;
            lps[i] = prefix_idx
        }
    }

    lps.into()
}
