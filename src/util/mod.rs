pub fn make_cmdline_parser(argv0: &'static str) -> clap::Command {
    clap::Command::new(argv0).color(clap::ColorChoice::Never)
}

// These types are shared between the main scan2blob server binary, and the
// "mksas" binary. Most of the structs that define the server's config file are
// in the server's own crate, but these ones need to be shared.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Sas {
    Literal(String),
    EnvironmentVariable { env: String },
}

impl Sas {
    pub fn get(&self) -> Result<String, crate::error::WuffError> {
        match self {
            Self::Literal(s) => Ok(s.clone()),
            Self::EnvironmentVariable { env: env_var_name } => {
                std::env::var(env_var_name).map_err(|_| {
                    crate::error::WuffError::from(format!(
                        "{}: environment variable not found",
                        env_var_name
                    ))
                })
            }
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct BlobStorageSpec {
    pub storage_account: String,
    pub container: String,
    pub sas: Sas,
    pub prefix: String,
}
