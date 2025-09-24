pub fn make_cmdline_parser(argv0: &'static str) -> clap::Command {
    clap::Command::new(argv0).color(clap::ColorChoice::Never)
}

// These types are shared between the main scan2blob server binary, and the
// "mksas" binary. Most of the structs that define the server's config file are
// in the server's own crate, but these ones need to be shared.

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum LiteralOrEnvironmentVariable {
    Literal(String),
    EnvironmentVariable { env: String },
}

impl LiteralOrEnvironmentVariable {
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
#[serde(untagged)]
pub enum LiteralOrFile {
    Literal(String),
    File { file: std::path::PathBuf },
}

impl LiteralOrFile {
    pub fn get(&self) -> Result<String, crate::error::WuffError> {
        match self {
            Self::Literal(s) => Ok(s.clone()),
            Self::File { file: filename } => {
                let data: Vec<u8> = std::fs::read(filename)?;
                String::from_utf8(data).map_err(|_| {
                    crate::error::WuffError::from(format!(
                        "{:?} contains non-UTF8 data",
                        filename
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
    pub sas: LiteralOrEnvironmentVariable,
    pub prefix: String,
}

pub fn system_time_to_utc_rfc3339(t: std::time::SystemTime) -> String {
    let as_duration: std::time::Duration =
        t.duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap();
    let as_time_t: u64 = as_duration.as_secs();
    let milliseconds: u32 = as_duration.subsec_millis();
    let as_jiff: jiff::Timestamp =
        jiff::Timestamp::from_second(as_time_t as i64).unwrap();
    let mut as_str: String = format!("{}", as_jiff);
    assert!(matches!(as_str.pop(), Some('Z')));
    as_str.push_str(&format!(".{:03}Z", milliseconds));
    as_str
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn stringify_systemtime() {
        let t = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs_f64(1758764290.005006);
        let s = system_time_to_utc_rfc3339(t);
        assert_eq!(s, "2025-09-25T01:38:10.005Z");

        let t = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs_f64(1758764290.500600);
        let s = system_time_to_utc_rfc3339(t);
        assert_eq!(s, "2025-09-25T01:38:10.500Z");
    }
}
