mod ctx;
mod destination;

async fn async_main(
    ctx: std::sync::Arc<crate::ctx::Ctx>,
) -> Result<(), scan2blob::error::WuffError> {
    for (_, destination_cfg) in &ctx.config.destinations {
        let destination: destination::Destination =
            destination::Destination::new(destination_cfg)?;
        destination.test().await;
    }
    Ok(())
}

fn main() -> Result<(), scan2blob::error::WuffError> {
    let cmdline_parser: clap::Command = crate::ctx::make_cmdline_parser();
    let cmdline_matches: clap::ArgMatches = cmdline_parser.get_matches();

    let ctx: std::sync::Arc<crate::ctx::Ctx> =
        std::sync::Arc::new(crate::ctx::Ctx::new(&cmdline_matches));
    ctx.base_ctx
        .run_async_main(async_main(std::sync::Arc::clone(&ctx)))
}
