mod ctx;
mod destination;
mod listener;

async fn async_main(
    ctx: std::sync::Arc<crate::ctx::Ctx>,
) -> Result<(), scan2blob::error::WuffError> {
    let destinations: destination::Destinations =
        destination::Destinations::new(&ctx)?;
    for listener_cfg in &ctx.config.listeners {
        match listener_cfg {
            ctx::ConfigListener::Sftp(listener_cfg) => {
                let listener: listener::sftp::SftpListener =
                    listener::sftp::SftpListener::new(
                        &ctx,
                        listener_cfg,
                        &destinations,
                    )?;
                std::sync::Arc::new(listener).start();
            }
            ctx::ConfigListener::Webdav(listener_cfg) => {
                let listener: listener::webdav::WebdavListener =
                    listener::webdav::WebdavListener::new(
                        &ctx,
                        listener_cfg,
                        &destinations,
                    )?;
                std::sync::Arc::new(listener).start();
            }
        }
    }
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(86400)).await;
    }
}

fn main() -> Result<(), scan2blob::error::WuffError> {
    // Set a process-wide default crypto provider that will be used by anything
    // that is based on rustls. i.e., this is what will be used by WebDAV, but
    // it is not relevant to sftp or the Azure blob stuff.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install default crypto provider");

    let cmdline_parser: clap::Command = crate::ctx::make_cmdline_parser();
    let cmdline_matches: clap::ArgMatches = cmdline_parser.get_matches();

    let ctx: std::sync::Arc<crate::ctx::Ctx> =
        std::sync::Arc::new(crate::ctx::Ctx::new(&cmdline_matches));
    ctx.base_ctx
        .run_async_main(async_main(std::sync::Arc::clone(&ctx)))
}
