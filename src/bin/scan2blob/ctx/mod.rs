const DEFAULT_CONFIG_FILENAME: &str = "/usr/local/etc/scan2blob.json";

pub fn make_cmdline_parser() -> clap::Command {
    scan2blob::util::make_cmdline_parser("scan2blob")
        .arg(
            clap::Arg::new("config_file")
                .long("configuration")
                .short('c')
                .value_parser(clap::value_parser!(std::path::PathBuf))
                .default_value(DEFAULT_CONFIG_FILENAME)
                .action(clap::ArgAction::Set),
        )
        .arg(
            clap::Arg::new("pid_file")
                .long("pid-file")
                .short('p')
                .value_parser(clap::value_parser!(std::path::PathBuf))
                .action(clap::ArgAction::Set),
        )
        .arg(
            clap::Arg::new("debug")
                .long("debug")
                .short('d')
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("foreground")
                .long("foreground")
                .short('f')
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("foreground_with_syslog")
                .long("foreground-with-syslog")
                .short('F')
                .action(clap::ArgAction::SetTrue),
        )
}

#[derive(serde::Deserialize)]
pub struct Config {
    pub listeners: Vec<crate::listener::ConfigListener>,
    pub gates: crate::gate::ConfigGates,
    pub destinations: crate::destination::ConfigDestinations,
    #[serde(default = "crate::mime_types::default_mime_types")]
    pub mime_types: crate::mime_types::ConfigMimeTypes,
}

pub struct ConfigEnriched {
    pub listeners: Vec<crate::listener::ConfigListenerEnriched>,
    pub gates: crate::gate::ConfigGatesEnriched,
    pub destinations: crate::destination::ConfigDestinationsEnriched,
    pub mime_types: crate::mime_types::ConfigMimeTypesEnriched,
}

impl TryFrom<Config> for ConfigEnriched {
    type Error = scan2blob::error::WuffError;
    fn try_from(config: Config) -> Result<Self, scan2blob::error::WuffError> {
        let Config {
            listeners,
            gates,
            destinations,
            mime_types,
        } = config;
        let mut enriched_listeners: Vec<
            crate::listener::ConfigListenerEnriched,
        > = Vec::with_capacity(listeners.len());
        for listener in listeners {
            enriched_listeners.push(listener.try_into()?);
        }
        let mut enriched_gates: crate::gate::ConfigGatesEnriched =
            std::collections::HashMap::new();
        for (name, gate) in gates {
            assert!(enriched_gates.insert(name, gate.try_into()?).is_none());
        }
        let mut enriched_destinations: crate::destination::ConfigDestinationsEnriched =
            std::collections::HashMap::new();
        for (name, destination) in destinations {
            assert!(
                enriched_destinations
                    .insert(name, destination.try_into()?)
                    .is_none()
            );
        }
        Ok(Self {
            listeners: enriched_listeners,
            gates: enriched_gates,
            destinations: enriched_destinations,
            mime_types: mime_types.try_into()?,
        })
    }
}

impl ConfigEnriched {
    pub fn new(
        cmdline_matches: &clap::ArgMatches,
    ) -> Result<Self, scan2blob::error::WuffError> {
        let config_filename: &std::path::PathBuf = cmdline_matches
            .get_one::<std::path::PathBuf>("config_file")
            .unwrap();
        let _config_filename_as_str = config_filename.to_string_lossy();
        let f: std::fs::File = std::fs::File::open(config_filename)?;
        let f: std::io::BufReader<_> = std::io::BufReader::new(f);
        let config: Config = serde_json::from_reader(f)?;
        config.try_into()
    }
}

pub struct Logger {
    pub debug: bool,
    log_to_stderr: std::sync::atomic::AtomicBool,
    log_to_syslog: Option<Syslog>,
}

impl Logger {
    pub fn new(cmdline_matches: &clap::ArgMatches) -> Self {
        let foreground: bool =
            *(cmdline_matches.get_one::<bool>("foreground").unwrap());
        let foreground_with_syslog: bool = *(cmdline_matches
            .get_one::<bool>("foreground_with_syslog")
            .unwrap());
        let log_to_syslog: Option<Syslog> =
            if foreground_with_syslog || !foreground {
                Some(Syslog::new())
            } else {
                None
            };
        Self {
            debug: *(cmdline_matches.get_one::<bool>("debug").unwrap()),
            log_to_stderr: std::sync::atomic::AtomicBool::new(true),
            log_to_syslog,
        }
    }

    pub fn log_debug<T: AsRef<str>>(&self, s: T) {
        if !self.debug {
            return;
        }
        if self
            .log_to_stderr
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            log_to_stderr("DEBUG", s.as_ref());
        }
        if let Some(ref syslog) = self.log_to_syslog {
            syslog.log_debug(s.as_ref());
        }
    }

    pub fn log_info<T: AsRef<str>>(&self, s: T) {
        if self
            .log_to_stderr
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            log_to_stderr("INFO", s.as_ref());
        }
        if let Some(ref syslog) = self.log_to_syslog {
            syslog.log_info(s.as_ref());
        }
    }

    pub fn log_warn<T: AsRef<str>>(&self, s: T) {
        if self
            .log_to_stderr
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            log_to_stderr("WARNING", s.as_ref());
        }
        if let Some(ref syslog) = self.log_to_syslog {
            syslog.log_warn(s.as_ref());
        }
    }

    pub fn log_err<T: AsRef<str>>(&self, s: T) {
        if self
            .log_to_stderr
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            log_to_stderr("ERROR", s.as_ref());
        }
        if let Some(ref syslog) = self.log_to_syslog {
            syslog.log_err(s.as_ref());
        }
    }

    pub fn install_panic_logger(self: &std::sync::Arc<Self>) {
        // This is a compromise. I don't want to call through to the default
        // hook, but I'm also not willing to replicate all of the default
        // hook's functionality. So I'm going to log an error, but then let the
        // default hook take over...
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new({
            let self_: std::sync::Arc<Self> = std::sync::Arc::clone(self);
            move |panic_info| {
                self_.log_err(format!("panic: {}", panic_info));
                prev_hook(panic_info)
            }
        }));
    }

    pub fn inform_daemonized(&self) {
        self.log_to_stderr
            .store(false, std::sync::atomic::Ordering::Relaxed);

        if let Some(ref syslog) = self.log_to_syslog {
            syslog.regenerate_after_fork();
        }
    }
}

pub struct Ctx {
    pub base_ctx: std::sync::Arc<scan2blob::ctx::Ctx>,
    pub config: ConfigEnriched,
    pub logger: std::sync::Arc<Logger>,
    pub shutdown_due_to_error:
        tokio::sync::SetOnce<scan2blob::error::WuffError>,
}

impl Ctx {
    pub fn new(
        logger: &std::sync::Arc<Logger>,
        config: ConfigEnriched,
    ) -> Self {
        Self {
            base_ctx: std::sync::Arc::new(scan2blob::ctx::Ctx::new()),
            config,
            logger: std::sync::Arc::clone(logger),
            shutdown_due_to_error: tokio::sync::SetOnce::new(),
        }
    }

    // A critical task is one where if it dies, the whole process should die.
    pub fn spawn_critical<S, F>(self: &std::sync::Arc<Self>, name: S, fut: F)
    where
        S: AsRef<str> + Send + 'static,
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let async_spawner = self.base_ctx.get_async_spawner();
        let task: tokio::task::JoinHandle<()> = async_spawner.spawn(fut);
        async_spawner.spawn({
            let self_: std::sync::Arc<Self> = std::sync::Arc::clone(self);
            async move {
                let err: scan2blob::error::WuffError =
                    if let Err(e) = task.await {
                        scan2blob::error::WuffError::from(format!(
                            "{}: {}",
                            name.as_ref(),
                            e
                        ))
                    } else {
                        scan2blob::error::WuffError::from(format!(
                            "{}: should not have exited",
                            name.as_ref()
                        ))
                    };
                let _ = self_.shutdown_due_to_error.set(err);
            }
        });
    }

    pub fn log_debug<T: AsRef<str>>(&self, s: T) {
        self.logger.log_debug(s);
    }

    pub fn log_info<T: AsRef<str>>(&self, s: T) {
        self.logger.log_info(s);
    }

    pub fn log_warn<T: AsRef<str>>(&self, s: T) {
        self.logger.log_warn(s);
    }

    pub fn log_err<T: AsRef<str>>(&self, s: T) {
        self.logger.log_err(s);
    }
}

fn log_to_stderr(level: &'static str, s: &str) {
    let now: std::time::SystemTime = std::time::SystemTime::now();
    eprintln!(
        "{} {} {}",
        scan2blob::util::system_time_to_utc_rfc3339(now),
        level,
        s
    );
}

struct SyslogInner {
    logger: syslog::Logger<syslog::LoggerBackend, syslog::Formatter3164>,
}

impl SyslogInner {
    fn new() -> Self {
        let pid: u32 = std::process::id();
        Self {
            logger: syslog::unix(syslog::Formatter3164 {
                facility: syslog::Facility::LOG_USER,
                process: "scan2blob".to_string(),
                pid,
                ..Default::default() // automatically detect hostname
            })
            .unwrap(),
        }
    }
}

struct Syslog(std::sync::Mutex<SyslogInner>);

impl Syslog {
    fn new() -> Self {
        Self(std::sync::Mutex::new(SyslogInner::new()))
    }

    fn regenerate_after_fork(&self) {
        let mut inner = self.0.lock().unwrap();
        std::mem::swap(
            std::ops::DerefMut::deref_mut(&mut inner),
            &mut SyslogInner::new(),
        );
    }

    fn log_debug(&self, s: &str) {
        let _ = self.0.lock().unwrap().logger.debug(s);
    }

    fn log_info(&self, s: &str) {
        let _ = self.0.lock().unwrap().logger.info(s);
    }

    fn log_warn(&self, s: &str) {
        let _ = self.0.lock().unwrap().logger.warning(s);
    }

    fn log_err(&self, s: &str) {
        let _ = self.0.lock().unwrap().logger.err(s);
    }
}
