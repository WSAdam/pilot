//! pilot: a TLS ClientHello shaped like Chrome's.
//!
//! Sites behind bot defenses fingerprint the ClientHello (JA3/JA4). libcurl's
//! own hello offers 20 cipher suites and 10 extensions, which reads as a
//! script, so those sites refuse pilot (Reuters 401, Etsy 403). Every
//! connection's SSL_CTX gets Chrome's cipher suites, groups and signature
//! algorithms, and the BoringSSL features Chrome uses through AWS-LC: GREASE,
//! extension permutation, brotli certificate compression, OCSP and SCT
//! requests, session tickets, and (per connection) ALPS and ECH GREASE.

use std::{
    ffi::{c_char, c_int, c_void},
    ptr,
};

use anyhow::{Result, bail};
use curl::easy::{Easy2, Handler};

// TLS 1.2 suites in Chrome's order; AWS-LC puts its TLS 1.3 suites first, in
// the same order Chrome uses.
const CHROME_TLS12_CIPHERS: &[u8] = b"ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:\
ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-CHACHA20-POLY1305:\
ECDHE-RSA-CHACHA20-POLY1305:ECDHE-RSA-AES128-SHA:ECDHE-RSA-AES256-SHA:AES128-GCM-SHA256:\
AES256-GCM-SHA384:AES128-SHA:AES256-SHA\0";
const CHROME_GROUPS: &[u8] = b"X25519MLKEM768:X25519:P-256:P-384\0";
const CHROME_SIGNATURE_ALGORITHMS: [u16; 8] = [
    0x0403, 0x0804, 0x0401, 0x0503, 0x0805, 0x0501, 0x0806, 0x0601,
];
const TLSEXT_CERT_COMPRESSION_BROTLI: u16 = 2;
const SSL_OP_NO_TICKET: u32 = 0x0000_4000;
const SSL_OP_IGNORE_UNEXPECTED_EOF: u32 = 0x0000_0080;
const SSL_CB_HANDSHAKE_START: c_int = 0x10;
const ALPN_H2: &[u8] = b"h2";

#[repr(C)]
struct SslCtx {
    _opaque: [u8; 0],
}
#[repr(C)]
struct Ssl {
    _opaque: [u8; 0],
}
#[repr(C)]
struct CryptoBuffer {
    _opaque: [u8; 0],
}

type CertDecompress = unsafe extern "C" fn(
    ssl: *mut Ssl,
    out: *mut *mut CryptoBuffer,
    uncompressed_len: usize,
    input: *const u8,
    input_len: usize,
) -> c_int;
type InfoCallback = unsafe extern "C" fn(ssl: *const Ssl, kind: c_int, value: c_int);

// AWS-LC, linked through openssl-sys. Its libcrypto symbols carry the
// release prefix; an AWS-LC upgrade shows up as a link error here.
unsafe extern "C" {
    fn SSL_CTX_set_strict_cipher_list(ctx: *mut SslCtx, list: *const c_char) -> c_int;
    fn SSL_CTX_set1_groups_list(ctx: *mut SslCtx, list: *const c_char) -> c_int;
    fn SSL_CTX_set_verify_algorithm_prefs(ctx: *mut SslCtx, prefs: *const u16, len: usize)
    -> c_int;
    fn SSL_CTX_set_grease_enabled(ctx: *mut SslCtx, enabled: c_int);
    fn SSL_CTX_set_permute_extensions(ctx: *mut SslCtx, enabled: c_int);
    fn SSL_CTX_enable_ocsp_stapling(ctx: *mut SslCtx);
    fn SSL_CTX_enable_signed_cert_timestamps(ctx: *mut SslCtx);
    fn SSL_CTX_clear_options(ctx: *mut SslCtx, options: u32) -> u32;
    fn SSL_CTX_set_options(ctx: *mut SslCtx, options: u32) -> u32;
    fn SSL_CTX_add_cert_compression_alg(
        ctx: *mut SslCtx,
        alg_id: u16,
        compress: *const c_void,
        decompress: Option<CertDecompress>,
    ) -> c_int;
    fn SSL_CTX_set_info_callback(ctx: *mut SslCtx, callback: Option<InfoCallback>);
    fn SSL_add_application_settings(
        ssl: *mut Ssl,
        proto: *const u8,
        proto_len: usize,
        settings: *const u8,
        settings_len: usize,
    ) -> c_int;
    fn SSL_set_alps_use_new_codepoint(ssl: *mut Ssl, use_new: c_int);
    fn SSL_set_enable_ech_grease(ssl: *mut Ssl, enable: c_int);
    #[link_name = "aws_lc_0_46_0_CRYPTO_BUFFER_alloc"]
    fn CRYPTO_BUFFER_alloc(out_data: *mut *mut u8, len: usize) -> *mut CryptoBuffer;
    #[link_name = "aws_lc_0_46_0_CRYPTO_BUFFER_free"]
    fn CRYPTO_BUFFER_free(buffer: *mut CryptoBuffer);
}

pub(crate) fn configure_chrome_client_hello<H: Handler>(easy: &mut Easy2<H>) -> Result<()> {
    let callback: unsafe extern "C" fn(
        *mut curl_sys::CURL,
        *mut c_void,
        *mut c_void,
    ) -> curl_sys::CURLcode = chrome_ssl_ctx;
    // SAFETY: CURLOPT_SSL_CTX_FUNCTION takes a function pointer with curl's
    // ssl_ctx_callback signature, which `chrome_ssl_ctx` has.
    let code = unsafe {
        curl_sys::curl_easy_setopt(easy.raw(), curl_sys::CURLOPT_SSL_CTX_FUNCTION, callback)
    };
    if code != curl_sys::CURLE_OK {
        bail!("failed to install pilot's Chrome TLS profile (curl error {code})");
    }
    Ok(())
}

/// curl's CURLOPT_SSL_CTX_FUNCTION: shapes each connection's SSL_CTX before
/// its ClientHello is built.
unsafe extern "C" fn chrome_ssl_ctx(
    _curl: *mut curl_sys::CURL,
    ssl_ctx: *mut c_void,
    _data: *mut c_void,
) -> curl_sys::CURLcode {
    let ctx = ssl_ctx.cast::<SslCtx>();
    // SAFETY: curl passes the SSL_CTX it is about to use; every call below
    // only configures it.
    let configured = unsafe {
        SSL_CTX_set_strict_cipher_list(ctx, CHROME_TLS12_CIPHERS.as_ptr().cast()) == 1
            && SSL_CTX_set1_groups_list(ctx, CHROME_GROUPS.as_ptr().cast()) == 1
            && SSL_CTX_set_verify_algorithm_prefs(
                ctx,
                CHROME_SIGNATURE_ALGORITHMS.as_ptr(),
                CHROME_SIGNATURE_ALGORITHMS.len(),
            ) == 1
    };
    if !configured {
        tracing::error!("❌ AWS-LC rejected pilot's Chrome TLS profile");
        return curl_sys::CURLE_SSL_CIPHER;
    }
    unsafe {
        SSL_CTX_set_grease_enabled(ctx, 1);
        SSL_CTX_set_permute_extensions(ctx, 1);
        SSL_CTX_enable_ocsp_stapling(ctx);
        SSL_CTX_enable_signed_cert_timestamps(ctx);
        SSL_CTX_clear_options(ctx, SSL_OP_NO_TICKET);
        // Like Chrome, a peer that closes TCP without a close_notify ends the
        // stream cleanly; HTTP framing decides whether the response is whole.
        SSL_CTX_set_options(ctx, SSL_OP_IGNORE_UNEXPECTED_EOF);
        // A reused context already has it; a second registration is refused.
        SSL_CTX_add_cert_compression_alg(
            ctx,
            TLSEXT_CERT_COMPRESSION_BROTLI,
            ptr::null(),
            Some(decompress_brotli_certificate),
        );
        SSL_CTX_set_info_callback(ctx, Some(at_handshake_start));
    }
    curl_sys::CURLE_OK
}

/// ALPS and ECH GREASE are per connection; set them as each handshake starts,
/// before AWS-LC writes the ClientHello.
unsafe extern "C" fn at_handshake_start(ssl: *const Ssl, kind: c_int, _value: c_int) {
    if kind & SSL_CB_HANDSHAKE_START == 0 {
        return;
    }
    let ssl = ssl.cast_mut();
    // SAFETY: AWS-LC calls this with the live SSL starting its handshake.
    unsafe {
        SSL_add_application_settings(ssl, ALPN_H2.as_ptr(), ALPN_H2.len(), ptr::null(), 0);
        SSL_set_alps_use_new_codepoint(ssl, 1);
        SSL_set_enable_ech_grease(ssl, 1);
    }
}

/// Decompresses a brotli-compressed server certificate (RFC 8879).
unsafe extern "C" fn decompress_brotli_certificate(
    _ssl: *mut Ssl,
    out: *mut *mut CryptoBuffer,
    uncompressed_len: usize,
    input: *const u8,
    input_len: usize,
) -> c_int {
    let mut data = ptr::null_mut();
    // SAFETY: AWS-LC owns the returned buffer once it is handed back in `out`.
    let buffer = unsafe { CRYPTO_BUFFER_alloc(&mut data, uncompressed_len) };
    if buffer.is_null() {
        return 0;
    }
    let mut decoded = uncompressed_len;
    // SAFETY: `data` has room for `uncompressed_len` bytes and `input` holds
    // `input_len` bytes, as AWS-LC promises.
    let result =
        unsafe { brotlic_sys::BrotliDecoderDecompress(input_len, input, &mut decoded, data) };
    if result != brotlic_sys::BrotliDecoderResult_BROTLI_DECODER_RESULT_SUCCESS
        || decoded != uncompressed_len
    {
        unsafe { CRYPTO_BUFFER_free(buffer) };
        return 0;
    }
    unsafe { *out = buffer };
    1
}

#[cfg(test)]
mod tests {
    use std::{io::Read, net::TcpListener, thread};

    use curl::easy::{Easy2, Handler, List};

    use super::*;

    struct Discard;
    impl Handler for Discard {}

    // Chrome 131+ on desktop.
    const CHROME_CIPHERS: [u16; 15] = [
        0x1301, 0x1302, 0x1303, 0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8, 0xc013, 0xc014,
        0x009c, 0x009d, 0x002f, 0x0035,
    ];
    const CHROME_GROUPS: [u16; 4] = [0x11ec, 0x001d, 0x0017, 0x0018];
    const CHROME_SIGNATURE_ALGORITHMS: [u16; 8] = [
        0x0403, 0x0804, 0x0401, 0x0503, 0x0805, 0x0501, 0x0806, 0x0601,
    ];
    // server_name, extended_master_secret, renegotiation_info,
    // supported_groups, ec_point_formats, session_ticket, ALPN,
    // status_request, signature_algorithms, signed_certificate_timestamp,
    // key_share, psk_key_exchange_modes, supported_versions,
    // compress_certificate, application_settings (new codepoint) and
    // encrypted_client_hello (GREASE).
    const CHROME_EXTENSIONS: [u16; 16] = [
        0, 23, 65281, 10, 11, 35, 16, 5, 13, 18, 51, 45, 43, 27, 17613, 65037,
    ];
    const BROTLI: u16 = 2;

    fn is_grease(value: u16) -> bool {
        value & 0x0f0f == 0x0a0a && value >> 8 == value & 0xff
    }

    struct ClientHello {
        ciphers: Vec<u16>,
        extensions: Vec<(u16, Vec<u8>)>,
    }

    fn u16_list(bytes: &[u8]) -> Vec<u16> {
        bytes
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect()
    }

    impl ClientHello {
        fn parse(record: &[u8]) -> Self {
            // Handshake header (type, 24-bit length), version, random.
            assert_eq!(record[0], 1, "a ClientHello handshake message");
            let mut at = 4 + 2 + 32;
            at += 1 + usize::from(record[at]);
            let cipher_len = usize::from(u16::from_be_bytes([record[at], record[at + 1]]));
            let ciphers = u16_list(&record[at + 2..at + 2 + cipher_len]);
            at += 2 + cipher_len;
            at += 1 + usize::from(record[at]);
            let extensions_end =
                at + 2 + usize::from(u16::from_be_bytes([record[at], record[at + 1]]));
            at += 2;
            let mut extensions = Vec::new();
            while at < extensions_end {
                let kind = u16::from_be_bytes([record[at], record[at + 1]]);
                let len = usize::from(u16::from_be_bytes([record[at + 2], record[at + 3]]));
                extensions.push((kind, record[at + 4..at + 4 + len].to_vec()));
                at += 4 + len;
            }
            Self {
                ciphers,
                extensions,
            }
        }

        fn extension(&self, kind: u16) -> &[u8] {
            &self
                .extensions
                .iter()
                .find(|(id, _)| *id == kind)
                .unwrap_or_else(|| panic!("extension {kind} is offered"))
                .1
        }

        fn ids(&self) -> Vec<u16> {
            self.extensions
                .iter()
                .map(|(id, _)| *id)
                .filter(|id| !is_grease(*id))
                .collect()
        }
    }

    /// Records the ClientHello a configured curl handle sends, then hangs up.
    fn capture_client_hello() -> ClientHello {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut header = [0_u8; 5];
            stream.read_exact(&mut header).unwrap();
            assert_eq!(header[0], 0x16, "a TLS handshake record");
            let mut record = vec![0; usize::from(u16::from_be_bytes([header[3], header[4]]))];
            stream.read_exact(&mut record).unwrap();
            record
        });
        let mut easy = Easy2::new(Discard);
        let mut resolve = List::new();
        resolve
            .append(&format!("pilot.test:{port}:127.0.0.1"))
            .unwrap();
        easy.resolve(resolve).unwrap();
        easy.url(&format!("https://pilot.test:{port}/")).unwrap();
        easy.http_version(curl::easy::HttpVersion::V2TLS).unwrap();
        configure_chrome_client_hello(&mut easy).unwrap();
        // The server hangs up after the hello, so the transfer fails.
        let _ = easy.perform();
        ClientHello::parse(&server.join().unwrap())
    }

    // 2026-11-03: libcurl's ClientHello (20 ciphers, 10 extensions, no
    // GREASE) fingerprinted pilot as a script; Reuters, Etsy and Craigslist
    // refused it.
    #[test]
    fn the_client_hello_offers_chromes_ciphers_groups_and_algorithms() {
        let hello = capture_client_hello();
        assert!(is_grease(hello.ciphers[0]), "a GREASE cipher comes first");
        assert_eq!(hello.ciphers[1..], CHROME_CIPHERS);

        let groups = u16_list(&hello.extension(10)[2..]);
        assert!(is_grease(groups[0]), "a GREASE group comes first");
        assert_eq!(groups[1..], CHROME_GROUPS);
        let key_share = hello.extension(51);
        let first_share = u16::from_be_bytes([key_share[2], key_share[3]]);
        assert!(is_grease(first_share), "a GREASE key share comes first");

        assert_eq!(
            u16_list(&hello.extension(13)[2..]),
            CHROME_SIGNATURE_ALGORITHMS
        );
        assert_eq!(u16_list(&hello.extension(27)[1..]), [BROTLI]);
    }

    #[test]
    fn brotli_compressed_certificates_decompress() {
        let certificate: Vec<u8> = (0..4096_u32).map(|i| (i % 251) as u8).collect();
        let mut compressed = vec![0_u8; certificate.len() + 64];
        let mut compressed_len = compressed.len();
        // SAFETY: both buffers are sized as passed.
        assert_eq!(
            unsafe {
                brotlic_sys::BrotliEncoderCompress(
                    9,
                    22,
                    brotlic_sys::BrotliEncoderMode_BROTLI_MODE_GENERIC,
                    certificate.len(),
                    certificate.as_ptr(),
                    &mut compressed_len,
                    compressed.as_mut_ptr(),
                )
            },
            1
        );
        let mut out = ptr::null_mut();
        // SAFETY: the inputs describe valid buffers; `out` receives a buffer
        // this test frees.
        let ok = unsafe {
            decompress_brotli_certificate(
                ptr::null_mut(),
                &mut out,
                certificate.len(),
                compressed.as_ptr(),
                compressed_len,
            )
        };
        assert_eq!(ok, 1);
        assert!(!out.is_null());
        unsafe { CRYPTO_BUFFER_free(out) };
        let wrong_length = unsafe {
            decompress_brotli_certificate(
                ptr::null_mut(),
                &mut out,
                certificate.len() + 1,
                compressed.as_ptr(),
                compressed_len,
            )
        };
        assert_eq!(wrong_length, 0, "a length mismatch is refused");
    }

    #[test]
    fn a_tls_server_still_completes_the_handshake_with_the_chrome_profile() {
        use std::{io::Write, sync::Arc};

        use rustls::{
            ServerConfig, ServerConnection, StreamOwned,
            pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
        };
        let certificate =
            rcgen::generate_simple_self_signed(vec!["pilot.test".to_owned()]).unwrap();
        let config = Arc::new(
            ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(
                    vec![CertificateDer::from(certificate.cert.der().to_vec())],
                    PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
                        certificate.key_pair.serialize_der(),
                    )),
                )
                .unwrap(),
        );
        let ca = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(ca.path(), certificate.cert.pem()).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut tls = StreamOwned::new(ServerConnection::new(config).unwrap(), stream);
            let mut request = [0_u8; 1024];
            let _ = tls.read(&mut request).unwrap();
            tls.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok")
                .unwrap();
            tls.flush().unwrap();
        });
        let mut easy = Easy2::new(Discard);
        let mut resolve = List::new();
        resolve
            .append(&format!("pilot.test:{port}:127.0.0.1"))
            .unwrap();
        easy.resolve(resolve).unwrap();
        easy.url(&format!("https://pilot.test:{port}/")).unwrap();
        easy.http_version(curl::easy::HttpVersion::V11).unwrap();
        easy.cainfo(ca.path()).unwrap();
        configure_chrome_client_hello(&mut easy).unwrap();
        easy.perform()
            .expect("the handshake completes and the request succeeds");
        assert_eq!(easy.response_code().unwrap(), 200);
        server.join().unwrap();
    }

    // 2026-11-03: Craigslist answers with a 302 that has no Content-Length,
    // then closes TCP without a TLS close_notify. curl reported error 56 and
    // the navigation failed; Chrome treats the close as the end of the body.
    #[test]
    fn a_close_delimited_response_ending_without_close_notify_completes() {
        use std::{io::Write, sync::Arc};

        use rustls::{
            ServerConfig, ServerConnection, StreamOwned,
            pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
        };
        let certificate =
            rcgen::generate_simple_self_signed(vec!["pilot.test".to_owned()]).unwrap();
        let config = Arc::new(
            ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(
                    vec![CertificateDer::from(certificate.cert.der().to_vec())],
                    PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
                        certificate.key_pair.serialize_der(),
                    )),
                )
                .unwrap(),
        );
        let ca = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(ca.path(), certificate.cert.pem()).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut tls = StreamOwned::new(ServerConnection::new(config).unwrap(), stream);
            let mut request = [0_u8; 1024];
            let _ = tls.read(&mut request).unwrap();
            tls.write_all(b"HTTP/1.1 302 Found\r\nLocation: https://pilot.test/next\r\n\r\n")
                .unwrap();
            tls.flush().unwrap();
            // Hang up without a close_notify, as Craigslist does.
            tls.sock.shutdown(std::net::Shutdown::Both).unwrap();
        });
        let mut easy = Easy2::new(Discard);
        let mut resolve = List::new();
        resolve
            .append(&format!("pilot.test:{port}:127.0.0.1"))
            .unwrap();
        easy.resolve(resolve).unwrap();
        easy.url(&format!("https://pilot.test:{port}/")).unwrap();
        easy.http_version(curl::easy::HttpVersion::V11).unwrap();
        easy.cainfo(ca.path()).unwrap();
        configure_chrome_client_hello(&mut easy).unwrap();
        easy.perform()
            .expect("the close ends the body instead of failing the transfer");
        assert_eq!(easy.response_code().unwrap(), 302);
        server.join().unwrap();
    }

    #[test]
    fn the_client_hello_carries_chromes_extensions_in_a_shuffled_order() {
        let first = capture_client_hello();
        let mut offered = first.ids();
        offered.sort_unstable();
        let mut expected = CHROME_EXTENSIONS.to_vec();
        expected.sort_unstable();
        assert_eq!(offered, expected, "Chrome's extensions, besides GREASE");
        let grease = first
            .extensions
            .iter()
            .filter(|(id, _)| is_grease(*id))
            .count();
        assert_eq!(grease, 2, "two GREASE extensions");

        // Chrome permutes its extensions on every connection.
        let orders: Vec<Vec<u16>> = (0..3).map(|_| capture_client_hello().ids()).collect();
        assert!(
            orders.iter().any(|order| *order != first.ids()),
            "the extension order changes between connections"
        );
    }
}
