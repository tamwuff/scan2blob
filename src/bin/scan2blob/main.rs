mod ctx;
mod destination;
mod gate;
mod listener;
mod mime_types;

async fn async_main(
    ctx: std::sync::Arc<ctx::Ctx>,
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
            listener::ConfigListenerEnriched::Sftp(listener_cfg) => {
                let listener: std::sync::Arc<listener::sftp::SftpListener> =
                    std::sync::Arc::new(listener::sftp::SftpListener::new(
                        &ctx,
                        listener_cfg,
                        &destinations,
                        &gates,
                    )?);
                listener.start();
            }
            listener::ConfigListenerEnriched::Webdav(listener_cfg) => {
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

fn main() {
    // Set a process-wide default crypto provider that will be used by anything
    // that is based on rustls. e.g., this is what will be used by WebDAV, but
    // it is not relevant to sftp or the Azure blob stuff.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install default crypto provider");

    let cmdline_parser: clap::Command = ctx::make_cmdline_parser();
    let cmdline_matches: clap::ArgMatches = cmdline_parser.get_matches();
    let foreground: bool =
        *(cmdline_matches.get_one::<bool>("foreground").unwrap());
    let foreground_with_syslog: bool = *(cmdline_matches
        .get_one::<bool>("foreground_with_syslog")
        .unwrap());
    let logger: std::sync::Arc<ctx::Logger> =
        std::sync::Arc::new(ctx::Logger::new(&cmdline_matches));
    logger.install_panic_logger();
    let config: ctx::ConfigEnriched =
        match ctx::ConfigEnriched::new(&cmdline_matches) {
            Ok(config) => config,
            Err(e) => {
                logger.log_err(format!("{}", e));
                std::process::exit(1);
            }
        };
    if !(foreground || foreground_with_syslog) {
        if let Err(err) = daemonize::Daemonize::new()
            .start()
        {
            logger.log_err(format!("{}", err));
            std::process::exit(1);
        }
        logger.inform_daemonized();
    }

    let ctx: std::sync::Arc<ctx::Ctx> =
        std::sync::Arc::new(ctx::Ctx::new(&logger, config));

    let _pid_file: Option<PidFile> = if let Some(pid_filename) =
        cmdline_matches.get_one::<std::path::PathBuf>("pid_file")
    {
        Some(match PidFile::new(pid_filename) {
            Ok(pid_file) => pid_file,
            Err(err) => {
                logger.log_err(format!("{}", err));
                std::process::exit(1);
            }
        })
    } else {
        None
    };

    ctx.log_err("running");
    if let Err(err) = ctx
        .base_ctx
        .run_async_main(async_main(std::sync::Arc::clone(&ctx)))
    {
        ctx.log_info(format!("{}", err));
        std::process::exit(1);
    }
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
