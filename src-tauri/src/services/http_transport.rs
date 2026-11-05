//! Shared pools grouped by transport policy. Credentials stay on individual requests.
use std::{sync::OnceLock, time::Duration};

pub(crate) fn local_worker() -> Result<&'static reqwest::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(120))
                .build()
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub(crate) fn catalog() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn local_pool_reuses_connection_and_never_reuses_authorization() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut headers = Vec::new();
            for _ in 0..2 {
                let mut bytes = Vec::new();
                while !bytes.ends_with(b"\r\n\r\n") {
                    bytes.push(stream.read_u8().await.unwrap());
                }
                headers.push(String::from_utf8(bytes).unwrap());
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                    .await
                    .unwrap();
            }
            headers
        });
        for secret in ["fixture-one", "fixture-two"] {
            super::local_worker()
                .unwrap()
                .get(&url)
                .header("authorization", secret)
                .send()
                .await
                .unwrap()
                .bytes()
                .await
                .unwrap();
        }
        let headers = tokio::time::timeout(std::time::Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
        assert!(headers[0].contains("fixture-one"));
        assert!(!headers[1].contains("fixture-one"));
        assert!(headers[1].contains("fixture-two"));
    }
}
