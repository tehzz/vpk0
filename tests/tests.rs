use std::io::Cursor;
use std::str::from_utf8;

#[test]
fn decode_method0() {
    let lorem_text = include_str!("lorem.txt");
    let lorem_vpk0 = include_bytes!("lorem.vpk0");

    let mut lorem_reader = Cursor::new(lorem_vpk0.as_ref());

    let decoded = vpk0::decode(&mut lorem_reader).unwrap();
    let decoded_str = from_utf8(&decoded).unwrap();

    assert_eq!(decoded_str, lorem_text);
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

#[test]
fn decode_method1() {
    let uncompressed = include_bytes!("file39.bin");
    let compressed = include_bytes!("file39-raw.vpk0");

    let mut reader = Cursor::new(compressed.as_ref());
    let decoded = vpk0::decode(&mut reader).unwrap();

    assert_eq!(
        uncompressed.as_ref(),
        decoded.as_slice(),
        "error decoding file 39"
    );
}
