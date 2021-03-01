//! A Rust library for handling Nintendo's N64-era `vpk0` data compression
//! 
//! `vpk0` is data compression scheme built on LZSS with Huffman coding for the 
//! dictionary offsets and match lengths. 
//! It was used at HAL Laboratory for three of their N64 games and later, 
//! for encoding the data on GBA E-Reader cards. 
//! 
//! This crate provides decoding and encoding for `vpk0` files. 
//! As it is an old scheme, the decoding and encoding is designed to match what 
//! Nintendo did in the 1990s; the crate does not focus on compression ratio or speed.
//! 
//! ## Usage
//! The [`decode()`] and [`encode()`] functions provide quick ways to deal with 
//! `vpk0` data.
//! 
//! ```
//! use vpk0::{encode, decode};
//! use std::io::Cursor;
//! 
//! # let data = &b"AAAVVVAAABABACCCDE"[..];
//! let raw = Cursor::new(data);
//! let compressed = encode(raw).unwrap();
//! let decompressed = decode(Cursor::new(&compressed)).unwrap();
//! assert_eq!(&data, &decompressed);
//! ```
//! 
//! For more control, you can use [`DecoderBuilder`] or [`EncoderBuilder`]:
//! 
//! ```
//! use vpk0::EncoderBuilder;
//! 
//! EncoderBuilder::for_bytes(b"ababacdcdeaba")
//!     .two_sample()
//!     .encode_to_writer(std::io::stdout())
//!     .unwrap();
//! ```
//! --------------------------------------------------------------------------------
//! ## `vpk0` Background
//! The `vpk0` format—named for the magic bytes—is thought to have been developed 
//! at [HAL Laboratories] in the late 1990s. Three of their N64 games use the compression scheme:
//! * Super Smash Bros.
//! * Pokémon Snap
//! * Shigesato Itoi's No. 1 Bass Fishing: Definitive Edition
//! 
//! The format next appeared in the mid-2000s as the compression used in the [Nintendo e-Reader]
//! for the GBA. This is where the format first received attention from the internet at large. 
//! Tim Schuerewegen’s [nevpk] and Caitsith’s [NVPK Tool and NEDEC Make] were 
//! open source implementations of `vpk0` that came from reverse engineering the e-Reader. 
//! 
//! This crate extends on their work to provide matching compression for HAL’s N64 titles.
//! 
//! ## Format Overview
//! `vpk0` is based on two fundamental encoding algorithms: LSZZ and Huffman Coding. 
//! The two techniques were bouncing around [the Japanese BSSes since the late 80s], 
//! and together they comprise the backbone of many modern day encoding schemes like [Deflate].
//! 
//! The `vpk0` format is comparatively simpler: it is a variable length LZSS. 
//! The input data is compressed by a standard LZSS implementation. 
//! But instead of having fixed bit sizes for the dictionary offset and length, the sizes are variable. 
//! The variable bit sizes are then encoded as a Huffman code, with the necessary 
//! [Huffman tree](format::TreeInfo) prepended to the encoded data. 
//! 
//! For more info, see [the documentation for the format module](crate::format)
//! 
//! ## Implementation Details
//! This implementation is designed to be a byte-perfect match of the encoder used 
//! for *Super Smash Bros.* As of March 2021, the LZSS encoder is byte-matching, 
//! but the Huffman compression is not. 
//! 
//! The matching LZSS encoding scheme is: after a found match, look ahead at the next byte 
//! to see if there is a longer match. Continue checking the next byte until 
//! a smaller or no match is found. 
//! 
//! The encoder in this crate checks at most the next ten bytes, 
//! as that was the maximum number necessary to match all 500 `vpk0` encoded files in *SSB64*. 
//! In the future, this parameter may become another option for [`EncoderBuilder`].
//! 
//! ## Advanced Usages
//! ### Getting info from a `vpk0` file
//! ```
//! use vpk0::vpk_info;
//! # use vpk0::EncoderBuilder;
//! # let vpkfile = EncoderBuilder::for_bytes(b"abababababacaaa").encode_to_vec().unwrap();
//! # let vpkfile = std::io::Cursor::new(vpkfile);
//! let (header, trees) = vpk_info(vpkfile).unwrap();
//! println!("Original size: {} bytes", header.size);
//! println!("VPK encoded with method {}", header.method);
//! println!("Offsets: {} || Lengths: {}", trees.offsets, trees.lengths);
//! ```
//! 
//! ### Encode like a standard LZSS
//! ```
//! use vpk0::{EncoderBuilder, LzssSettings};
//! // use fixed length compression by setting the offset to 10 and the length to 6.
//! let compressed = EncoderBuilder::for_bytes(b"I am Sam. Sam I am.")
//!     .one_sample()
//!     .with_lzss_settings(LzssSettings::new(10, 6, 2))
//!     .with_offsets("10")
//!     .with_lengths("6")
//!     .encode_to_vec();
//! ```
//! 
//! [HAL Laboratories]: https://www.hallab.co.jp/eng/
//! [Nintendo e-Reader]: https://en.m.wikipedia.org/wiki/Nintendo_e-Reader
//! [nevpk]: http://users.skynet.be/firefly/gba/e-reader/tools/index.htm
//! [NVPK Tool and NEDEC Make]: https://caitsith2.com/ereader/devtools.htm
//! [the Japanese BSSes since the late 80s]: https://web.archive.org/web/20160110174426/https://oku.edu.mie-u.ac.jp/~okumura/compression/history.html
//! [Deflate]: https://en.m.wikipedia.org/wiki/Deflate

mod decode;
mod encode;
pub mod errors;
pub mod format;

pub use decode::{decode, vpk_info, DecoderBuilder};
pub use encode::{lzss::LzssSettings, encode, EncoderBuilder, LzssBackend};
