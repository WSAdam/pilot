//! pilot: what a navigation sends on the wire, in Chrome's header order.

use super::*;

/// `<p>hello</p>`, gzip-compressed.
const HELLO_GZIP: &[u8] = &[
    31, 139, 8, 0, 0, 0, 0, 0, 2, 255, 179, 41, 176, 203, 72, 205, 201, 201, 183, 209, 47, 176, 3,
    0, 68, 251, 174, 210, 12, 0, 0, 0,
];

/// Answers one request with `response`, and returns the request.
async fn serve_one(listener: TcpListener, response: Vec<u8>) -> Result<Vec<u8>> {
    let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept()).await??;
    let mut request = Vec::new();
    while !request.windows(4).any(|part| part == b"\r\n\r\n") {
        let mut chunk = [0; 1024];
        let count = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk)).await??;
        anyhow::ensure!(
            count != 0 && request.len() < 64 * 1024,
            "incomplete request"
        );
        request.extend_from_slice(&chunk[..count]);
    }
    stream.write_all(&response).await?;
    stream.shutdown().await?;
    Ok(request)
}

/// The request's header fields, in the order they were sent.
fn fields(request: &[u8]) -> Vec<(String, String)> {
    std::str::from_utf8(request)
        .unwrap()
        .split("\r\n")
        .skip(1)
        .take_while(|line| !line.is_empty())
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.to_ascii_lowercase(), value.trim().to_owned()))
        })
        .collect()
}

#[tokio::test]
async fn a_navigation_goes_out_in_chromes_header_order_and_still_decodes_gzip() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/page", listener.local_addr()?);
    let response = [
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            HELLO_GZIP.len()
        )
        .into_bytes(),
        HELLO_GZIP.to_vec(),
    ]
    .concat();
    let server = tokio::spawn(serve_one(listener, response));
    let client = FetchClient::new(&FetchConfig::default(), new_shared_browser_cookie_store());
    let response = client.fetch(Request::get(&url)?.with_single_hop()).await?;
    let (_, body) = response.into_body();
    assert_eq!(body.into_materialized_bytes().await?, b"<p>hello</p>");

    let fields = fields(&server.await??);
    let names: Vec<&str> = fields.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "host",
            "sec-ch-ua",
            "sec-ch-ua-mobile",
            "sec-ch-ua-platform",
            "upgrade-insecure-requests",
            "user-agent",
            "accept",
            "sec-fetch-site",
            "sec-fetch-mode",
            "sec-fetch-user",
            "sec-fetch-dest",
            "accept-encoding",
            "accept-language",
        ],
        "HTTP/1.1 carries no Priority header: {fields:?}"
    );
    let value = |wanted: &str| {
        fields
            .iter()
            .find(|(name, _)| name == wanted)
            .map(|(_, value)| value.as_str())
    };
    assert_eq!(value("accept-encoding"), Some("gzip, deflate, br"));
    assert!(
        value("user-agent").is_some_and(|agent| agent.contains("Chrome/")),
        "{fields:?}"
    );
    Ok(())
}
