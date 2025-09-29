// Copied and pasted from
// https://github.com/rustls/hyper-rustls/blob/main/examples/server.rs and
// https://github.com/messense/dav-server-rs/blob/main/README.md

#[derive(Clone)]
struct DestinationAndGate {
    destination: std::sync::Arc<crate::destination::Destination>,
    gate: std::sync::Arc<crate::gate::Gate>,
}

#[derive(Debug, Clone)]
struct FileMetadata(u64);

impl dav_server::fs::DavMetaData for FileMetadata {
    fn len(&self) -> u64 {
        self.0
    }
    fn modified(&self) -> dav_server::fs::FsResult<std::time::SystemTime> {
        Ok(std::time::SystemTime::now())
    }
    fn is_dir(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone)]
struct DirMetadata;

impl dav_server::fs::DavMetaData for DirMetadata {
    fn len(&self) -> u64 {
        0
    }
    fn modified(&self) -> dav_server::fs::FsResult<std::time::SystemTime> {
        Ok(std::time::SystemTime::now())
    }
    fn is_dir(&self) -> bool {
        true
    }
}

struct OpenFile {
    webdav_listener: std::sync::Arc<WebdavListener>,
    orig_filename: String,
    writer: Option<scan2blob::chunker::Writer>,
    off: u64,
    expected_file_size: u64,
}

impl dav_server::fs::DavFile for OpenFile {
    fn metadata(
        &mut self,
    ) -> dav_server::fs::FsFuture<Box<dyn dav_server::fs::DavMetaData>> {
        Box::pin(async move {
            let metadata: FileMetadata = FileMetadata(self.off);
            let boxed_metadata: Box<dyn dav_server::fs::DavMetaData> =
                Box::new(metadata);
            Ok(boxed_metadata)
        })
    }

    fn write_buf(
        &mut self,
        mut buf: Box<dyn hyper::body::Buf + Send>,
    ) -> dav_server::fs::FsFuture<()> {
        Box::pin(async move {
            let writer: &mut scan2blob::chunker::Writer =
                self.writer.as_mut().unwrap();
            loop {
                let chunk: &[u8] = buf.chunk();
                if chunk.is_empty() {
                    break;
                }
                let chunk_len: usize = chunk.len();

                if let Err(err) = writer.write(chunk).await {
                    self.webdav_listener.ctx.log_info(format!(
                        "webdav: aborting upload of {} due to propagated error: {}",
                        self.orig_filename, err
                    ));
                    return Err(dav_server::fs::FsError::GeneralFailure);
                }
                self.off += chunk_len as u64;
                buf.advance(chunk_len);
            }
            Ok(())
        })
    }
    fn write_bytes(
        &mut self,
        buf: bytes::Bytes,
    ) -> dav_server::fs::FsFuture<()> {
        Box::pin(async move {
            let writer: &mut scan2blob::chunker::Writer =
                self.writer.as_mut().unwrap();
            let as_slice: &[u8] = buf.as_ref();
            if let Err(err) = writer.write(as_slice).await {
                self.webdav_listener.ctx.log_info(format!(
                    "webdav: aborting upload of {} due to propagated error: {}",
                    self.orig_filename, err
                ));
                return Err(dav_server::fs::FsError::GeneralFailure);
            }
            self.off += as_slice.len() as u64;
            Ok(())
        })
    }
    fn read_bytes(
        &mut self,
        _count: usize,
    ) -> dav_server::fs::FsFuture<bytes::Bytes> {
        Box::pin(async move { Err(dav_server::fs::FsError::NotImplemented) })
    }
    fn seek(
        &mut self,
        _pos: std::io::SeekFrom,
    ) -> dav_server::fs::FsFuture<u64> {
        self.webdav_listener.ctx.log_info(format!(
            "webdav: aborting upload of {} because client attempted a seek, which is not supported",
            self.orig_filename
        ));
        self.writer.as_ref().unwrap().observe_error(
            scan2blob::error::WuffError::from(
                "webdav client attempted seek, which is not supported",
            ),
        );
        Box::pin(async move { Err(dav_server::fs::FsError::NotImplemented) })
    }
    fn flush(&mut self) -> dav_server::fs::FsFuture<()> {
        Box::pin(async move { Ok(()) })
    }
}

impl Drop for OpenFile {
    fn drop(&mut self) {
        let async_spawner =
            self.webdav_listener.ctx.base_ctx.get_async_spawner();
        let Some(mut writer) = self.writer.take() else {
            return;
        };
        if self.off != self.expected_file_size {
            let msg: String = format!(
                "webdav: aborting upload of {}, {} bytes were written but there were supposed to be {} bytes",
                self.orig_filename, self.off, self.expected_file_size
            );
            self.webdav_listener.ctx.log_info(msg);
        }
        let webdav_listener: std::sync::Arc<WebdavListener> =
            std::sync::Arc::clone(&self.webdav_listener);
        let mut orig_filename: String = String::new();
        std::mem::swap(&mut orig_filename, &mut self.orig_filename);
        async_spawner.spawn(async move {
            if let Err(err) = writer.finalize().await {
                webdav_listener.ctx.log_info(format!(
                    "webdav: aborting upload of {} due to propagated error: {}",
                    orig_filename, err
                ));
            }
        });
    }
}

// I never wanted to impl Debug anyway, but the DavFile trait requires it...
impl std::fmt::Debug for OpenFile {
    fn fmt(&self, _f: &mut std::fmt::Formatter) -> std::fmt::Result {
        Ok(())
    }
}

// If we're not careful here we're going to end up with a circular reference
// where the WebdavListener holds a reference to a dav_server::DavHandler,
// which then in turn holds a reference to the WebdavListener again.
//
// We avoid this by never storing the dav_server::DavHandler anywhere except,
// ephemerally, as a local variable in a running async task. So it can hold a
// reference to the WebdavListener, but the WebdavListener is never going to
// hold a reference back to it.
//
// WebdavFilesystem is a tiny little newtype wrapper around Arc<WebdavListener>
// for the sole reason that we can't impl dav_server::fs::GuardedFileSystem
// for Arc<WebdavListener> directly, so we have to impl it for a newtype around
// Arc<WebdavListener>.
#[derive(Clone)]
struct WebdavFilesystem(std::sync::Arc<WebdavListener>);

impl dav_server::fs::GuardedFileSystem<DestinationAndGate>
    for WebdavFilesystem
{
    fn open(
        &self,
        path: &dav_server::davpath::DavPath,
        options: dav_server::fs::OpenOptions,
        destination_and_gate: &DestinationAndGate,
    ) -> dav_server::fs::FsFuture<Box<dyn dav_server::fs::DavFile>> {
        let destination_and_gate = destination_and_gate.clone();
        let orig_filename: Option<String> =
            path.file_name().map(ToOwned::to_owned);
        Box::pin(async move {
            if options.read
                || !(options.write || options.append)
                || !(options.truncate || options.create_new)
            {
                return Err(dav_server::fs::FsError::NotImplemented);
            }
            let Some(orig_filename) = orig_filename else {
                return Err(dav_server::fs::FsError::NotFound);
            };
            let Some(expected_file_size) = options.size else {
                self.0.ctx.log_info(
                    "webdav: attempted to open a file for writing without providing an expected size",
                );
                return Err(dav_server::fs::FsError::NotImplemented);
            };

            let Some(writer) = destination_and_gate.gate.try_write_file(
                &orig_filename,
                &destination_and_gate.destination,
            ) else {
                self.0.ctx.log_info(format!(
                    "webdav: rejecting file upload because gate {} is closed",
                    destination_and_gate.gate.name
                ));
                return Err(dav_server::fs::FsError::Forbidden);
            };
            let webdav_listener: std::sync::Arc<WebdavListener> =
                std::sync::Arc::clone(&self.0);
            let open_file: OpenFile = OpenFile {
                webdav_listener,
                orig_filename,
                writer: Some(writer),
                off: 0,
                expected_file_size,
            };
            let boxed_open_file: Box<dyn dav_server::fs::DavFile> =
                Box::new(open_file);
            Ok(boxed_open_file)
        })
    }

    fn read_dir(
        &self,
        _path: &dav_server::davpath::DavPath,
        _meta: dav_server::fs::ReadDirMeta,
        _destination_and_gate: &DestinationAndGate,
    ) -> dav_server::fs::FsFuture<
        dav_server::fs::FsStream<Box<dyn dav_server::fs::DavDirEntry>>,
    > {
        Box::pin(async move {
            let stream = futures::stream::empty();
            let boxed_stream: dav_server::fs::FsStream<
                Box<dyn dav_server::fs::DavDirEntry>,
            > = Box::pin(stream);
            Ok(boxed_stream)
        })
    }

    fn metadata(
        &self,
        _path: &dav_server::davpath::DavPath,
        _destination_and_gate: &DestinationAndGate,
    ) -> dav_server::fs::FsFuture<Box<dyn dav_server::fs::DavMetaData>> {
        Box::pin(async move {
            let boxed_metadata: Box<dyn dav_server::fs::DavMetaData> =
                Box::new(DirMetadata);
            Ok(boxed_metadata)
        })
    }

    fn create_dir(
        &self,
        _path: &dav_server::davpath::DavPath,
        _destination_and_gate: &DestinationAndGate,
    ) -> dav_server::fs::FsFuture<()> {
        Box::pin(async move { Ok(()) })
    }

    fn remove_dir(
        &self,
        _path: &dav_server::davpath::DavPath,
        _destination_and_gate: &DestinationAndGate,
    ) -> dav_server::fs::FsFuture<()> {
        Box::pin(async move { Ok(()) })
    }

    fn remove_file(
        &self,
        _path: &dav_server::davpath::DavPath,
        _destination_and_gate: &DestinationAndGate,
    ) -> dav_server::fs::FsFuture<()> {
        Box::pin(async move { Ok(()) })
    }

    fn rename(
        &self,
        _from: &dav_server::davpath::DavPath,
        _to: &dav_server::davpath::DavPath,
        _destination_and_gate: &DestinationAndGate,
    ) -> dav_server::fs::FsFuture<()> {
        Box::pin(async move { Ok(()) })
    }

    #[cfg(feature = "webdav-props")]
    fn have_props(
        &self,
        _path: &dav_server::davpath::DavPath,
        _destination_and_gate: &DestinationAndGate,
    ) -> std::pin::Pin<Box<dyn Future<Output = bool> + Send>> {
        Box::pin(async move { true })
    }

    #[cfg(feature = "webdav-props")]
    fn patch_props(
        &self,
        _path: &dav_server::davpath::DavPath,
        patch: Vec<(bool, dav_server::fs::DavProp)>,
        _destination_and_gate: &DestinationAndGate,
    ) -> dav_server::fs::FsFuture<
        Vec<(hyper::StatusCode, dav_server::fs::DavProp)>,
    > {
        Box::pin(async move {
            Ok(patch
                .into_iter()
                .map(|(_, dav_prop)| (hyper::StatusCode::NOT_FOUND, dav_prop))
                .collect())
        })
    }

    #[cfg(feature = "webdav-props")]
    fn get_props(
        &self,
        _path: &dav_server::davpath::DavPath,
        _do_content: bool,
        _destination_and_gate: &DestinationAndGate,
    ) -> dav_server::fs::FsFuture<Vec<dav_server::fs::DavProp>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    #[cfg(feature = "webdav-props")]
    fn get_prop(
        &self,
        _path: &dav_server::davpath::DavPath,
        prop: dav_server::fs::DavProp,
        _destination_and_gate: &DestinationAndGate,
    ) -> dav_server::fs::FsFuture<Vec<u8>> {
        Box::pin(async move { Err(dav_server::fs::FsError::NotFound) })
    }
}

struct WebdavListenerEachPort {
    webdav_listener: std::sync::Arc<WebdavListener>,
    listen_on: std::net::SocketAddr,
}

impl WebdavListenerEachPort {
    async fn run(
        self,
        dav_handler: std::sync::Arc<
            dav_server::DavHandler<DestinationAndGate>,
        >,
    ) {
        let async_spawner =
            self.webdav_listener.ctx.base_ctx.get_async_spawner();
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
                std::sync::Arc::clone(&self.webdav_listener)
                    .handle_connection(
                        std::sync::Arc::clone(&dav_handler),
                        sock,
                    ),
            );
        }
    }
}

#[derive(serde::Deserialize)]
pub struct ConfigListenerWebdavUser {
    password: String,
    destination: String,
    gate: String,
}

#[derive(serde::Deserialize)]
pub struct ConfigListenerWebdav {
    listen_on: Vec<std::net::SocketAddr>,
    certificate_chain: scan2blob::util::LiteralOrFile,
    private_key: scan2blob::util::LiteralOrFile,
    users: std::collections::HashMap<String, ConfigListenerWebdavUser>,
}

pub struct ConfigListenerWebdavEnriched {
    listen_on: Vec<std::net::SocketAddr>,
    certificate_chain: String,
    private_key: String,
    users: std::collections::HashMap<String, ConfigListenerWebdavUser>,
}

impl TryFrom<ConfigListenerWebdav> for ConfigListenerWebdavEnriched {
    type Error = scan2blob::error::WuffError;

    fn try_from(
        config: ConfigListenerWebdav,
    ) -> Result<Self, scan2blob::error::WuffError> {
        let ConfigListenerWebdav {
            listen_on,
            certificate_chain,
            private_key,
            users,
        } = config;
        Ok(Self {
            listen_on,
            certificate_chain: certificate_chain.try_into()?,
            private_key: private_key.try_into()?,
            users,
        })
    }
}

struct WebdavListenerUser {
    password: String,
    destination_and_gate: DestinationAndGate,
}

pub struct WebdavListener {
    ctx: std::sync::Arc<crate::ctx::Ctx>,
    listen_on: Vec<std::net::SocketAddr>,
    rustls_acceptor: tokio_rustls::TlsAcceptor,
    hyper_builder:
        hyper_util::server::conn::auto::Builder<hyper_util::rt::TokioExecutor>,
    users: std::collections::HashMap<String, WebdavListenerUser>,
    http_basic_auth: scan2blob::http_basic_auth::HttpBasicAuth,
}

impl WebdavListener {
    pub fn new(
        ctx: &std::sync::Arc<crate::ctx::Ctx>,
        config: &ConfigListenerWebdavEnriched,
        destinations: &crate::destination::Destinations,
        gates: &crate::gate::Gates,
    ) -> Result<Self, scan2blob::error::WuffError> {
        let mut certificate_chain_data: std::io::Cursor<&[u8]> =
            std::io::Cursor::new(config.certificate_chain.as_bytes());
        let mut private_key_data: std::io::Cursor<&[u8]> =
            std::io::Cursor::new(config.private_key.as_bytes());
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
        let mut users: std::collections::HashMap<String, WebdavListenerUser> =
            std::collections::HashMap::new();
        for (
            username,
            ConfigListenerWebdavUser {
                password,
                destination,
                gate,
            },
        ) in &config.users
        {
            let Some(destination) = destinations.get(destination) else {
                return Err(scan2blob::error::WuffError::from(
                    "Destination not found",
                ));
            };
            let Some(gate) = gates.get(gate) else {
                return Err(scan2blob::error::WuffError::from(
                    "Gate not found",
                ));
            };
            let _ = users.insert(
                username.clone(),
                WebdavListenerUser {
                    password: password.clone(),
                    destination_and_gate: DestinationAndGate {
                        destination,
                        gate,
                    },
                },
            );
        }
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
        Ok(Self {
            ctx: std::sync::Arc::clone(ctx),
            listen_on: config.listen_on.clone(),
            rustls_acceptor,
            hyper_builder,
            users,
            http_basic_auth,
        })
    }

    pub fn start(self: &std::sync::Arc<Self>) {
        let dav_handler: std::sync::Arc<
            dav_server::DavHandler<DestinationAndGate>,
        > = std::sync::Arc::new(
            dav_server::DavHandler::builder()
                .filesystem(Box::new(WebdavFilesystem(std::sync::Arc::clone(
                    self,
                ))))
                .locksystem(dav_server::fakels::FakeLs::new())
                .build_handler(),
        );

        for listen_on in &self.listen_on {
            self.ctx.spawn_critical(
                "webdav",
                WebdavListenerEachPort {
                    webdav_listener: std::sync::Arc::clone(self),
                    listen_on: *listen_on,
                }
                .run(std::sync::Arc::clone(&dav_handler)),
            );
        }
    }

    async fn handle_connection(
        self: std::sync::Arc<Self>,
        dav_handler: std::sync::Arc<
            dav_server::DavHandler<DestinationAndGate>,
        >,
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
            let dav_handler: std::sync::Arc<
                dav_server::DavHandler<DestinationAndGate>,
            > = std::sync::Arc::clone(&dav_handler);
            move |req| {
                std::sync::Arc::clone(&self_)
                    .handle_request(std::sync::Arc::clone(&dav_handler), req)
            }
        });
        if let Err(err) = self
            .hyper_builder
            .serve_connection(hyper_tls_sock, hyper_service)
            .await
        {
            println!("failed to serve connection: {err:#}");
        }
    }

    // Trait bounds are copied/pasted from the definition of
    // dav_server::DavHandler::handle()
    async fn handle_request<ReqBody, ReqData, ReqError>(
        self: std::sync::Arc<Self>,
        dav_handler: std::sync::Arc<
            dav_server::DavHandler<DestinationAndGate>,
        >,
        req: hyper::Request<ReqBody>,
    ) -> Result<hyper::Response<dav_server::body::Body>, hyper::http::Error>
    where
        ReqData: hyper::body::Buf + Send + 'static,
        ReqError: std::error::Error + Send + Sync + 'static,
        ReqBody: hyper::body::Body<Data = ReqData, Error = ReqError>,
    {
        let headers: &hyper::HeaderMap = req.headers();

        let destination_and_gate: Option<DestinationAndGate> =
            if let Some(auth_header) =
                headers.get(hyper::header::AUTHORIZATION)
            {
                self.check_auth(auth_header)
            } else {
                None
            };

        let Some(destination_and_gate) = destination_and_gate else {
            return hyper::Response::builder()
                .status(hyper::StatusCode::UNAUTHORIZED)
                .header(
                    hyper::header::WWW_AUTHENTICATE,
                    "Basic realm=\"scan2blob\"",
                )
                .body(dav_server::body::Body::empty());
        };

        Ok(dav_handler.handle_guarded(req, destination_and_gate).await)
    }

    fn check_auth(
        &self,
        auth_header: &hyper::header::HeaderValue,
    ) -> Option<DestinationAndGate> {
        let Ok(auth_header) = auth_header.to_str() else {
            return None;
        };

        let Some((username, plaintext)) =
            self.http_basic_auth.parse(auth_header)
        else {
            return None;
        };

        let Some(WebdavListenerUser {
            password,
            destination_and_gate,
        }) = self.users.get(&username)
        else {
            return None;
        };

        if scan2blob::pwhash::verify(&plaintext, password) {
            Some(destination_and_gate.clone())
        } else {
            None
        }
    }
}
