use rquickjs::function::Args;
use rquickjs::{CatchResultExt, Ctx, Persistent, Value};

// Native functions for the timeout implementation (based on wasm-rquickjs)
#[rquickjs::module]
pub mod native_module {
    use futures::future::abortable;
    use rquickjs::{Ctx, Persistent, Value};

    #[rquickjs::function]
    pub fn schedule(
        ctx: Ctx<'_>,
        code_or_fn: Persistent<Value<'static>>,
        delay: Option<u32>,
        periodic: bool,
        args: Persistent<Vec<Value<'static>>>,
    ) -> usize {
        let delay = delay.unwrap_or(0);

        let (task, _abort_handle) = abortable(super::scheduled_task(
            ctx.clone(),
            code_or_fn,
            delay,
            periodic,
            args,
        ));

        ctx.spawn(async move {
            let _ = task.await;
        });
        0
    }
}

// JS functions for the console implementation
pub const TIMEOUT_JS: &str = include_str!("timeout.js");

// JS code wiring the console module into the global context
pub const WIRE_JS: &str = r#"
        import * as __golem_exec_js_timeout from '__golem_exec_js_builtin/timeout';
        globalThis.setTimeout = __golem_exec_js_timeout.setTimeout;
        globalThis.setImmediate = __golem_exec_js_timeout.setImmediate;
        globalThis.setInterval = __golem_exec_js_timeout.setInterval;
    "#;

async fn scheduled_task(
    ctx: Ctx<'_>,
    code_or_fn: Persistent<Value<'static>>,
    delay: u32,
    periodic: bool,
    args: Persistent<Vec<Value<'static>>>,
) {
    if delay == 0 {
        run_scheduled_task(ctx.clone(), code_or_fn.clone(), args.clone())
            .catch(&ctx)
            .expect("Failed to run scheduled task");
    } else {
        let duration = wstd::time::Duration::from_millis(delay as u64);

        loop {
            wstd::task::sleep(duration).await;

            run_scheduled_task(ctx.clone(), code_or_fn.clone(), args.clone())
                .catch(&ctx)
                .expect("Failed to run scheduled task");

            if !periodic {
                break;
            }
        }
    }
}

fn run_scheduled_task(
    ctx: Ctx,
    code_or_fn: Persistent<Value<'static>>,
    args: Persistent<Vec<Value<'static>>>,
) -> rquickjs::Result<()> {
    let restored_code_or_fn = code_or_fn.restore(&ctx)?;
    let restored_args = args.restore(&ctx)?;

    if let Some(func) = restored_code_or_fn.as_function() {
        let mut args = Args::new(ctx, restored_args.len());
        args.push_args(&restored_args)?;
        func.call_arg(args)
    } else if let Some(code) = restored_code_or_fn.as_string() {
        if !restored_args.is_empty() {
            panic!("Passing arguments to scheduled code snippets is not supported");
        }
        ctx.eval(code.to_string()?)
    } else {
        panic!("Unsupported value passed to setTimeout or setInterval: {restored_code_or_fn:?}");
    }
}
