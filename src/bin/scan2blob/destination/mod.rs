const MAX_NUM_CHUNKS: u16 = 50000;

#[derive(serde::Deserialize)]
pub struct ConfigDestination {
    #[serde(flatten)]
    pub blob_storage_spec: scan2blob::util::BlobStorageSpec,
    #[serde(default = "default_initial_chunk_size")]
    pub initial_chunk_size: usize,
    #[serde(default = "default_max_chunk_size")]
    pub max_chunk_size: usize,
}

pub type ConfigDestinations =
    std::collections::HashMap<String, ConfigDestination>;

fn default_initial_chunk_size() -> usize {
    16384
}

// Each concurrent file upload will take 3 times this amount of memory (because
// of double buffering, and also the fact that the Azure SDK needs an owned
// buffer to upload a block)
fn default_max_chunk_size() -> usize {
    1048576
}

pub struct Destination {
    ctx: std::sync::Arc<crate::ctx::Ctx>,
    name: String,
    container_client: azure_storage_blobs::prelude::ContainerClient,
    prefix: String,
    initial_chunk_size: usize,
    max_chunk_size: usize,
}

impl Destination {
    pub fn new(
        ctx: &std::sync::Arc<crate::ctx::Ctx>,
        name: &str,
        cfg: &ConfigDestination,
    ) -> Result<Self, scan2blob::error::WuffError> {
        let container_client: azure_storage_blobs::prelude::ContainerClient =
            azure_storage_blobs::prelude::ClientBuilder::new(
                cfg.blob_storage_spec.storage_account.clone(),
                azure_storage::StorageCredentials::sas_token(
                    cfg.blob_storage_spec.sas.get()?,
                )?,
            )
            .container_client(cfg.blob_storage_spec.container.clone());

        Ok(Self {
            ctx: std::sync::Arc::clone(ctx),
            name: name.to_string(),
            container_client,
            prefix: cfg.blob_storage_spec.prefix.clone(),
            initial_chunk_size: cfg.initial_chunk_size,
            max_chunk_size: cfg.max_chunk_size,
        })
    }

    pub fn write_file(
        self: &std::sync::Arc<Self>,
        name_hint: Option<String>,
        suffix: String,
        content_type: String,
    ) -> scan2blob::chunker::Writer {
        let now: std::time::SystemTime = std::time::SystemTime::now();

        let (writer, reader) = scan2blob::chunker::new(
            self.initial_chunk_size,
            self.max_chunk_size,
            MAX_NUM_CHUNKS,
        );

        let async_spawner = self.ctx.base_ctx.get_async_spawner();
        async_spawner.spawn(std::sync::Arc::clone(self).do_upload(
            reader,
            now,
            name_hint,
            suffix,
            content_type,
        ));

        writer
    }

    async fn do_upload(
        self: std::sync::Arc<Self>,
        mut reader: scan2blob::chunker::Reader,
        timestamp: std::time::SystemTime,
        name_hint: Option<String>,
        suffix: String,
        content_type: String,
    ) {
        let name_hint1: &str = name_hint.as_ref().map_or("", |_| "-");
        let name_hint2: String = name_hint.unwrap_or_default();
        let blob_name: String = format!(
            "{}{}{}{}{}",
            self.prefix,
            scan2blob::util::system_time_to_utc_rfc3339(timestamp),
            name_hint1,
            name_hint2,
            suffix
        );
        let blob_client: azure_storage_blobs::prelude::BlobClient =
            self.container_client.blob_client(&blob_name);
        let mut block_num: u16 = 0;
        let mut block_ids: Vec<azure_storage_blobs::prelude::BlobBlockType> =
            Vec::new();

        self.ctx
            .log_debug(format!("{}: uploading {}", self.name, blob_name));
        let hash: [u8; 16] = loop {
            let chunk: Vec<u8> = match reader.get_next_chunk().await {
                Err(err) => {
                    self.ctx.log(format!(
                        "{}: aborting upload of {} due to propagated error: {}",
                        self.name, blob_name, err
                    ));
                    return;
                }
                Ok(scan2blob::chunker::ChunkOrEof::Chunk(chunk)) => chunk,
                Ok(scan2blob::chunker::ChunkOrEof::Eof(hash)) => {
                    break hash;
                }
            };

            // for some reason you can make a BlockId from a vector but
            // not from an array directly. which is no problem, it's
            // just odd. We can make a vector.
            let block_num_as_bytes: Vec<u8> = block_num.to_be_bytes().into();
            let block_id: azure_storage_blobs::prelude::BlockId =
                azure_storage_blobs::prelude::BlockId::new(block_num_as_bytes);
            block_num += 1;
            block_ids.push(
                azure_storage_blobs::blob::BlobBlockType::new_uncommitted(
                    block_id.clone(),
                ),
            );

            if let Err(e) =
                blob_client.put_block(block_id, chunk).into_future().await
            {
                self.ctx.log(format!(
                    "{}: upload of {} failed: {}",
                    self.name, blob_name, e
                ));
                reader.observe_error(scan2blob::error::WuffError::from(e));
                return;
            }
        };

        if let Err(e) = blob_client
            .put_block_list(azure_storage_blobs::blob::BlockList {
                blocks: block_ids,
            })
            .content_md5(hash)
            .content_type(&content_type)
            .into_future()
            .await
        {
            self.ctx.log(format!(
                "{}: upload of {} failed: {}",
                self.name, blob_name, e
            ));
            reader.observe_error(scan2blob::error::WuffError::from(e));
            return;
        }

        if let Err(e) = reader.finalize().await {
            self.ctx.log(format!(
                "{}: aborting upload of {} due to propagated error: {}",
                self.name, blob_name, e
            ));
        }
    }
}

pub struct Destinations {
    destinations:
        std::collections::HashMap<String, std::sync::Arc<Destination>>,
}

impl Destinations {
    pub fn new(
        ctx: &std::sync::Arc<crate::ctx::Ctx>,
    ) -> Result<Self, scan2blob::error::WuffError> {
        let mut destinations: std::collections::HashMap<
            String,
            std::sync::Arc<Destination>,
        > = std::collections::HashMap::new();
        for (destination_name, destination_cfg) in &ctx.config.destinations {
            let destination: Destination =
                Destination::new(ctx, &destination_name, destination_cfg)?;
            assert!(
                destinations
                    .insert(
                        destination_name.clone(),
                        std::sync::Arc::new(destination)
                    )
                    .is_none()
            );
        }
        Ok(Self { destinations })
    }
    pub fn get(&self, name: &str) -> Option<std::sync::Arc<Destination>> {
        self.destinations.get(name).cloned()
    }
}
