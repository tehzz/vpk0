extern crate vpk0;

use std::io::Cursor;
use std::str::from_utf8;

#[test]
fn decode_tests() {
    let lorem_text = include_str!("lorem.txt");
    let lorem_vpk0 = include_bytes!("lorem.vpk0");

    let mut lorem_reader = Cursor::new(lorem_vpk0.as_ref());

    let decoded = vpk0::decode(&mut lorem_reader).unwrap();
    let decoded_str = from_utf8(&decoded).unwrap();

    assert_eq!(decoded_str, lorem_text);
}
