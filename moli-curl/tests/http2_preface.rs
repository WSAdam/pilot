//! pilot's HTTP/2 connection preface is Chrome's. Bot walls fingerprint it
//! (Akamai's `SETTINGS|WINDOW_UPDATE|PRIORITY|pseudo-header order`; Chrome
//! is `1:65536;2:0;4:6291456;6:262144|15663105|0|m,a,s,p`). The values come
//! from pilot's vendored libcurl (vendor/curl-sys, lib/http2.c).

use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::Arc,
    thread,
};

use curl::easy::{Easy2, Handler, HttpVersion, List, WriteError};
use rustls::{
    ServerConfig, ServerConnection, StreamOwned,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
};

const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
const SETTINGS: u8 = 0x4;
const WINDOW_UPDATE: u8 = 0x8;
const HEADERS: u8 = 0x1;
const PRIORITY: u8 = 0x2;
const FLAG_PADDED: u8 = 0x8;
const FLAG_PRIORITY: u8 = 0x20;

struct Discard;

impl Handler for Discard {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        Ok(data.len())
    }
}

#[derive(Debug)]
struct Frame {
    kind: u8,
    flags: u8,
    stream: u32,
    payload: Vec<u8>,
}

fn read_frame(stream: &mut impl Read) -> Frame {
    let mut header = [0_u8; 9];
    stream.read_exact(&mut header).unwrap();
    let length =
        usize::from(header[0]) << 16 | usize::from(header[1]) << 8 | usize::from(header[2]);
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload).unwrap();
    Frame {
        kind: header[3],
        flags: header[4],
        stream: u32::from_be_bytes([header[5], header[6], header[7], header[8]]) & 0x7fff_ffff,
        payload,
    }
}

/// An HPACK integer with an `prefix`-bit prefix (RFC 7541 5.1).
fn integer(block: &mut &[u8], prefix: u32) -> usize {
    let mask = (1_usize << prefix) - 1;
    let mut value = usize::from(block[0]) & mask;
    *block = &block[1..];
    if value < mask {
        return value;
    }
    let mut shift = 0;
    loop {
        let byte = block[0];
        *block = &block[1..];
        value += (usize::from(byte) & 0x7f) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            return value;
        }
    }
}

/// The pseudo-headers at the start of a HEADERS block, in order. Every
/// pseudo-header name is in HPACK's static table, so no Huffman decoding is
/// needed to read them.
fn pseudo_headers(mut block: &[u8]) -> Vec<&'static str> {
    let mut names = Vec::new();
    while !block.is_empty() {
        let first = block[0];
        let index = if first & 0x80 != 0 {
            integer(&mut block, 7)
        } else if first & 0xe0 == 0x20 {
            integer(&mut block, 5);
            continue;
        } else {
            let index = integer(&mut block, if first & 0x40 != 0 { 6 } else { 4 });
            let value_length = integer(&mut block, 7);
            block = &block[value_length..];
            index
        };
        match index {
            1 => names.push(":authority"),
            2 | 3 => names.push(":method"),
            4 | 5 => names.push(":path"),
            6 | 7 => names.push(":scheme"),
            _ => break,
        }
    }
    names
}

/// What curl sends on a new HTTP/2 connection, up to its first request.
struct Preface {
    settings: Vec<(u16, u32)>,
    window_update: Option<u32>,
    priority_frames: usize,
    pseudo_headers: Vec<&'static str>,
}

fn capture_preface() -> Preface {
    let certificate = rcgen::generate_simple_self_signed(vec!["pilot.test".to_owned()]).unwrap();
    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(certificate.cert.der().to_vec())],
            PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
                certificate.key_pair.serialize_der(),
            )),
        )
        .unwrap();
    config.alpn_protocols = vec![b"h2".to_vec()];
    let config = Arc::new(config);
    let ca = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(ca.path(), certificate.cert.pem()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut tls = StreamOwned::new(ServerConnection::new(config).unwrap(), stream);
        let mut preface = [0_u8; PREFACE.len()];
        tls.read_exact(&mut preface).unwrap();
        assert_eq!(preface, PREFACE, "the connection speaks HTTP/2");
        let mut captured = Preface {
            settings: Vec::new(),
            window_update: None,
            priority_frames: 0,
            pseudo_headers: Vec::new(),
        };
        loop {
            let frame = read_frame(&mut tls);
            match frame.kind {
                SETTINGS if frame.stream == 0 && frame.flags & 0x1 == 0 => {
                    captured.settings = frame
                        .payload
                        .chunks(6)
                        .map(|entry| {
                            (
                                u16::from_be_bytes([entry[0], entry[1]]),
                                u32::from_be_bytes([entry[2], entry[3], entry[4], entry[5]]),
                            )
                        })
                        .collect();
                }
                WINDOW_UPDATE if frame.stream == 0 => {
                    captured.window_update = Some(
                        u32::from_be_bytes(frame.payload[..4].try_into().unwrap()) & 0x7fff_ffff,
                    );
                }
                PRIORITY => captured.priority_frames += 1,
                HEADERS => {
                    let mut block = frame.payload.as_slice();
                    if frame.flags & FLAG_PADDED != 0 {
                        let padding = usize::from(block[0]);
                        block = &block[1..block.len() - padding];
                    }
                    if frame.flags & FLAG_PRIORITY != 0 {
                        block = &block[5..];
                    }
                    captured.pseudo_headers = pseudo_headers(block);
                    break;
                }
                _ => {}
            }
        }
        // Answer with an empty SETTINGS and a GOAWAY so curl stops cleanly.
        tls.write_all(&[0, 0, 0, SETTINGS, 0, 0, 0, 0, 0]).unwrap();
        tls.write_all(&[0, 0, 8, 0x7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
            .unwrap();
        tls.flush().unwrap();
        captured
    });
    let mut easy = Easy2::new(Discard);
    let mut resolve = List::new();
    resolve
        .append(&format!("pilot.test:{port}:127.0.0.1"))
        .unwrap();
    easy.resolve(resolve).unwrap();
    easy.url(&format!("https://pilot.test:{port}/")).unwrap();
    easy.http_version(HttpVersion::V2TLS).unwrap();
    easy.cainfo(ca.path()).unwrap();
    // The server hangs up after the request's HEADERS; only the preface
    // matters here.
    let _ = easy.perform();
    server.join().unwrap()
}

#[test]
fn the_http2_settings_are_chromes_in_chromes_order() {
    let preface = capture_preface();
    assert_eq!(
        preface.settings,
        [(1, 65_536), (2, 0), (4, 6_291_456), (6, 262_144)],
        "HEADER_TABLE_SIZE, ENABLE_PUSH, INITIAL_WINDOW_SIZE, MAX_HEADER_LIST_SIZE"
    );
}

#[test]
fn the_connection_window_grows_by_chromes_increment_without_priority_frames() {
    let preface = capture_preface();
    assert_eq!(preface.window_update, Some(15_663_105));
    assert_eq!(preface.priority_frames, 0);
}

#[test]
fn a_request_sends_its_pseudo_headers_in_chromes_order() {
    let preface = capture_preface();
    assert_eq!(
        preface.pseudo_headers,
        [":method", ":authority", ":scheme", ":path"]
    );
}
