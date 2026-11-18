use super::*;
#[cfg(test)]
use crate::local_executor::JsOwnerLocalRuntimeAccessPath;
#[cfg(test)]
use crate::local_executor::JsOwnerLocalRuntimeEntryPath;
use tokio::sync::oneshot;

pub(super) async fn run_named_owner_local_task<R, F>(
    local_executor: JsLocalExecutor,
    canceled_message: &'static str,
    future: F,
) -> Result<R>
where
    R: 'static,
    F: std::future::Future<Output = Result<R>> + 'static,
{
    let (reply_tx, reply_rx) = oneshot::channel();
    let timing_enabled = moli_trace::cdp_nav_timing_enabled();
    let started = timing_enabled.then(std::time::Instant::now);
    if timing_enabled {
        tracing::info!(
            target: "moli_cdp_nav_timing",
            task = canceled_message,
            stage = "owner_local_task_spawned",
        );
    }
    // pilot: the task future is boxed before it is spawned, so only a pointer
    // travels through tokio's spawn frames. In a debug build each of those
    // frames holds the spawned future by value, and a page-task future is
    // large enough that a dozen copies overflowed the 2 MiB test thread stack
    // (gate runs 44155204410 and 44196996026).
    let future = Box::pin(future);
    tokio::task::spawn_local(async move {
        let result = local_executor.scope_on_current_thread(future).await;
        let _ = reply_tx.send(result);
    });
    let result = reply_rx.await.map_err(|_| anyhow!(canceled_message))?;
    if let Some(started) = started {
        tracing::info!(
            target: "moli_cdp_nav_timing",
            task = canceled_message,
            elapsed_ms = started.elapsed().as_millis(),
            stage = "owner_local_task_finished",
        );
    }
    result
}

#[cfg(test)]
pub(super) type OwnerLocalRuntimeAccessPath = JsOwnerLocalRuntimeAccessPath;
#[cfg(test)]
pub(super) type OwnerLocalRuntimeEntryPath = JsOwnerLocalRuntimeEntryPath;

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScriptExecutionDomainPath {
    DirectNamedLane,
    DirectScaffoldLane,
    CurrentThreadFallback,
    Inaccessible,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScriptExecutionLanePath {
    DirectNamedLane,
    DirectScaffoldLane,
    Inaccessible,
}

#[cfg(test)]
pub(super) fn owner_local_runtime_access_path(
    local_executor: &JsLocalExecutor,
) -> OwnerLocalRuntimeAccessPath {
    local_executor.current_owner_local_runtime_access_path()
}

#[cfg(test)]
pub(super) fn owner_local_runtime_entry_path(
    local_executor: &JsLocalExecutor,
) -> OwnerLocalRuntimeEntryPath {
    local_executor.current_owner_local_runtime_entry_path()
}

#[cfg(test)]
pub(super) fn script_execution_domain_path(
    local_executor: &JsLocalExecutor,
) -> ScriptExecutionDomainPath {
    match local_executor.current_access_context() {
        crate::local_executor::JsLocalExecutorAccessContext::MatchingNamedLane => {
            ScriptExecutionDomainPath::DirectNamedLane
        }
        crate::local_executor::JsLocalExecutorAccessContext::DifferentNamedLane => {
            ScriptExecutionDomainPath::Inaccessible
        }
        crate::local_executor::JsLocalExecutorAccessContext::ScaffoldLane => {
            ScriptExecutionDomainPath::DirectScaffoldLane
        }
        crate::local_executor::JsLocalExecutorAccessContext::CurrentThreadOutsideLane => {
            ScriptExecutionDomainPath::CurrentThreadFallback
        }
        crate::local_executor::JsLocalExecutorAccessContext::Outside => {
            ScriptExecutionDomainPath::Inaccessible
        }
    }
}

#[cfg(test)]
pub(super) fn script_execution_lane_path(
    local_executor: &JsLocalExecutor,
) -> ScriptExecutionLanePath {
    match script_execution_domain_path(local_executor) {
        ScriptExecutionDomainPath::DirectNamedLane => ScriptExecutionLanePath::DirectNamedLane,
        ScriptExecutionDomainPath::DirectScaffoldLane => {
            ScriptExecutionLanePath::DirectScaffoldLane
        }
        ScriptExecutionDomainPath::CurrentThreadFallback
        | ScriptExecutionDomainPath::Inaccessible => ScriptExecutionLanePath::Inaccessible,
    }
}

#[cfg(test)]
pub(super) fn is_on_script_execution_domain_for(local_executor: &JsLocalExecutor) -> bool {
    !matches!(
        script_execution_domain_path(local_executor),
        ScriptExecutionDomainPath::Inaccessible
    )
}

pub(super) fn is_on_script_execution_lane_for(local_executor: &JsLocalExecutor) -> bool {
    #[cfg(test)]
    {
        crate::local_executor::is_on_script_execution_lane_for(local_executor)
    }

    #[cfg(not(test))]
    {
        is_on_named_owner_execution_lane_for(local_executor)
    }
}

#[cfg(test)]
pub(super) fn is_on_parse_time_scaffold_lane() -> bool {
    crate::local_executor::is_on_scoped_js_local_executor_lane()
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-11-17: run_named_owner_local_task handed its task future to tokio::task::spawn_local by value, and in a debug build each of tokio's dozen spawn frames held a copy, so a large page-task future overflowed the 2 MiB test thread stack (worker test child_message_handler_external_dedicated_worker_binds_child_client_event_owner, gate runs 44155204410 and 44196996026).
    #[test]
    fn a_large_owner_task_future_is_spawned_without_copies_on_the_stack() {
        const PAYLOAD_BYTES: usize = 256 * 1024;
        // The default test thread stack, whatever RUST_MIN_STACK says.
        let thread = std::thread::Builder::new()
            .name("large-owner-task".to_owned())
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("current-thread runtime");
                let local = tokio::task::LocalSet::new();
                runtime.block_on(local.run_until(Box::pin(async {
                    // The payload lives across an await, so it is part of the
                    // task future's own state, like a page task's locals.
                    let task = async {
                        let payload = [7u8; PAYLOAD_BYTES];
                        tokio::task::yield_now().await;
                        Ok::<usize, anyhow::Error>(
                            payload.iter().map(|byte| usize::from(*byte)).sum(),
                        )
                    };
                    run_named_owner_local_task(
                        JsLocalExecutor::new(),
                        "large owner task was canceled",
                        task,
                    )
                    .await
                })))
            })
            .expect("spawn the 2 MiB test thread");
        let sum = thread
            .join()
            .expect("the large owner task must not overflow the stack")
            .expect("the large owner task should finish");
        assert_eq!(sum, 7 * PAYLOAD_BYTES);
    }
}
