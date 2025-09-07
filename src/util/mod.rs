pub fn make_cmdline_parser(argv0: &'static str) -> clap::Command {
    clap::Command::new(argv0).color(clap::ColorChoice::Never)
}
