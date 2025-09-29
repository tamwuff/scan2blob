#[derive(serde::Deserialize)]
pub struct ConfigMimeType {
    pub override_suffix: Option<String>,
    pub content_type: String,
}

pub type ConfigMimeTypes = std::collections::HashMap<String, ConfigMimeType>;

pub fn default_mime_types() -> ConfigMimeTypes {
    let mut mime_types: std::collections::HashMap<String, ConfigMimeType> =
        std::collections::HashMap::new();
    mime_types.insert(
        "pdf".to_string(),
        ConfigMimeType {
            override_suffix: None,
            content_type: "application/pdf".to_string(),
        },
    );
    mime_types.insert(
        "jpg".to_string(),
        ConfigMimeType {
            override_suffix: None,
            content_type: "image/jpeg".to_string(),
        },
    );
    mime_types.insert(
        "jpeg".to_string(),
        ConfigMimeType {
            override_suffix: Some(".jpg".to_string()),
            content_type: "image/jpeg".to_string(),
        },
    );
    mime_types.insert(
        "png".to_string(),
        ConfigMimeType {
            override_suffix: None,
            content_type: "image/png".to_string(),
        },
    );
    mime_types.insert(
        "tiff".to_string(),
        ConfigMimeType {
            override_suffix: None,
            content_type: "image/tiff".to_string(),
        },
    );
    mime_types.insert(
        "tif".to_string(),
        ConfigMimeType {
            override_suffix: Some(".tiff".to_string()),
            content_type: "image/tiff".to_string(),
        },
    );
    mime_types
}

#[derive(Clone)]
pub struct ConfigMimeTypeEnriched {
    pub suffix: String,
    pub content_type: String,
}

pub struct ConfigMimeTypesEnriched(
    std::collections::HashMap<String, ConfigMimeTypeEnriched>,
);

impl TryFrom<ConfigMimeTypes> for ConfigMimeTypesEnriched {
    type Error = scan2blob::error::WuffError;
    fn try_from(
        config: ConfigMimeTypes,
    ) -> Result<Self, scan2blob::error::WuffError> {
        let mut mime_types: std::collections::HashMap<
            String,
            ConfigMimeTypeEnriched,
        > = std::collections::HashMap::new();
        for (extension, mime_type_cfg) in config {
            let extension: String = extension.to_lowercase();
            let suffix: String =
                if let Some(ref suffix) = mime_type_cfg.override_suffix {
                    suffix.clone()
                } else {
                    ".".to_string() + &extension
                };
            let mime_type: ConfigMimeTypeEnriched = ConfigMimeTypeEnriched {
                suffix,
                content_type: mime_type_cfg.content_type.clone(),
            };
            if mime_types.insert(extension, mime_type).is_some() {
                return Err(scan2blob::error::WuffError::from(
                    "duplicate mime types",
                ));
            }
        }
        Ok(Self(mime_types))
    }
}

impl ConfigMimeTypesEnriched {
    pub fn get(&self, filename: &str) -> Option<ConfigMimeTypeEnriched> {
        let Some((_, extension)) = filename.rsplit_once('.') else {
            return None;
        };
        self.0.get(&extension.to_lowercase()).cloned()
    }
}
