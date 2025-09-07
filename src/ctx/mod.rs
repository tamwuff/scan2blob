pub struct Ctx {
    tokio_runtime: tokio::runtime::Runtime,
}

impl Ctx {
    pub fn new() -> Self {
        Self {
            tokio_runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio"),
        }
    }

    pub fn run_async_main<F>(
        &self,
        f: F,
    ) -> Result<(), crate::error::WuffError>
    where
        F: std::future::Future<Output = Result<(), crate::error::WuffError>>,
    {
        self.tokio_runtime.block_on(f)
    }

    pub fn get_async_spawner(&self) -> &tokio::runtime::Handle {
        self.tokio_runtime.handle()
    }
}
