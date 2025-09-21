// Copied and pasted from
// https://github.com/rustls/hyper-rustls/blob/main/examples/server.rs and
// https://github.com/messense/dav-server-rs/blob/main/README.md

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
    writer: Option<scan2blob::chunker::Writer>,
    off: u64,
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
                    // log error
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
                // log error
                return Err(dav_server::fs::FsError::GeneralFailure);
            }
            self.off += as_slice.len() as u64;
            Ok(())
        })
    }
    fn read_bytes(
        &mut self,
        count: usize,
    ) -> dav_server::fs::FsFuture<bytes::Bytes> {
        Box::pin(async move { Err(dav_server::fs::FsError::NotImplemented) })
    }
    fn seek(
        &mut self,
        pos: std::io::SeekFrom,
    ) -> dav_server::fs::FsFuture<u64> {
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
        async_spawner.spawn(async move {
            if let Err(err) = writer.finalize().await {
                // log error
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

impl
    dav_server::fs::GuardedFileSystem<
        std::sync::Arc<crate::destination::Destination>,
    > for WebdavFilesystem
{
    fn open(
        &self,
        _path: &dav_server::davpath::DavPath,
        options: dav_server::fs::OpenOptions,
        destination: &std::sync::Arc<crate::destination::Destination>,
    ) -> dav_server::fs::FsFuture<Box<dyn dav_server::fs::DavFile>> {
        let destination: std::sync::Arc<crate::destination::Destination> =
            std::sync::Arc::clone(destination);
        Box::pin(async move {
            if options.read
                || !(options.write || options.append)
                || !(options.truncate || options.create_new)
            {
                return Err(dav_server::fs::FsError::NotImplemented);
            }

            let writer: scan2blob::chunker::Writer = destination.write_file();
            let webdav_listener: std::sync::Arc<WebdavListener> =
                std::sync::Arc::clone(&self.0);
            let open_file: OpenFile = OpenFile {
                webdav_listener,
                writer: Some(writer),
                off: 0,
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
        _destination: &std::sync::Arc<crate::destination::Destination>,
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
        _destination: &std::sync::Arc<crate::destination::Destination>,
    ) -> dav_server::fs::FsFuture<Box<dyn dav_server::fs::DavMetaData>> {
        Box::pin(async move {
            let metadata: DirMetadata = DirMetadata;
            let boxed_metadata: Box<dyn dav_server::fs::DavMetaData> =
                Box::new(DirMetadata);
            Ok(boxed_metadata)
        })
    }

    fn create_dir(
        &self,
        _path: &dav_server::davpath::DavPath,
        _destination: &std::sync::Arc<crate::destination::Destination>,
    ) -> dav_server::fs::FsFuture<()> {
        Box::pin(async move { Ok(()) })
    }

    fn remove_dir(
        &self,
        _path: &dav_server::davpath::DavPath,
        _destination: &std::sync::Arc<crate::destination::Destination>,
    ) -> dav_server::fs::FsFuture<()> {
        Box::pin(async move { Ok(()) })
    }

    fn remove_file(
        &self,
        _path: &dav_server::davpath::DavPath,
        _destination: &std::sync::Arc<crate::destination::Destination>,
    ) -> dav_server::fs::FsFuture<()> {
        Box::pin(async move { Ok(()) })
    }

    fn rename(
        &self,
        _from: &dav_server::davpath::DavPath,
        _to: &dav_server::davpath::DavPath,
        _destination: &std::sync::Arc<crate::destination::Destination>,
    ) -> dav_server::fs::FsFuture<()> {
        Box::pin(async move { Ok(()) })
    }

    fn have_props(
        &self,
        _path: &dav_server::davpath::DavPath,
        _destination: &std::sync::Arc<crate::destination::Destination>,
    ) -> std::pin::Pin<Box<dyn Future<Output = bool> + Send>> {
        Box::pin(async move { true })
    }

    fn patch_props(
        &self,
        _path: &dav_server::davpath::DavPath,
        patch: Vec<(bool, dav_server::fs::DavProp)>,
        _destination: &std::sync::Arc<crate::destination::Destination>,
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

    fn get_props(
        &self,
        _path: &dav_server::davpath::DavPath,
        _do_content: bool,
        _destination: &std::sync::Arc<crate::destination::Destination>,
    ) -> dav_server::fs::FsFuture<Vec<dav_server::fs::DavProp>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn get_prop(
        &self,
        _path: &dav_server::davpath::DavPath,
        prop: dav_server::fs::DavProp,
        destination: &std::sync::Arc<crate::destination::Destination>,
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
        mut self,
        dav_handler: std::sync::Arc<
            dav_server::DavHandler<
                std::sync::Arc<crate::destination::Destination>,
            >,
        >,
    ) -> ! {
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
}

#[derive(serde::Deserialize)]
pub struct ConfigListenerWebdav {
    listen_on: Vec<std::net::SocketAddr>,
    certificate_chain: scan2blob::util::LiteralOrFile,
    private_key: scan2blob::util::LiteralOrFile,
    users: std::collections::HashMap<String, ConfigListenerWebdavUser>,
}

struct WebdavListenerUser {
    password: String,
    destination: std::sync::Arc<crate::destination::Destination>,
}

pub struct WebdavListener {
    ctx: std::sync::Arc<crate::ctx::Ctx>,
    listen_on: Vec<std::net::SocketAddr>,
    rustls_acceptor: tokio_rustls::TlsAcceptor,
    hyper_builder:
        hyper_util::server::conn::auto::Builder<hyper_util::rt::TokioExecutor>,
    users: std::collections::HashMap<String, WebdavListenerUser>,
    basic_auth_regex: regex::Regex,
}

impl WebdavListener {
    pub fn new(
        ctx: &std::sync::Arc<crate::ctx::Ctx>,
        config: &ConfigListenerWebdav,
        destinations: &crate::destination::Destinations,
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
        let mut users: std::collections::HashMap<String, WebdavListenerUser> =
            std::collections::HashMap::new();
        for (
            username,
            ConfigListenerWebdavUser {
                password,
                destination,
            },
        ) in &config.users
        {
            let Some(destination) = destinations.get(destination) else {
                return Err(scan2blob::error::WuffError::from(
                    "Destination not found",
                ));
            };
            let _ = users.insert(
                username.clone(),
                WebdavListenerUser {
                    password: password.clone(),
                    destination,
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
        let basic_auth_regex: regex::Regex =
            regex::Regex::new(r"^\s*Basic\s+(.+?)\s*$").expect("regex");
        Ok(Self {
            ctx: std::sync::Arc::clone(ctx),
            listen_on: config.listen_on.clone(),
            rustls_acceptor,
            hyper_builder,
            users,
            basic_auth_regex,
        })
    }

    pub fn start(self: &std::sync::Arc<Self>) {
        let async_spawner = self.ctx.base_ctx.get_async_spawner();
        let dav_handler: std::sync::Arc<
            dav_server::DavHandler<
                std::sync::Arc<crate::destination::Destination>,
            >,
        > = std::sync::Arc::new(
            dav_server::DavHandler::builder()
                .filesystem(Box::new(WebdavFilesystem(std::sync::Arc::clone(
                    self,
                ))))
                .locksystem(dav_server::fakels::FakeLs::new())
                .build_handler(),
        );

        for listen_on in &self.listen_on {
            async_spawner.spawn(
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
            dav_server::DavHandler<
                std::sync::Arc<crate::destination::Destination>,
            >,
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
                dav_server::DavHandler<
                    std::sync::Arc<crate::destination::Destination>,
                >,
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
            dav_server::DavHandler<
                std::sync::Arc<crate::destination::Destination>,
            >,
        >,
        req: hyper::Request<ReqBody>,
    ) -> Result<hyper::Response<dav_server::body::Body>, hyper::http::Error>
    where
        ReqData: hyper::body::Buf + Send + 'static,
        ReqError: std::error::Error + Send + Sync + 'static,
        ReqBody: hyper::body::Body<Data = ReqData, Error = ReqError>,
    {
        let headers: &hyper::HeaderMap = req.headers();

        let destination: Option<
            std::sync::Arc<crate::destination::Destination>,
        > = if let Some(auth_header) =
            headers.get(hyper::header::AUTHORIZATION)
        {
            self.check_auth(auth_header)
        } else {
            None
        };

        let Some(destination) = destination else {
            return hyper::Response::builder()
                .status(hyper::StatusCode::UNAUTHORIZED)
                .header(
                    hyper::header::WWW_AUTHENTICATE,
                    "Basic realm=\"scan2blob\"",
                )
                .body(dav_server::body::Body::empty());
        };

        Ok(dav_handler.handle_guarded(req, destination).await)
    }

    fn check_auth(
        &self,
        auth_header: &hyper::header::HeaderValue,
    ) -> Option<std::sync::Arc<crate::destination::Destination>> {
        let Ok(auth_header) = auth_header.to_str() else {
            return None;
        };

        let Some(m) = self.basic_auth_regex.captures(auth_header) else {
            return None;
        };

        let Ok(userpass) = base64::Engine::decode(
            &base64::prelude::BASE64_STANDARD,
            m.get(1).unwrap().as_str(),
        ) else {
            return None;
        };

        let Ok(userpass) = String::from_utf8(userpass) else {
            return None;
        };

        let Some((username, plaintext)) = userpass.split_once(':') else {
            return None;
        };

        let Some(WebdavListenerUser {
            password,
            destination,
        }) = self.users.get(username)
        else {
            return None;
        };

        if scan2blob::pwhash::verify(plaintext, password) {
            Some(std::sync::Arc::clone(destination))
        } else {
            None
        }
    }
}
