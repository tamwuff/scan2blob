// Copied and pasted from
// https://github.com/rustls/hyper-rustls/blob/main/examples/server.rs

#[derive(Debug, serde::Deserialize)]
struct GateWebAppArgs {
    open: bool,
    name_hint: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct GateWebAppArgsCgi {
    open: String,
    name_hint: Option<String>,
}

impl TryFrom<GateWebAppArgsCgi> for GateWebAppArgs {
    type Error = scan2blob::error::WuffError;

    fn try_from(
        cgi_args: GateWebAppArgsCgi,
    ) -> Result<Self, scan2blob::error::WuffError> {
        let open: bool = match cgi_args.open.as_str() {
            "Locked" => false,
            "Unlocked" => true,
            _ => {
                return Err(scan2blob::error::WuffError::from(
                    "Invalid \"open\" value",
                ));
            }
        };
        let name_hint: Option<String> =
            if let Some(name_hint) = cgi_args.name_hint {
                if name_hint.trim().is_empty() {
                    None
                } else {
                    Some(name_hint)
                }
            } else {
                None
            };
        Ok(Self { open, name_hint })
    }
}

#[derive(Debug, serde::Serialize)]
struct GateWebAppResponse {
    error: Option<String>,
    open: bool,
    name_hint: Option<String>,
    next_change_time: Option<u64>,
}

impl GateWebAppResponse {
    fn to_browser(&self, web_listener: &GateWebListener) -> String {
        let mut page: String = String::new();
        page.push_str(r#"<html><head><title>"#);
        Self::html_escape(&mut page, &web_listener.gate.name);
        page.push_str(r#"</title></head><body><h1>"#);
        Self::html_escape(&mut page, &web_listener.gate.name);
        page.push_str(r#"</h1>"#);
        if let Some(ref error) = self.error {
            page.push_str(r#"<p style="color:red;"><strong>Error: "#);
            Self::html_escape(&mut page, error);
            page.push_str(r#"</strong></p><hr/>"#);
        }
        page.push_str(r#"<form method="post" action="/"><table><tr>"#);
        page.push_str(r#"<td colspan="2">Current status: "#);
        if self.open {
            page.push_str(r#"unlocked"#);
        } else {
            page.push_str(r#"locked"#);
        }
        if let Some(next_change_time) = self.next_change_time {
            let initial_value_secs: u64 = next_change_time.saturating_sub(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            );
            let initial_value_mins: f64 = (initial_value_secs as f64) / 60.0;
            page.push_str(&format!(
                r#" (valid for next <span id="next_change_time">{}</span> minutes)"#,
                initial_value_mins.round() as u64
            ));
        }
        page.push_str(r#"</td></tr><tr><td colspan="2">Name hint: "#);
        page.push_str(
            r#"<input type="text" size="32" name="name_hint" value=""#,
        );
        if let Some(name_hint) = self.name_hint.as_ref() {
            Self::html_escape(&mut page, name_hint);
        }
        page.push_str(r#""/><br></td></tr><tr><td align="left">"#);
        page.push_str(r#"<input type="submit" name="open" value="Locked"/>"#);
        page.push_str(r#"</td><td align="right">"#);
        page.push_str(
            r#"<input type="submit" name="open" value="Unlocked"/>"#,
        );
        page.push_str(r#"</td></tr></table></form>"#);
        if let Some(next_change_time) = self.next_change_time {
            page.push_str("<script>");
            page.push_str("setInterval(updateNextChangeTime, 10000);\n");
            page.push_str("function updateNextChangeTime() {\n");
            page.push_str("const now = Date.now() / 1000;\n");
            page.push_str(&format!(
                "const next_change_time = Math.max(0, {next_change_time} - now);\n"
            ));
            page.push_str(
                "const elem = document.getElementById(\"next_change_time\");\n",
            );
            page.push_str(
                "elem.textContent = Math.round(next_change_time / 60).toString()\n",
            );
            page.push_str("}</script>");
        }
        page.push_str(r#"</body></html>"#);
        page
    }

    fn html_escape(page: &mut String, s: &str) {
        for c in s.chars() {
            match c {
                '&' => page.push_str(r#"&amp;"#),
                '"' => page.push_str(r#"&quot;"#),
                '<' => page.push_str(r#"&lt;"#),
                '>' => page.push_str(r#"&gt;"#),
                c => page.push(c),
            }
        }
    }
}

struct GateWebListenerEachPort {
    web_listener: std::sync::Arc<GateWebListener>,
    listen_on: std::net::SocketAddr,
}

impl GateWebListenerEachPort {
    async fn run(self) -> ! {
        let async_spawner = self.web_listener.ctx.base_ctx.get_async_spawner();
        let server_sock: tokio::net::TcpListener =
            tokio::net::TcpListener::bind(&self.listen_on)
                .await
                .expect(&format!("{}", self.listen_on));
        loop {
            let Ok((sock, _peername)) = server_sock.accept().await else {
                // log something
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            };
            async_spawner.spawn(
                std::sync::Arc::clone(&self.web_listener)
                    .handle_connection(sock),
            );
        }
    }
}

#[derive(serde::Deserialize)]
pub struct ConfigGateWeb {
    listen_on: Vec<std::net::SocketAddr>,
    certificate_chain: scan2blob::util::LiteralOrFile,
    private_key: scan2blob::util::LiteralOrFile,
    users: std::collections::HashMap<String, String>,
}

pub struct GateWebListener {
    ctx: std::sync::Arc<crate::ctx::Ctx>,
    listen_on: Vec<std::net::SocketAddr>,
    rustls_acceptor: tokio_rustls::TlsAcceptor,
    hyper_builder:
        hyper_util::server::conn::auto::Builder<hyper_util::rt::TokioExecutor>,
    users: std::collections::HashMap<String, String>,
    http_basic_auth: scan2blob::http_basic_auth::HttpBasicAuth,
    http_accept_header: scan2blob::http_accept_header::HttpAcceptHeader,
    gate: std::sync::Arc<crate::gate::Gate>,
}

impl GateWebListener {
    pub fn new(
        ctx: &std::sync::Arc<crate::ctx::Ctx>,
        gate: &std::sync::Arc<crate::gate::Gate>,
        config: &ConfigGateWeb,
    ) -> Result<Self, scan2blob::error::WuffError> {
        let mut certificate_chain_data: std::io::Cursor<Vec<u8>> =
            std::io::Cursor::new(config.certificate_chain.get()?.into_bytes());
        let mut private_key_data: std::io::Cursor<Vec<u8>> =
            std::io::Cursor::new(config.private_key.get()?.into_bytes());
        let mut certificate_chain: Vec<
            rustls_pki_types::CertificateDer<'static>,
        > = Vec::new();
        for cert in rustls_pemfile::certs(&mut certificate_chain_data) {
            certificate_chain.push(cert?);
        }
        let Some(private_key) =
            rustls_pemfile::private_key(&mut private_key_data)?
        else {
            return Err(scan2blob::error::WuffError::from(
                "webdav: failed to load private key",
            ));
        };
        let mut rustls_server_config: rustls::ServerConfig =
            rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(certificate_chain, private_key)?;
        rustls_server_config.alpn_protocols =
            vec![b"h2".to_vec(), b"http/1.1".to_vec(), b"http/1.0".to_vec()];
        let rustls_acceptor: tokio_rustls::TlsAcceptor =
            tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(
                rustls_server_config,
            ));
        let hyper_executor: hyper_util::rt::TokioExecutor =
            hyper_util::rt::TokioExecutor::new();
        let hyper_builder: hyper_util::server::conn::auto::Builder<
            hyper_util::rt::TokioExecutor,
        > = hyper_util::server::conn::auto::Builder::new(hyper_executor);
        let http_basic_auth: scan2blob::http_basic_auth::HttpBasicAuth =
            scan2blob::http_basic_auth::HttpBasicAuth::new();
        let http_accept_header: scan2blob::http_accept_header::HttpAcceptHeader =
            scan2blob::http_accept_header::HttpAcceptHeader::new();
        Ok(Self {
            ctx: std::sync::Arc::clone(ctx),
            listen_on: config.listen_on.clone(),
            rustls_acceptor,
            hyper_builder,
            users: config.users.clone(),
            http_basic_auth,
            http_accept_header,
            gate: std::sync::Arc::clone(gate),
        })
    }

    pub fn start(self: &std::sync::Arc<Self>) {
        let async_spawner = self.ctx.base_ctx.get_async_spawner();
        for listen_on in &self.listen_on {
            async_spawner.spawn(
                GateWebListenerEachPort {
                    web_listener: std::sync::Arc::clone(self),
                    listen_on: *listen_on,
                }
                .run(),
            );
        }
    }

    async fn handle_connection(
        self: std::sync::Arc<Self>,
        sock: tokio::net::TcpStream,
    ) {
        let tls_sock: tokio_rustls::server::TlsStream<tokio::net::TcpStream> =
            match self.rustls_acceptor.accept(sock).await {
                Ok(tls_sock) => tls_sock,
                Err(err) => {
                    println!("failed to perform tls handshake: {err:#}");
                    return;
                }
            };
        let hyper_tls_sock: hyper_util::rt::TokioIo<
            tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
        > = hyper_util::rt::TokioIo::new(tls_sock);

        let hyper_service = hyper::service::service_fn({
            let self_: std::sync::Arc<Self> = std::sync::Arc::clone(&self);
            move |req| std::sync::Arc::clone(&self_).handle_request(req)
        });
        if let Err(err) = self
            .hyper_builder
            .serve_connection(hyper_tls_sock, hyper_service)
            .await
        {
            println!("failed to serve connection: {err:#}");
        }
    }

    fn calculate_redirect(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> String {
        let Some(host_header) = req.headers().get(hyper::header::HOST) else {
            return "/".to_string();
        };
        let Ok(host_header) = host_header.to_str() else {
            return "/".to_string();
        };
        format!("https://{}/", host_header)
    }

    async fn handle_request(
        self: std::sync::Arc<Self>,
        mut req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<String>, hyper::http::Error> {
        let body: Vec<u8> = Body::new(req.body_mut(), 10000).await;
        let uri: &hyper::Uri = req.uri();
        let headers: &hyper::HeaderMap = req.headers();

        let authorized: bool = if let Some(auth_header) =
            headers.get(hyper::header::AUTHORIZATION)
        {
            self.check_auth(auth_header)
        } else {
            false
        };

        if !authorized {
            return hyper::Response::builder()
                .status(hyper::StatusCode::UNAUTHORIZED)
                .header(
                    hyper::header::WWW_AUTHENTICATE,
                    "Basic realm=\"scan2blob\"",
                )
                .body("".into());
        }

        let (args, error, mut need_redirect) = match headers
            .get(hyper::header::CONTENT_TYPE)
            .map(AsRef::<[u8]>::as_ref)
        {
            Some(b"application/json") => {
                match serde_json::from_slice::<GateWebAppArgs>(&body) {
                    Ok(args) => (Some(args), None, false),
                    Err(e) => (
                        None,
                        Some(scan2blob::error::WuffError::from(e)),
                        false,
                    ),
                }
            }
            Some(b"application/x-www-form-urlencoded") => {
                match serde_urlencoded::from_bytes::<GateWebAppArgsCgi>(&body)
                {
                    Ok(args) => match GateWebAppArgs::try_from(args) {
                        Ok(args) => (Some(args), None, false),
                        Err(e) => (None, Some(e), false),
                    },
                    Err(e) => (
                        None,
                        Some(scan2blob::error::WuffError::from(e)),
                        false,
                    ),
                }
            }
            _ => {
                if let Some(query) = uri.query() {
                    match serde_urlencoded::from_str::<GateWebAppArgsCgi>(
                        query,
                    ) {
                        Ok(args) => match GateWebAppArgs::try_from(args) {
                            Ok(args) => (Some(args), None, true),
                            Err(e) => (None, Some(e), true),
                        },
                        Err(e) => (
                            None,
                            Some(scan2blob::error::WuffError::from(e)),
                            true,
                        ),
                    }
                } else {
                    (None, None, false)
                }
            }
        };
        match uri.path() {
            "" | "/" => {}
            _ => {
                need_redirect = true;
            }
        }

        if let Some(args) = args {
            if args.open {
                self.gate
                    .assert_gate_open_timed_with_name_hint(args.name_hint);
            } else {
                self.gate.assert_gate_closed();
            }
        }

        let (state, next_change_time) = self.gate.get_current_state_extended();
        let error: Option<String> = error.map(|e| format!("{}", e));
        let next_change_time: Option<u64> = next_change_time.map(|d| {
            (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                + d)
                .as_secs()
        });
        let response: GateWebAppResponse = if let Some(name_hint) = state {
            GateWebAppResponse {
                open: true,
                name_hint,
                error,
                next_change_time,
            }
        } else {
            GateWebAppResponse {
                open: false,
                name_hint: None,
                error,
                next_change_time,
            }
        };

        // Heuristic: browsers will tend to explicitly ask for text/html (not
        // using wildcards, but actually literally text/html), earlier in the
        // list than they ask for application/json.
        let mut is_browser: bool = false;
        if let Some(accept_header) = headers.get(hyper::header::ACCEPT) {
            if let Ok(accept_header) = accept_header.to_str() {
                for mime_type in self.http_accept_header.parse(accept_header) {
                    match mime_type {
                        (Some("text"), Some("html")) => {
                            is_browser = true;
                            break;
                        }
                        (Some("application"), Some("json")) => {
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        let status_code: hyper::StatusCode = if response.error.is_some() {
            hyper::StatusCode::BAD_REQUEST
        } else {
            hyper::StatusCode::OK
        };

        if is_browser {
            const CACHE_CONTROL: &str = "no-store";
            if need_redirect {
                if let Some(error) = response.error {
                    // We're going to lose the error if we redirect.
                    hyper::Response::builder()
                        .status(status_code)
                        .header(hyper::header::CONTENT_TYPE, "text/plain")
                        .header(hyper::header::CACHE_CONTROL, CACHE_CONTROL)
                        .body(error)
                } else {
                    hyper::Response::builder()
                        .status(hyper::StatusCode::FOUND)
                        .header(hyper::header::CONTENT_TYPE, "text/html")
                        .header(hyper::header::CACHE_CONTROL, CACHE_CONTROL)
                        .header(
                            hyper::header::LOCATION,
                            self.calculate_redirect(req),
                        )
                        .body(response.to_browser(self.as_ref()))
                }
            } else {
                hyper::Response::builder()
                    .status(status_code)
                    .header(hyper::header::CONTENT_TYPE, "text/html")
                    .header(hyper::header::CACHE_CONTROL, CACHE_CONTROL)
                    .body(response.to_browser(self.as_ref()))
            }
        } else {
            hyper::Response::builder()
                .status(status_code)
                .header(hyper::header::CONTENT_TYPE, "application/json")
                .body(
                    serde_json::to_string_pretty(&response)
                        .expect("serde_json"),
                )
        }
    }

    fn check_auth(&self, auth_header: &hyper::header::HeaderValue) -> bool {
        let Ok(auth_header) = auth_header.to_str() else {
            return false;
        };

        let Some((username, plaintext)) =
            self.http_basic_auth.parse(auth_header)
        else {
            return false;
        };

        let Some(password) = self.users.get(&username) else {
            return false;
        };

        scan2blob::pwhash::verify(&plaintext, password)
    }
}

struct Body<'a> {
    inner: Box<&'a mut hyper::body::Incoming>,
    s: Vec<u8>,
    limit: usize,
}

impl<'a> Body<'a> {
    fn new(inner: &'a mut hyper::body::Incoming, limit: usize) -> Self {
        Self {
            s: Vec::new(),
            limit,
            inner: Box::new(inner),
        }
    }
}

impl<'a> std::future::Future for Body<'a> {
    type Output = Vec<u8>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Vec<u8>> {
        while self.s.len() < self.limit {
            let pinned = std::pin::Pin::new(self.inner.as_mut());
            let frame = match hyper::body::Body::poll_frame(pinned, cx) {
                std::task::Poll::Pending => {
                    return std::task::Poll::Pending;
                }
                std::task::Poll::Ready(None | Some(Err(_))) => {
                    break;
                }
                std::task::Poll::Ready(Some(Ok(frame))) => frame,
            };
            let Ok(data) = frame.into_data() else {
                break;
            };
            self.s.extend_from_slice(data.as_ref());
        }
        let mut temp: Vec<u8> = Vec::new();
        std::mem::swap(&mut self.s, &mut temp);
        std::task::Poll::Ready(temp)
    }
}
