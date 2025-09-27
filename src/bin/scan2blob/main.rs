mod ctx;
mod destination;
mod gate;
mod listener;
mod mime_types;

async fn async_main(
    ctx: std::sync::Arc<crate::ctx::Ctx>,
) -> Result<(), scan2blob::error::WuffError> {
    let destinations: destination::Destinations =
        destination::Destinations::new(&ctx)?;
    let gates: gate::Gates = gate::Gates::new(&ctx)?;
    for (gate_name, gate_cfg) in &ctx.config.gates {
        let gate: std::sync::Arc<gate::Gate> = gates.get(gate_name).unwrap();
        if let Some(ref web_ui_cfg) = gate_cfg.web_ui {
            let web_ui: std::sync::Arc<gate::web::GateWebListener> =
                std::sync::Arc::new(gate::web::GateWebListener::new(
                    &ctx, &gate, web_ui_cfg,
                )?);
            web_ui.start();
        }
    }
    for listener_cfg in &ctx.config.listeners {
        match listener_cfg {
            ctx::ConfigListener::Sftp(listener_cfg) => {
                let listener: std::sync::Arc<listener::sftp::SftpListener> =
                    std::sync::Arc::new(listener::sftp::SftpListener::new(
                        &ctx,
                        listener_cfg,
                        &destinations,
                        &gates,
                    )?);
                listener.start();
            }
            ctx::ConfigListener::Webdav(listener_cfg) => {
                let listener: std::sync::Arc<
                    listener::webdav::WebdavListener,
                > = std::sync::Arc::new(
                    listener::webdav::WebdavListener::new(
                        &ctx,
                        listener_cfg,
                        &destinations,
                        &gates,
                    )?,
                );
                listener.start();
            }
        }
    }

    let mut sigint: tokio::signal::unix::Signal = tokio::signal::unix::signal(
        tokio::signal::unix::SignalKind::interrupt(),
    )?;
    let mut sigterm: tokio::signal::unix::Signal =
        tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate(),
        )?;
    futures::select! {
        _ = futures::FutureExt::fuse(sigint.recv()) => Ok(()),
        _ = futures::FutureExt::fuse(sigterm.recv()) => Ok(()),
        err = futures::FutureExt::fuse(ctx.shutdown_due_to_error.wait()) =>
            Err(err.clone()),
    }
}

fn main() -> Result<(), scan2blob::error::WuffError> {
    // Set a process-wide default crypto provider that will be used by anything
    // that is based on rustls. e.g., this is what will be used by WebDAV, but
    // it is not relevant to sftp or the Azure blob stuff.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install default crypto provider");

    let cmdline_parser: clap::Command = crate::ctx::make_cmdline_parser();
    let cmdline_matches: clap::ArgMatches = cmdline_parser.get_matches();
    let foreground: bool =
        *(cmdline_matches.get_one::<bool>("foreground").unwrap());
    let foreground_with_syslog: bool = *(cmdline_matches
        .get_one::<bool>("foreground_with_syslog")
        .unwrap());
    let parsed_config: crate::ctx::ParsedConfig =
        crate::ctx::ParsedConfig::new(&cmdline_matches);
    if parsed_config.daemonize {
        daemonize::Daemonize::new()
            .stdout(daemonize::Stdio::keep())
            .stderr(daemonize::Stdio::keep())
            .start()?;
    }

    let ctx: std::sync::Arc<crate::ctx::Ctx> =
        std::sync::Arc::new(parsed_config.into());
    ctx.install_panic_logger();

    let _pid_file: Option<PidFile> =
        if let Some(pid_filename) = ctx.pid_filename.as_ref() {
            Some(PidFile::new(pid_filename)?)
        } else {
            None
        };

    ctx.log_err("running");
    let main_result: Result<(), scan2blob::error::WuffError> = ctx
        .base_ctx
        .run_async_main(async_main(std::sync::Arc::clone(&ctx)));
    if ctx.daemonize {
        if let Err(ref err) = main_result {
            ctx.log_err(format!("{}", err));
        }
    }
    main_result
}

struct PidFile {
    filename: std::path::PathBuf,
}

impl PidFile {
    fn new(
        filename: &std::path::Path,
    ) -> Result<Self, scan2blob::error::WuffError> {
        let pid: u32 = std::process::id();
        let pid_as_str: String = format!("{}\n", pid);
        std::fs::write(filename, pid_as_str)?;
        Ok(Self {
            filename: filename.into(),
        })
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.filename);
    }
}
