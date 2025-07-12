use std::time::Duration;
use wasi::clocks::monotonic_clock::subscribe_duration;
use wasi_async_runtime::Reactor;

#[allow(async_fn_in_trait)]
pub trait AsyncRuntime {
    async fn sleep(&self, duration: Duration);
}

pub struct WasiAyncRuntime {
    reactor: Reactor,
}

impl WasiAyncRuntime {
    pub fn new(reactor: Reactor) -> Self {
        Self { reactor }
    }
}

impl AsyncRuntime for WasiAyncRuntime {
    async fn sleep(&self, duration: Duration) {
        let duration = duration.as_nanos() as u64;
        let pollable = subscribe_duration(duration);
        self.reactor.wait_for(pollable).await;
    }
}
