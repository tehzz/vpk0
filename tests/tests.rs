use std::io::Cursor;
use vpk0::LzssBackend::{self, *};

const LOGO: &[u8] = include_bytes!("small-logo.png");
const BACKENDS: &[LzssBackend] = &[Brute, Kmp, KmpAhead];
const VPK_METHOD0: &[u8] = include_bytes!("method0.vpk0");
const RAW_METHOD0: &[u8] = include_bytes!("method0-orig.bin");
const VPK_METHOD1: &[u8] = include_bytes!("method1.vpk0");
const RAW_METHOD1: &[u8] = include_bytes!("method1-orig.bin");
const NOREPT_MESG: &[u8] = b"abcdefgh12345";

#[test]
fn decode_method0() {
    let decoded = vpk0::Decoder::for_bytes(VPK_METHOD0)
        .decode()
        .expect("working decode");

    assert_eq!(decoded, RAW_METHOD0, "decoding method 0");
}

#[test]
fn decode_method0_bytes() {
    let decoded = vpk0::decode_bytes(VPK_METHOD0).expect("working decode");

    assert_eq!(decoded, RAW_METHOD0, "decoding method 0");
}

#[test]
fn encode_method0() {
    for &backend in BACKENDS {
        vpk0::Encoder::for_bytes(LOGO)
            .one_sample()
            .lzss_backend(backend)
            .encode_to_vec()
            .expect(&format!("valid encode for {:?}", backend));
    }
}

#[test]
fn encode_method0_bytes() {
    vpk0::encode_bytes(LOGO).expect("encode bytes helper function works");
}

#[test]
fn encode_method0_no_matching() {
    for &backend in BACKENDS {
        let compressed = vpk0::Encoder::for_bytes(NOREPT_MESG)
            .one_sample()
            .lzss_backend(backend)
            .encode_to_vec()
            .expect(&format!("encoding no match data for {:?}", backend));

        let mut decoder = vpk0::Decoder::for_bytes(&compressed);

        let trees = decoder.trees().expect("valid zero length VPK trees");
        assert_eq!(trees.lengths, "()", "Expected empty length tree");
        assert_eq!(trees.offsets, "()", "Expected empty offset tree");

        let decompressed = decoder.decode().expect("decoding of empty tree vpk0 file");

        assert_eq!(
            NOREPT_MESG, decompressed,
            "Expected encoded-decoded data to match original data"
        );
    }
}

#[test]
fn match_method0() {
    let (_header, trees) = vpk0::vpk_info(Cursor::new(VPK_METHOD0)).unwrap();

    let compressed = vpk0::Encoder::for_bytes(RAW_METHOD0)
        .one_sample()
        .lzss_backend(Brute)
        .with_lengths(&trees.lengths)
        .with_offsets(&trees.offsets)
        .encode_to_vec()
        .unwrap();

    assert_eq!(compressed, VPK_METHOD0);
}

#[test]
fn decode_method1() {
    let mut reader = Cursor::new(VPK_METHOD1);
    let decoded = vpk0::decode(&mut reader).unwrap();

    assert_eq!(decoded, RAW_METHOD1, "error method 1");
}

#[test]
fn decode_method1_bytes() {
    let decoded = vpk0::decode_bytes(VPK_METHOD1).unwrap();

    assert_eq!(decoded, RAW_METHOD1, "error method 1");
}

#[test]
fn encode_method1() {
    for &backend in BACKENDS {
        vpk0::Encoder::for_bytes(LOGO)
            .two_sample()
            .lzss_backend(backend)
            .encode_to_vec()
            .expect(&format!("valid encode for {:?}", backend));
    }
}

#[test]
fn encode_method1_no_matching() {
    for &backend in BACKENDS {
        let compressed = vpk0::Encoder::for_bytes(NOREPT_MESG)
            .two_sample()
            .lzss_backend(backend)
            .encode_to_vec()
            .expect(&format!("encoding no match data for {:?}", backend));

        let mut decoder = vpk0::Decoder::for_bytes(&compressed);

        let trees = decoder.trees().expect("valid zero length VPK trees");
        assert_eq!(trees.lengths, "()", "Expected empty length tree");
        assert_eq!(trees.offsets, "()", "Expected empty offset tree");

        let decompressed = decoder.decode().expect("decoding of empty tree vpk0 file");

        assert_eq!(
            NOREPT_MESG, decompressed,
            "Expected encoded-decoded data to match original data"
        );
    }
}

#[test]
fn match_method1() {
    let (_header, trees) = vpk0::vpk_info(Cursor::new(VPK_METHOD1)).unwrap();

    let compressed = vpk0::Encoder::for_bytes(RAW_METHOD1)
        .two_sample()
        .lzss_backend(Brute)
        .with_lengths(&trees.lengths)
        .with_offsets(&trees.offsets)
        .encode_to_vec()
        .unwrap();

    assert_eq!(compressed, VPK_METHOD1);
}

#[test]
fn decode_bad_file() {
    let bad_file = include_bytes!("bad-file.vpk0");
    let mut bf_r = Cursor::new(bad_file.as_ref());

    match vpk0::decode(&mut bf_r) {
        Ok(result) => {
            let err = "Expected error when decoding bad file";
            eprintln!("{}", err);
            eprintln!("{:?}", result);
            panic!("Expected error when decoding bad file");
        }
        Err(err) => {
            eprintln!("{}", err);
            assert!(true)
        }
    };
}
