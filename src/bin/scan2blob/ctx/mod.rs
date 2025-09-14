const DEFAULT_CONFIG_FILENAME: &str = "/usr/local/etc/scan2blob.json";

pub fn make_cmdline_parser() -> clap::Command {
    scan2blob::util::make_cmdline_parser("scan2blob").arg(
        clap::Arg::new("config_file")
            .long("configuration")
            .short('c')
            .value_parser(clap::value_parser!(std::path::PathBuf))
            .default_value(DEFAULT_CONFIG_FILENAME)
            .action(clap::ArgAction::Set),
    )
}

#[derive(serde::Deserialize)]
pub struct ConfigDestination {
    #[serde(flatten)]
    pub blob_storage_spec: scan2blob::util::BlobStorageSpec,
}

#[derive(serde::Deserialize)]
pub struct Config {
    pub destinations: std::collections::HashMap<String, ConfigDestination>,
}

pub struct Ctx {
    pub base_ctx: std::sync::Arc<scan2blob::ctx::Ctx>,
    pub config: Config,
}

impl Ctx {
    pub fn new(cmdline_matches: &clap::ArgMatches) -> Self {
        let config_filename: &std::path::PathBuf = cmdline_matches
            .get_one::<std::path::PathBuf>("config_file")
            .unwrap();
        let config_filename_as_str = config_filename.to_string_lossy();
        let f: std::fs::File = std::fs::File::open(config_filename)
            .expect(&config_filename_as_str);
        let f: std::io::BufReader<_> = std::io::BufReader::new(f);
        let config: Config =
            serde_json::from_reader(f).expect(&config_filename_as_str);
        Self {
            base_ctx: std::sync::Arc::new(scan2blob::ctx::Ctx::new()),
            config,
        }
    }
}
