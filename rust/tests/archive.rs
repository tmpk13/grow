//! The zip the sprite editor hands out has to be one an ordinary unzip opens,
//! so this checks it against the format rather than against itself.

use grow::zip::{crc32, from_base64, Zip};

#[test]
fn the_crc_matches_the_one_every_other_tool_computes() {
    // The check value the standard states for this input.
    assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    assert_eq!(crc32(b""), 0);
}

#[test]
fn an_archive_has_a_header_a_directory_and_an_end() {
    let mut zip = Zip::new();
    assert!(zip.is_empty());
    zip.add("walk.png", b"the pixels");
    zip.add("stand.png", b"more pixels");
    assert_eq!(zip.len(), 2);
    let out = zip.finish();

    assert_eq!(&out[..4], b"PK\x03\x04", "it should start with a local file header");
    assert_eq!(&out[out.len() - 22..out.len() - 18], b"PK\x05\x06", "and end with the end record");

    // Two files, said twice, at the end.
    let n = u16::from_le_bytes([out[out.len() - 14], out[out.len() - 13]]);
    assert_eq!(n, 2);

    // The directory it points at has to be inside the file and start where it
    // says it does.
    let size = u32::from_le_bytes(out[out.len() - 10..out.len() - 6].try_into().unwrap()) as usize;
    let at = u32::from_le_bytes(out[out.len() - 6..out.len() - 2].try_into().unwrap()) as usize;
    assert_eq!(at + size, out.len() - 22, "the directory should run up to the end record");
    assert_eq!(&out[at..at + 4], b"PK\x01\x02");
}

#[test]
fn a_file_carries_its_own_name_size_and_crc() {
    let data = b"the pixels".to_vec();
    let mut zip = Zip::new();
    zip.add("walk.png", &data);
    let out = zip.finish();

    let crc = u32::from_le_bytes(out[14..18].try_into().unwrap());
    assert_eq!(crc, crc32(&data));
    // Stored, so both sizes are the length of the data.
    assert_eq!(u32::from_le_bytes(out[18..22].try_into().unwrap()), data.len() as u32);
    assert_eq!(u32::from_le_bytes(out[22..26].try_into().unwrap()), data.len() as u32);
    let name_len = u16::from_le_bytes(out[26..28].try_into().unwrap()) as usize;
    assert_eq!(&out[30..30 + name_len], b"walk.png");
    assert_eq!(&out[30 + name_len..30 + name_len + data.len()], &data[..]);
}

#[test]
fn an_empty_archive_is_still_a_valid_one() {
    let out = Zip::new().finish();
    assert_eq!(out.len(), 22, "nothing but the end record");
    assert_eq!(&out[..4], b"PK\x05\x06");
}

#[test]
fn the_same_files_twice_give_the_same_bytes() {
    let build = || {
        let mut zip = Zip::new();
        zip.add("a.png", b"one");
        zip.add("b.png", b"two");
        zip.finish()
    };
    assert_eq!(build(), build(), "nothing in an archive should carry a clock");
}

#[test]
fn base64_comes_back_as_the_bytes_that_went_in() {
    // What a data URL from a canvas looks like: standard alphabet, padded.
    assert_eq!(from_base64("aGVsbG8=").as_deref(), Some(&b"hello"[..]));
    assert_eq!(from_base64("aGVsbG8h").as_deref(), Some(&b"hello!"[..]));
    assert_eq!(from_base64("").as_deref(), Some(&b""[..]));
    // A PNG starts with a fixed signature, which is what a bad decode breaks.
    let png = from_base64("iVBORw0KGgo=").expect("the png signature");
    assert_eq!(png, vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    assert!(from_base64("not base64!").is_none());
}

#[test]
fn every_byte_survives_the_round_trip() {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = (0..=255u8).collect();
    // Encode the long way round, so the test does not lean on code under test.
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let mut n = 0u32;
        for i in 0..3 {
            n = (n << 8) | *chunk.get(i).unwrap_or(&0) as u32;
        }
        for i in 0..4 {
            if i <= chunk.len() {
                text.push(ALPHABET[((n >> (18 - i * 6)) & 63) as usize] as char);
            } else {
                text.push('=');
            }
        }
    }
    assert_eq!(from_base64(&text).as_deref(), Some(&bytes[..]));
}
