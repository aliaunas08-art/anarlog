//! The one-shot local web server the browser is redirected back to after the
//! user approves access (RFC 8252 loopback redirect).

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::Error;

const MAX_REQUEST_BYTES: usize = 8 * 1024;
/// Browsers open speculative connections that never send a request; each is
/// handled on its own task, and dropped after this long.
const READ_TIMEOUT: Duration = Duration::from_secs(3);
const BIND_ATTEMPTS: usize = 10;

pub(crate) struct Loopback {
    v4: TcpListener,
    /// The same port on IPv6, for browsers that resolve `localhost` to `::1`.
    v6: Option<TcpListener>,
    port: u16,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Callback {
    Code(String),
    Denied(String),
}

impl Loopback {
    /// Binds a free port on `127.0.0.1` and the same port on `::1`, so a
    /// browser that tries IPv6 first for `localhost` can't reach some other
    /// process holding that port. Without IPv6 on the Mac, IPv4 alone is used.
    pub(crate) async fn bind() -> Result<Self, Error> {
        for _ in 0..BIND_ATTEMPTS {
            let v4 = TcpListener::bind(("127.0.0.1", 0))
                .await
                .map_err(|e| Error::Loopback(e.to_string()))?;
            let port = v4
                .local_addr()
                .map_err(|e| Error::Loopback(e.to_string()))?
                .port();
            match TcpListener::bind(("::1", port)).await {
                Ok(v6) => {
                    return Ok(Self {
                        v4,
                        v6: Some(v6),
                        port,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => continue,
                Err(_) => return Ok(Self { v4, v6: None, port }),
            }
        }
        Err(Error::Loopback("no free local port".into()))
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    /// Waits for the redirect carrying `expected_state`. Requests for other
    /// paths (a favicon) or with the wrong state are answered and ignored.
    pub(crate) async fn wait(&self, expected_state: &str) -> Result<Callback, Error> {
        let (found_tx, mut found_rx) = tokio::sync::mpsc::channel::<Callback>(1);
        // Dropped on return, which aborts any connection still being read.
        let mut handlers = tokio::task::JoinSet::new();
        loop {
            let accepted = tokio::select! {
                Some(callback) = found_rx.recv() => return Ok(callback),
                accepted = self.v4.accept() => accepted,
                accepted = accept_optional(self.v6.as_ref()) => accepted,
            };
            let (stream, _) = accepted.map_err(|e| Error::Loopback(e.to_string()))?;
            let found_tx = found_tx.clone();
            let expected_state = expected_state.to_string();
            handlers.spawn(async move {
                if let Some(callback) = handle(stream, &expected_state).await {
                    let _ = found_tx.send(callback).await;
                }
            });
            // Reap finished handlers so the set doesn't grow.
            while handlers.try_join_next().is_some() {}
        }
    }
}

async fn accept_optional(
    listener: Option<&TcpListener>,
) -> std::io::Result<(TcpStream, std::net::SocketAddr)> {
    match listener {
        Some(listener) => listener.accept().await,
        None => std::future::pending().await,
    }
}

async fn handle(mut stream: TcpStream, expected_state: &str) -> Option<Callback> {
    let Some(target) = read_request_target(&mut stream).await else {
        respond(&mut stream, 400, "Bad request").await;
        return None;
    };
    match parse_callback(&target, expected_state) {
        Parsed::Callback(callback) => {
            let page = match &callback {
                Callback::Code(_) => SUCCESS_PAGE,
                Callback::Denied(_) => DENIED_PAGE,
            };
            respond(&mut stream, 200, page).await;
            Some(callback)
        }
        Parsed::Ignore => {
            respond(&mut stream, 404, "Not found").await;
            None
        }
        Parsed::WrongState => {
            respond(&mut stream, 400, "This sign-in link is no longer valid.").await;
            None
        }
    }
}

async fn read_request_target(stream: &mut TcpStream) -> Option<String> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let read = async {
        loop {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                return None;
            }
            buffer.extend_from_slice(&chunk[..n]);
            if buffer.windows(4).any(|w| w == b"\r\n\r\n") || buffer.len() >= MAX_REQUEST_BYTES {
                return Some(());
            }
        }
    };
    tokio::time::timeout(READ_TIMEOUT, read).await.ok()??;
    let head = String::from_utf8_lossy(&buffer);
    let line = head.lines().next()?;
    let mut parts = line.split(' ');
    (parts.next()? == "GET").then_some(())?;
    Some(parts.next()?.to_string())
}

#[derive(Debug, PartialEq, Eq)]
enum Parsed {
    Callback(Callback),
    Ignore,
    WrongState,
}

fn parse_callback(target: &str, expected_state: &str) -> Parsed {
    let Ok(url) = url::Url::parse(&format!("http://loopback{target}")) else {
        return Parsed::Ignore;
    };
    if url.path() != "/" {
        return Parsed::Ignore;
    }
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            _ => {}
        }
    }
    if code.is_none() && error.is_none() {
        return Parsed::Ignore;
    }
    if state.as_deref() != Some(expected_state) {
        return Parsed::WrongState;
    }
    match (code, error) {
        (_, Some(error)) => Parsed::Callback(Callback::Denied(error)),
        (Some(code), None) if !code.is_empty() => Parsed::Callback(Callback::Code(code)),
        _ => Parsed::Ignore,
    }
}

async fn respond(stream: &mut TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

const SUCCESS_PAGE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>Signed in</title>
<style>body{font:15px -apple-system,system-ui,sans-serif;background:#fbfbfd;color:#1d1d1f;display:grid;place-items:center;height:100vh;margin:0}
main{text-align:center}h1{font-size:22px;font-weight:600;letter-spacing:-0.01em;margin:0 0 8px}p{color:#6e6e73;margin:0}
@media(prefers-color-scheme:dark){body{background:#000;color:#f5f5f7}p{color:#a1a1a6}}</style></head>
<body><main><h1>You're connected</h1><p>You can close this tab and go back to Notes.</p></main></body></html>"#;

const DENIED_PAGE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>Not connected</title>
<style>body{font:15px -apple-system,system-ui,sans-serif;background:#fbfbfd;color:#1d1d1f;display:grid;place-items:center;height:100vh;margin:0}
main{text-align:center}h1{font-size:22px;font-weight:600;letter-spacing:-0.01em;margin:0 0 8px}p{color:#6e6e73;margin:0}
@media(prefers-color-scheme:dark){body{background:#000;color:#f5f5f7}p{color:#a1a1a6}}</style></head>
<body><main><h1>Not connected</h1><p>Access wasn't granted. You can close this tab.</p></main></body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codes_denials_and_ignores_noise() {
        assert_eq!(
            parse_callback("/?code=abc&state=s1&scope=x", "s1"),
            Parsed::Callback(Callback::Code("abc".into()))
        );
        assert_eq!(
            parse_callback("/?error=access_denied&state=s1", "s1"),
            Parsed::Callback(Callback::Denied("access_denied".into()))
        );
        assert_eq!(
            parse_callback("/?code=abc&state=other", "s1"),
            Parsed::WrongState
        );
        assert_eq!(parse_callback("/?code=abc", "s1"), Parsed::WrongState);
        assert_eq!(parse_callback("/favicon.ico", "s1"), Parsed::Ignore);
        assert_eq!(parse_callback("/", "s1"), Parsed::Ignore);
    }

    #[tokio::test]
    async fn answers_the_browser_and_returns_the_code() {
        let loopback = Loopback::bind().await.unwrap();
        let port = loopback.port();
        let client = tokio::spawn(async move {
            let get = |path: &'static str| async move {
                let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                stream
                    .write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                    .await
                    .unwrap();
                let mut response = String::new();
                stream.read_to_string(&mut response).await.unwrap();
                response
            };
            // A speculative connection that never sends anything.
            let _idle = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            let favicon = get("/favicon.ico").await;
            let forged = get("/?code=evil&state=nope").await;
            let real = get("/?code=good&state=s1").await;
            (favicon, forged, real)
        });
        let callback = loopback.wait("s1").await.unwrap();
        assert_eq!(callback, Callback::Code("good".into()));
        let (favicon, forged, real) = client.await.unwrap();
        assert!(favicon.starts_with("HTTP/1.1 404"));
        assert!(forged.starts_with("HTTP/1.1 400"));
        assert!(real.starts_with("HTTP/1.1 200"));
        assert!(real.contains("You're connected"));
    }
}
