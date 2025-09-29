pub mod sftp;
pub mod webdav;

#[derive(serde::Deserialize)]
#[serde(tag = "type")]
pub enum ConfigListener {
    #[serde(rename = "sftp")]
    Sftp(sftp::ConfigListenerSftp),
    #[serde(rename = "webdav")]
    Webdav(webdav::ConfigListenerWebdav),
}

pub enum ConfigListenerEnriched {
    Sftp(sftp::ConfigListenerSftpEnriched),
    Webdav(webdav::ConfigListenerWebdavEnriched),
}

impl TryFrom<ConfigListener> for ConfigListenerEnriched {
    type Error = scan2blob::error::WuffError;

    fn try_from(
        config: ConfigListener,
    ) -> Result<ConfigListenerEnriched, scan2blob::error::WuffError> {
        Ok(match config {
            ConfigListener::Sftp(config) => Self::Sftp(config.try_into()?),
            ConfigListener::Webdav(config) => Self::Webdav(config.try_into()?),
        })
    }
}
