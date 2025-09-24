// Copied and pasted from
// https://github.com/Eugeny/russh/blob/main/russh/examples/sftp_server.rs

#[derive(Clone)]
struct DestinationAndGate {
    destination: std::sync::Arc<crate::destination::Destination>,
    gate: std::sync::Arc<crate::gate::Gate>,
}

struct SshConnection {
    sftp_listener: std::sync::Arc<SftpListener>,
    authenticated_destination_and_gate: Option<DestinationAndGate>,

    // These are channels that have been opened with SSH_MSG_CHANNEL_OPEN, but
    // not yet attached to a subsystem with SSH_MSG_CHANNEL_REQUEST. (Obviously
    // we're hoping that the client requests the sftp subsystem for them...)
    pending_channels: std::collections::HashMap<
        russh::ChannelId,
        russh::Channel<russh::server::Msg>,
    >,
}

impl SshConnection {
    fn new(sftp_listener: &std::sync::Arc<SftpListener>) -> Self {
        Self {
            sftp_listener: std::sync::Arc::clone(sftp_listener),
            authenticated_destination_and_gate: None,
            pending_channels: std::collections::HashMap::new(),
        }
    }
}

impl russh::server::Handler for SshConnection {
    type Error = scan2blob::error::WuffError;

    async fn auth_publickey_offered(
        &mut self,
        _user: &str,
        public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<russh::server::Auth, Self::Error> {
        Ok(
            if self
                .sftp_listener
                .authorized_keys
                .contains_key(public_key.key_data())
            {
                russh::server::Auth::Accept
            } else {
                russh::server::Auth::reject()
            },
        )
    }

    async fn auth_publickey(
        &mut self,
        _user: &str,
        public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<russh::server::Auth, Self::Error> {
        let Some(destination_and_gate) = self
            .sftp_listener
            .authorized_keys
            .get(public_key.key_data())
        else {
            return Ok(russh::server::Auth::reject());
        };

        self.authenticated_destination_and_gate =
            Some(destination_and_gate.clone());
        Ok(russh::server::Auth::Accept)
    }

    async fn channel_open_session(
        &mut self,
        channel: russh::Channel<russh::server::Msg>,
        _session: &mut russh::server::Session,
    ) -> Result<bool, Self::Error> {
        self.pending_channels.insert(channel.id(), channel);
        Ok(true)
    }

    async fn channel_eof(
        &mut self,
        channel: russh::ChannelId,
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        session.close(channel)?;
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel_id: russh::ChannelId,
        name: &str,
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        if name != "sftp" {
            session.channel_failure(channel_id)?;
            return Ok(());
        }

        let Some(channel) = self.pending_channels.remove(&channel_id) else {
            session.channel_failure(channel_id)?;
            return Ok(());
        };

        let Some(authenticated_destination_and_gate) =
            self.authenticated_destination_and_gate.as_ref()
        else {
            session.channel_failure(channel_id)?;
            return Ok(());
        };

        let sftp_session: SftpSession = SftpSession::new(
            &self.sftp_listener.ctx,
            authenticated_destination_and_gate,
        );
        session.channel_success(channel_id)?;
        russh_sftp::server::run(channel.into_stream(), sftp_session).await;

        Ok(())
    }
}

struct OpenFile {
    orig_filename: String,
    writer: scan2blob::chunker::Writer,
    off: u64,
    expected_file_size: Option<u64>,
}

struct SftpSession {
    ctx: std::sync::Arc<crate::ctx::Ctx>,
    destination_and_gate: DestinationAndGate,
    open_files: std::collections::HashMap<String, OpenFile>,
    next_handle: std::sync::atomic::AtomicU64,
}

impl SftpSession {
    fn new(
        ctx: &std::sync::Arc<crate::ctx::Ctx>,
        destination_and_gate: &DestinationAndGate,
    ) -> Self {
        Self {
            ctx: std::sync::Arc::clone(&ctx),
            destination_and_gate: destination_and_gate.clone(),
            open_files: std::collections::HashMap::new(),
            next_handle: std::sync::atomic::AtomicU64::new(0),
        }
    }

    fn get_next_handle(&self) -> String {
        let i: u64 = self
            .next_handle
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("{}", i)
    }
}

impl russh_sftp::server::Handler for SftpSession {
    type Error = russh_sftp::protocol::StatusCode;

    fn unimplemented(&self) -> Self::Error {
        russh_sftp::protocol::StatusCode::OpUnsupported
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        pflags: russh_sftp::protocol::OpenFlags,
        attrs: russh_sftp::protocol::FileAttributes,
    ) -> Result<russh_sftp::protocol::Handle, Self::Error> {
        if pflags.contains(russh_sftp::protocol::OpenFlags::READ)
            || !(pflags.contains(russh_sftp::protocol::OpenFlags::WRITE)
                || pflags.contains(russh_sftp::protocol::OpenFlags::APPEND))
            || !(pflags.contains(russh_sftp::protocol::OpenFlags::TRUNCATE)
                || pflags.contains(russh_sftp::protocol::OpenFlags::EXCLUDE))
        {
            return Err(self.unimplemented());
        }
        let Some(writer) = self
            .destination_and_gate
            .gate
            .try_write_file(&filename, &self.destination_and_gate.destination)
        else {
            self.ctx.log(format!(
                "sftp: rejecting file upload because gate {} is closed",
                self.destination_and_gate.gate.name
            ));
            return Err(russh_sftp::protocol::StatusCode::PermissionDenied);
        };
        let handle: String = self.get_next_handle();
        assert!(
            self.open_files
                .insert(
                    handle.clone(),
                    OpenFile {
                        writer,
                        off: 0,
                        orig_filename: filename,
                        expected_file_size: attrs.size
                    }
                )
                .is_none()
        );
        Ok(russh_sftp::protocol::Handle { id, handle })
    }

    async fn close(
        &mut self,
        id: u32,
        handle: String,
    ) -> Result<russh_sftp::protocol::Status, Self::Error> {
        let Some(mut open_file) = self.open_files.remove(&handle) else {
            return Ok(russh_sftp::protocol::Status {
                id,
                status_code: russh_sftp::protocol::StatusCode::Ok,
                error_message: "Ok".to_string(),
                language_tag: "en-US".to_string(),
            });
        };

        if let Some(expected_file_size) = open_file.expected_file_size {
            if open_file.off != expected_file_size {
                let msg: String = format!(
                    "sftp: aborting upload of {}, {} bytes were written but there were supposed to be {} bytes",
                    open_file.orig_filename, open_file.off, expected_file_size
                );
                self.ctx.log(&msg);
                return Ok(russh_sftp::protocol::Status {
                    id,
                    status_code: russh_sftp::protocol::StatusCode::Failure,
                    error_message: msg,
                    language_tag: "en-US".to_string(),
                });
            }
        }

        if let Err(err) = open_file.writer.finalize().await {
            self.ctx.log(format!(
                "sftp: aborting upload of {} due to propagated error: {}",
                open_file.orig_filename, err
            ));
            return Ok(russh_sftp::protocol::Status {
                id,
                status_code: russh_sftp::protocol::StatusCode::Failure,
                error_message: format!("{}", err),
                language_tag: "en-US".to_string(),
            });
        }

        Ok(russh_sftp::protocol::Status {
            id,
            status_code: russh_sftp::protocol::StatusCode::Ok,
            error_message: "Ok".to_string(),
            language_tag: "en-US".to_string(),
        })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<russh_sftp::protocol::Status, Self::Error> {
        let Some(open_file) = self.open_files.get_mut(&handle) else {
            return Ok(russh_sftp::protocol::Status {
                id,
                status_code: russh_sftp::protocol::StatusCode::Ok,
                error_message: "Ok".to_string(),
                language_tag: "en-US".to_string(),
            });
        };

        if offset != open_file.off {
            self.ctx.log(format!(
                "sftp: aborting upload of {} because client attempted a random-access write, which is not supported",
                open_file.orig_filename
            ));
            open_file
                .writer
                .observe_error(scan2blob::error::WuffError::from(
                    "sftp client attempted random-access write which is not supported",
                ));
            return Ok(russh_sftp::protocol::Status {
                id,
                status_code: russh_sftp::protocol::StatusCode::OpUnsupported,
                error_message: "Random-access writing is not supported"
                    .to_string(),
                language_tag: "en-US".to_string(),
            });
        }

        if let Err(err) = open_file.writer.write(&data).await {
            self.ctx.log(format!(
                "sftp: aborting upload of {} due to propagated error: {}",
                open_file.orig_filename, err
            ));
            return Ok(russh_sftp::protocol::Status {
                id,
                status_code: russh_sftp::protocol::StatusCode::Failure,
                error_message: format!("{}", err),
                language_tag: "en-US".to_string(),
            });
        }
        open_file.off += data.len() as u64;

        Ok(russh_sftp::protocol::Status {
            id,
            status_code: russh_sftp::protocol::StatusCode::Ok,
            error_message: "Ok".to_string(),
            language_tag: "en-US".to_string(),
        })
    }

    async fn opendir(
        &mut self,
        id: u32,
        _path: String,
    ) -> Result<russh_sftp::protocol::Handle, Self::Error> {
        Ok(russh_sftp::protocol::Handle {
            id,
            handle: self.get_next_handle(),
        })
    }

    async fn readdir(
        &mut self,
        _id: u32,
        _handle: String,
    ) -> Result<russh_sftp::protocol::Name, Self::Error> {
        Err(russh_sftp::protocol::StatusCode::Eof)
    }

    async fn mkdir(
        &mut self,
        id: u32,
        _path: String,
        _attrs: russh_sftp::protocol::FileAttributes,
    ) -> Result<russh_sftp::protocol::Status, Self::Error> {
        Ok(russh_sftp::protocol::Status {
            id,
            status_code: russh_sftp::protocol::StatusCode::Ok,
            error_message: "Ok".to_string(),
            language_tag: "en-US".to_string(),
        })
    }

    async fn realpath(
        &mut self,
        id: u32,
        _path: String,
    ) -> Result<russh_sftp::protocol::Name, Self::Error> {
        //info!("realpath: {}", path);
        Ok(russh_sftp::protocol::Name {
            id,
            files: vec![russh_sftp::protocol::File::dummy("/")],
        })
    }

    async fn rename(
        &mut self,
        id: u32,
        _oldpath: String,
        _newpath: String,
    ) -> Result<russh_sftp::protocol::Status, Self::Error> {
        Ok(russh_sftp::protocol::Status {
            id,
            status_code: russh_sftp::protocol::StatusCode::Ok,
            error_message: "Ok".to_string(),
            language_tag: "en-US".to_string(),
        })
    }
}

#[derive(Clone)]
struct SftpListenerEachPort {
    sftp_listener: std::sync::Arc<SftpListener>,
    listen_on: std::net::SocketAddr,
}

impl SftpListenerEachPort {
    async fn run(mut self) -> ! {
        let russh_config: russh::server::Config = russh::server::Config {
            methods: self.sftp_listener.auth_methods.clone(),
            keys: vec![self.sftp_listener.host_key.clone()],
            ..Default::default()
        };

        let listen_on: std::net::SocketAddr = self.listen_on;
        russh::server::Server::run_on_address(
            &mut self,
            std::sync::Arc::new(russh_config),
            listen_on,
        )
        .await
        .unwrap();
        panic!("run_on_address exited");
    }
}

impl russh::server::Server for SftpListenerEachPort {
    type Handler = SshConnection;

    fn new_client(
        &mut self,
        _: Option<std::net::SocketAddr>,
    ) -> Self::Handler {
        SshConnection::new(&self.sftp_listener)
    }
}

#[derive(serde::Deserialize)]
pub struct ConfigListenerSftpAuthorizedKey {
    public_key: String,
    destination: String,
    gate: String,
}

#[derive(serde::Deserialize)]
pub struct ConfigListenerSftp {
    listen_on: Vec<std::net::SocketAddr>,
    host_key: scan2blob::util::LiteralOrFile,
    authorized_keys: Vec<ConfigListenerSftpAuthorizedKey>,
}

pub struct SftpListener {
    ctx: std::sync::Arc<crate::ctx::Ctx>,
    listen_on: Vec<std::net::SocketAddr>,
    host_key: russh::keys::PrivateKey,
    // You would think that russh::keys::PublicKey would implement Hash and
    // Eq, but it doesn't, or at least not in any way that makes sense. So we
    // have to use this freakish KeyData thing...
    authorized_keys: std::collections::HashMap<
        internal_russh_forked_ssh_key::public::KeyData,
        DestinationAndGate,
    >,
    auth_methods: russh::MethodSet,
}

impl SftpListener {
    pub fn new(
        ctx: &std::sync::Arc<crate::ctx::Ctx>,
        config: &ConfigListenerSftp,
        destinations: &crate::destination::Destinations,
        gates: &crate::gate::Gates,
    ) -> Result<Self, scan2blob::error::WuffError> {
        let host_key: String = config.host_key.get()?;
        // Can't use the ? operator on russh::keys::PrivateKey::from_openssh()
        // because it returns a weird Result type...
        let host_key: russh::keys::PrivateKey =
            match russh::keys::PrivateKey::from_openssh(&host_key) {
                Ok(key) => key,
                Err(_) => {
                    return Err(scan2blob::error::WuffError::from(
                        "Error parsing ssh host key",
                    ));
                }
            };
        let mut authorized_keys: std::collections::HashMap<
            internal_russh_forked_ssh_key::public::KeyData,
            DestinationAndGate,
        > = std::collections::HashMap::new();
        for ConfigListenerSftpAuthorizedKey {
            public_key,
            destination,
            gate,
        } in &config.authorized_keys
        {
            // Can't use the ? operator on russh::keys::PublicKey::from_openssh
            // because it returns a weird Result type...
            let public_key: russh::keys::PublicKey =
                match russh::keys::PublicKey::from_openssh(public_key) {
                    Ok(key) => key,
                    Err(_) => {
                        return Err(scan2blob::error::WuffError::from(
                            "Error parsing user public key",
                        ));
                    }
                };
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
            if authorized_keys
                .insert(
                    public_key.key_data().clone(),
                    DestinationAndGate { destination, gate },
                )
                .is_some()
            {
                return Err(scan2blob::error::WuffError::from(
                    "Duplicate user public key",
                ));
            }
        }
        let mut auth_methods: russh::MethodSet = russh::MethodSet::empty();
        auth_methods.push(russh::MethodKind::PublicKey);
        Ok(Self {
            ctx: std::sync::Arc::clone(ctx),
            listen_on: config.listen_on.clone(),
            host_key,
            authorized_keys,
            auth_methods,
        })
    }

    pub fn start(self: &std::sync::Arc<Self>) {
        let async_spawner = self.ctx.base_ctx.get_async_spawner();

        for listen_on in &self.listen_on {
            async_spawner.spawn(
                SftpListenerEachPort {
                    sftp_listener: std::sync::Arc::clone(self),
                    listen_on: *listen_on,
                }
                .run(),
            );
        }
    }
}
