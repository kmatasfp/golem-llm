use std::time::Duration;

#[allow(async_fn_in_trait)]
pub trait AsyncRuntime {
    async fn sleep(&self, duration: Duration);
}
