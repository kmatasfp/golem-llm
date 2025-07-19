use std::time::Duration;

use wstd::task;

#[allow(async_fn_in_trait)]
pub trait AsyncRuntime {
    async fn sleep(&self, duration: Duration);
}

pub struct WasiAyncRuntime {}

impl WasiAyncRuntime {
    pub fn new() -> Self {
        Self {}
    }
}

impl AsyncRuntime for WasiAyncRuntime {
    async fn sleep(&self, duration: Duration) {
        task::sleep(duration.into()).await;
    }
}
