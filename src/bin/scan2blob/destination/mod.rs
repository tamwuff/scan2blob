pub struct Destination {
    container_client: azure_storage_blobs::prelude::ContainerClient,
    prefix: String,
}

impl Destination {
    pub fn new(
        cfg: &crate::ctx::ConfigDestination,
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
            container_client,
            prefix: cfg.blob_storage_spec.prefix.clone(),
        })
    }

    pub async fn test(&self) {
        let blob_name: String = format!("{}foo.txt", self.prefix);
        let blob_client: azure_storage_blobs::prelude::BlobClient =
            self.container_client.blob_client(blob_name);
        let block_id: azure_storage_blobs::prelude::BlockId =
            azure_storage_blobs::prelude::BlockId::new(vec![42]);
        blob_client
            .put_block(block_id.clone(), "Hello, world!")
            .into_future()
            .await
            .unwrap();
        blob_client
            .put_block_list(azure_storage_blobs::blob::BlockList {
                blocks: vec![
                    azure_storage_blobs::blob::BlobBlockType::new_uncommitted(
                        block_id.clone(),
                    ),
                ],
            })
            .into_future()
            .await
            .unwrap();
    }
}
