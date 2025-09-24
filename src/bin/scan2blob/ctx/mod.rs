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
#[serde(tag = "type")]
pub enum ConfigListener {
    #[serde(rename = "sftp")]
    Sftp(crate::listener::sftp::ConfigListenerSftp),
    #[serde(rename = "webdav")]
    Webdav(crate::listener::webdav::ConfigListenerWebdav),
}

#[derive(serde::Deserialize)]
pub struct Config {
    #[serde(default)]
    pub verbose: bool,
    pub listeners: Vec<ConfigListener>,
    pub gates: crate::gate::ConfigGates,
    pub destinations: crate::destination::ConfigDestinations,
    #[serde(default = "crate::mime_types::default_mime_types")]
    pub mime_types: crate::mime_types::ConfigMimeTypes,
}

pub struct Ctx {
    pub base_ctx: std::sync::Arc<scan2blob::ctx::Ctx>,
    pub config: Config,
    pub mime_types: crate::mime_types::MimeTypes,
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
        let mime_types: crate::mime_types::MimeTypes =
            crate::mime_types::MimeTypes::new(&config.mime_types)
                .expect(&config_filename_as_str);
        Self {
            base_ctx: std::sync::Arc::new(scan2blob::ctx::Ctx::new()),
            config,
            mime_types,
        }
    }

    pub fn log<T: AsRef<str>>(&self, s: T) {
        println!("{}", s.as_ref());
    }

    pub fn log_debug<T: AsRef<str>>(&self, s: T) {
        if self.config.verbose {
            println!("{}", s.as_ref());
        }
    }
}
