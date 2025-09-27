#[derive(Debug, Clone)]
pub struct WuffError {
    pub message: String,
}

impl std::fmt::Display for WuffError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for WuffError {}

impl From<&str> for WuffError {
    fn from(message: &str) -> WuffError {
        WuffError {
            message: String::from(message),
        }
    }
}

impl From<String> for WuffError {
    fn from(message: String) -> WuffError {
        WuffError { message: message }
    }
}

impl From<std::io::Error> for WuffError {
    fn from(err: std::io::Error) -> WuffError {
        WuffError {
            message: format!("{:?}", err),
        }
    }
}

impl From<tokio::task::JoinError> for WuffError {
    fn from(err: tokio::task::JoinError) -> WuffError {
        WuffError {
            message: format!("{:?}", err),
        }
    }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for WuffError {
    fn from(err: tokio::sync::mpsc::error::SendError<T>) -> WuffError {
        WuffError {
            message: format!("{:?}", err),
        }
    }
}

impl From<azure_storage::Error> for WuffError {
    fn from(err: azure_storage::Error) -> WuffError {
        WuffError {
            message: format!("{:?}", err),
        }
    }
}

impl From<serde_json::Error> for WuffError {
    fn from(err: serde_json::Error) -> WuffError {
        WuffError {
            message: format!("{:?}", err),
        }
    }
}

impl From<russh::Error> for WuffError {
    fn from(err: russh::Error) -> WuffError {
        WuffError {
            message: format!("{:?}", err),
        }
    }
}

impl From<rustls::Error> for WuffError {
    fn from(err: rustls::Error) -> WuffError {
        WuffError {
            message: format!("{:?}", err),
        }
    }
}

impl From<serde_urlencoded::de::Error> for WuffError {
    fn from(err: serde_urlencoded::de::Error) -> WuffError {
        WuffError {
            message: format!("{:?}", err),
        }
    }
}

impl From<daemonize::Error> for WuffError {
    fn from(err: daemonize::Error) -> WuffError {
        WuffError {
            message: format!("{:?}", err),
        }
    }
}
