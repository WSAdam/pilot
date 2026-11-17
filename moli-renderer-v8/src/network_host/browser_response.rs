use super::*;
use moli_web_mime::data_url_body_and_mime_type;
use std::fmt;

#[derive(Debug)]
pub(crate) enum LocalUrlError {
    BlobMethod { method: String },
    BlobUnavailable { url: url::Url },
    InvalidData { url: url::Url },
}

impl fmt::Display for LocalUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlobMethod { method } => write!(f, "blob URL fetch requires GET, got `{method}`"),
            Self::BlobUnavailable { url } => write!(f, "blob URL `{url}` is unavailable"),
            Self::InvalidData { url } => write!(f, "data URL `{url}` is invalid"),
        }
    }
}

impl std::error::Error for LocalUrlError {}

pub(crate) fn blob_url_response(url: &url::Url) -> Option<Response> {
    let (body_bytes, mime_type) = blob::object_url_bytes_and_type(url.as_str())?;
    let headers = moli_fetch::headers_from_byte_strings(&[
        ("Content-Length".to_owned(), body_bytes.len().to_string()),
        ("Content-Type".to_owned(), mime_type),
    ])
    .expect("Blob response headers contain ByteStrings");
    Some(Response::from_head_and_lossy_body_bytes(
        moli_fetch::ResponseHead {
            status_text: None,
            final_url: url.clone(),
            status: 200,
            headers,
            request_cookie_report: None,
            cookie_set_reports: Vec::new(),
            redirected: false,
            redirect_chain: Vec::new(),
            from_cache: false,
            cache_state: Default::default(),
            negotiated_http_version: None,
        },
        body_bytes,
    ))
}

pub(crate) fn data_url_response(url: &url::Url) -> Option<Response> {
    let (body_bytes, mime_type) = data_url_body_and_mime_type(url.as_str())?;
    Some(Response::from_head_and_lossy_body_bytes(
        moli_fetch::ResponseHead {
            status_text: None,
            final_url: url.clone(),
            status: 200,
            headers: vec![(
                "Content-Type".to_owned(),
                moli_fetch::header_value_from_byte_string(&mime_type)
                    .expect("serialized MIME types contain ByteStrings"),
            )],
            request_cookie_report: None,
            cookie_set_reports: Vec::new(),
            redirected: false,
            redirect_chain: Vec::new(),
            from_cache: false,
            cache_state: Default::default(),
            negotiated_http_version: None,
        },
        body_bytes,
    ))
}

/// pilot: the largest local file a file:// document may load as a
/// subresource, the same 64 MiB limit as a file:// document itself.
const LOCAL_FILE_SUBRESOURCE_LIMIT: u64 = 64 * 1024 * 1024;

/// pilot: reads a file:// subresource from disk as a synthetic 200, the way
/// Chrome's file loader serves a file:// document's stylesheets, scripts and
/// images. Only regular files within the size limit are read (a directory,
/// FIFO or device is not a resource), typed from the extension, else sniffed.
/// The error is Chrome's net error text. The caller decides whether the
/// request may load a local file at all.
pub(crate) fn local_file_response(url: &url::Url) -> Result<Response, &'static str> {
    use std::io::Read as _;

    const NOT_FOUND: &str = "net::ERR_FILE_NOT_FOUND";
    const TOO_BIG: &str = "net::ERR_FILE_TOO_BIG";
    let path = url.to_file_path().map_err(|()| NOT_FOUND)?;
    let metadata = std::fs::metadata(&path).map_err(|_| NOT_FOUND)?;
    if !metadata.is_file() {
        return Err(NOT_FOUND);
    }
    if metadata.len() > LOCAL_FILE_SUBRESOURCE_LIMIT {
        return Err(TOO_BIG);
    }
    let mut body_bytes = Vec::new();
    std::fs::File::open(&path)
        .map_err(|_| "net::ERR_ACCESS_DENIED")?
        .take(LOCAL_FILE_SUBRESOURCE_LIMIT + 1)
        .read_to_end(&mut body_bytes)
        .map_err(|_| "net::ERR_FAILED")?;
    if body_bytes.len() as u64 > LOCAL_FILE_SUBRESOURCE_LIMIT {
        return Err(TOO_BIG);
    }
    let content_type = match moli_web_mime::selected_file_mime_type(url.path()) {
        "" => moli_web_mime::sniff_unknown_mime_type(&body_bytes, true),
        known => known.to_owned(),
    };
    tracing::debug!(url = %url, content_type = %content_type, "🔍 loaded a local file subresource");
    Ok(Response::from_head_and_lossy_body_bytes(
        moli_fetch::ResponseHead {
            status_text: None,
            final_url: url.clone(),
            status: 200,
            headers: vec![(
                "Content-Type".to_owned(),
                moli_fetch::header_value_from_byte_string(&content_type)
                    .expect("serialized MIME types contain ByteStrings"),
            )],
            request_cookie_report: None,
            cookie_set_reports: Vec::new(),
            redirected: false,
            redirect_chain: Vec::new(),
            from_cache: false,
            cache_state: Default::default(),
            negotiated_http_version: None,
        },
        body_bytes,
    ))
}

/// Reads a local resource for consumers whose requests always use GET.
pub(crate) fn local_url_response(url: &url::Url) -> Option<Response> {
    local_url_response_result(url, "GET").and_then(Result::ok)
}

/// Resolves renderer-owned URL schemes without falling through to the network
/// transport when the local resource is malformed, revoked, or unavailable.
///
/// `None` means that the URL is not owned by this resolver. `Some(Err(..))`
/// means that it is a local URL and therefore must fail locally instead of
/// being handed to libcurl.
/// `method` is the request's already normalized method.
pub(crate) fn local_url_response_result(
    url: &url::Url,
    method: &str,
) -> Option<Result<Response, LocalUrlError>> {
    match url.scheme() {
        "blob" if method != "GET" => Some(Err(LocalUrlError::BlobMethod {
            method: method.to_owned(),
        })),
        "blob" => Some(
            blob_url_response(url)
                .ok_or_else(|| LocalUrlError::BlobUnavailable { url: url.clone() }),
        ),
        "data" => Some(
            data_url_response(url).ok_or_else(|| LocalUrlError::InvalidData { url: url.clone() }),
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_url_response_decodes_plain_and_base64_payloads() {
        let plain = url::Url::parse("data:,BB-8").unwrap();
        let (_, body) = data_url_response(&plain).unwrap().into_body();
        let (text, _) = body.try_into_lossy_materialized_text().unwrap();
        assert_eq!(text, "BB-8");

        let base64 = url::Url::parse("data:text/plain;base64,Sy0yU08=").unwrap();
        let (_, body) = data_url_response(&base64).unwrap().into_body();
        let (text, _) = body.try_into_lossy_materialized_text().unwrap();
        assert_eq!(text, "K-2SO");
    }

    #[test]
    fn data_url_response_preserves_supplied_charset_parameter() {
        let url = url::Url::parse("data:text/html;charset=iso-2022-jp,hello").unwrap();
        let response = data_url_response(&url).unwrap();

        assert_eq!(
            response
                .head()
                .headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
                .map(|(_, value)| value.as_slice()),
            Some(b"text/html;charset=iso-2022-jp".as_slice())
        );
    }

    #[test]
    fn unavailable_blob_url_is_a_local_failure() {
        let url = url::Url::parse("blob:https://example.test/not-registered").unwrap();

        let error = local_url_response_result(&url, "GET")
            .expect("blob URL must be owned by the local resolver")
            .expect_err("an unregistered blob URL must fail locally");

        assert_eq!(
            error.to_string(),
            "blob URL `blob:https://example.test/not-registered` is unavailable"
        );
    }
}
