//! pilot: a classic `<script>` inserted into the main Document runs during the
//! insertion. When it throws, its exception is reported as uncaught, exactly
//! once (the window `error` event, then `Runtime.exceptionThrown`, which is
//! Playwright's `pageerror`), and the insertion returns normally, as in Chrome,
//! whatever code inserted it.
use super::*;

/// Installs `insertThrowingScript(message)`: it appends an inline script that
/// throws `new Error(message)` and says whether the append returned or threw.
/// Window `error` events are recorded in `uncaughtErrorEvents`.
const INSTALL_INSERT_THROWING_SCRIPT: &str = r#"
    globalThis.uncaughtErrorEvents = [];
    addEventListener('error', event => uncaughtErrorEvents.push(event.error && event.error.message));
    globalThis.insertThrowingScript = message => {
        const script = document.createElement('script');
        script.textContent = 'throw new Error(' + JSON.stringify(message) + ')';
        try {
            document.body.appendChild(script);
            return 'returned';
        } catch (error) {
            return 'escaped: ' + error.message;
        }
    };
    'installed'
"#;

async fn evaluate_value(ctx: &mut TestContext, id: u64, expression: &str) -> serde_json::Value {
    ctx.process_async(json!({
        "id": id,
        "method": "Runtime.evaluate",
        "params": {
            "expression": expression,
            "returnByValue": true
        }
    }))
    .await;
    let response = take_response_by_id(ctx, id);
    assert!(
        response["result"]["exceptionDetails"].is_null(),
        "Runtime.evaluate {id} threw: {response}"
    );
    response["result"]["result"]["value"].clone()
}

/// Loads a page with Runtime enabled and the insert helper installed, then
/// clears what was sent so far. Returns the main world's execution context.
async fn load_page_with_insert_helper(ctx: &mut TestContext, first_id: u64) -> i64 {
    with_loaded_document_async(ctx, "<!doctype html><body></body>").await;
    let execution_context_id =
        enable_runtime_and_take_execution_context_id_async(ctx, first_id).await;
    assert_eq!(
        evaluate_value(ctx, first_id + 1, INSTALL_INSERT_THROWING_SCRIPT).await,
        json!("installed")
    );
    ctx.sent.clear();
    execution_context_id
}

fn exceptions_thrown_with<'a>(
    sent: &'a [serde_json::Value],
    message: &str,
) -> Vec<&'a serde_json::Value> {
    sent.iter()
        .filter(|event| {
            event["method"] == json!("Runtime.exceptionThrown")
                && event["params"]["exceptionDetails"]
                    .to_string()
                    .contains(message)
        })
        .collect()
}

/// Waits for the report of `message`, gives a duplicate every chance to
/// arrive (one more command round trip and a few scheduler turns), then checks
/// it is the only one and that it carries the error object, in the page's
/// main world.
async fn expect_one_exception_thrown(
    ctx: &mut TestContext,
    flush_id: u64,
    execution_context_id: i64,
    message: &str,
) {
    wait_until_message(
        ctx,
        None,
        &format!("Runtime.exceptionThrown for {message}"),
        |event| {
            event["method"] == json!("Runtime.exceptionThrown")
                && event["params"]["exceptionDetails"]
                    .to_string()
                    .contains(message)
        },
    )
    .await;
    assert_eq!(
        evaluate_value(ctx, flush_id, "'flushed'").await,
        json!("flushed")
    );
    for _ in 0..8 {
        ctx.complete_one_ready_scheduler_input_for_test().await;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let reports = exceptions_thrown_with(&ctx.sent, message);
    assert_eq!(
        reports.len(),
        1,
        "exactly one Runtime.exceptionThrown for {message}: {reports:?}"
    );
    let details = reports[0]["params"]["exceptionDetails"].clone();
    assert_eq!(
        details["executionContextId"],
        json!(execution_context_id),
        "{details}"
    );
    assert_eq!(details["exception"]["type"], json!("object"), "{details}");
    assert_eq!(details["exception"]["subtype"], json!("error"), "{details}");
    let expected_description = format!("Error: {message}");
    assert!(
        details["exception"]["description"]
            .as_str()
            .is_some_and(|description| description.starts_with(&expected_description)),
        "the exception object describes the error: {details}"
    );
}

// 2026-11-17: the main Document ran an inserted inline script with no TryCatch and no "report the exception" step (execute_committed_inline_classic_script), so its exception was lost inside the insertion: no error event, no Runtime.exceptionThrown.
#[tokio::test(flavor = "multi_thread")]
async fn a_throwing_script_inserted_by_runtime_evaluate_is_reported_once_and_the_insert_returns() {
    let mut ctx = TestContext::new();
    let execution_context_id = load_page_with_insert_helper(&mut ctx, 1_117_100).await;

    assert_eq!(
        evaluate_value(
            &mut ctx,
            1_117_102,
            "insertThrowingScript('from an inserted script')"
        )
        .await,
        json!("returned"),
        "the insert returns normally and the evaluate does not throw"
    );
    expect_one_exception_thrown(
        &mut ctx,
        1_117_103,
        execution_context_id,
        "from an inserted script",
    )
    .await;
    assert_eq!(
        evaluate_value(&mut ctx, 1_117_104, "JSON.stringify(uncaughtErrorEvents)").await,
        json!(r#"["from an inserted script"]"#),
        "the window error event fires once with the error"
    );
}

// 2026-11-17: the main Document ran an inserted inline script with no TryCatch and no "report the exception" step (execute_committed_inline_classic_script), so its exception was lost inside the insertion: no error event, no Runtime.exceptionThrown.
#[tokio::test(flavor = "multi_thread")]
async fn a_throwing_script_inserted_by_a_page_timer_is_reported_once_and_the_insert_returns() {
    let mut ctx = TestContext::new();
    let execution_context_id = load_page_with_insert_helper(&mut ctx, 1_117_200).await;

    assert_eq!(
        evaluate_value(
            &mut ctx,
            1_117_202,
            "setTimeout(() => { globalThis.timerInsert = insertThrowingScript('from a timer'); }, 0); 'scheduled'"
        )
        .await,
        json!("scheduled")
    );
    expect_one_exception_thrown(&mut ctx, 1_117_203, execution_context_id, "from a timer").await;
    assert_eq!(
        evaluate_value(&mut ctx, 1_117_204, "globalThis.timerInsert").await,
        json!("returned"),
        "the insert returns normally inside the timer"
    );
    assert_eq!(
        evaluate_value(&mut ctx, 1_117_205, "JSON.stringify(uncaughtErrorEvents)").await,
        json!(r#"["from a timer"]"#),
        "the window error event fires once with the error"
    );
}

// 2026-11-17: the main Document ran an inserted inline script with no TryCatch and no "report the exception" step (execute_committed_inline_classic_script), so its exception was lost inside the insertion: no error event, no Runtime.exceptionThrown.
#[tokio::test(flavor = "multi_thread")]
async fn a_throwing_script_inserted_by_an_event_handler_is_reported_once_and_the_insert_returns() {
    let mut ctx = TestContext::new();
    let execution_context_id = load_page_with_insert_helper(&mut ctx, 1_117_300).await;

    assert_eq!(
        evaluate_value(
            &mut ctx,
            1_117_302,
            r#"
            document.body.addEventListener('insert', () => {
                globalThis.handlerInsert = insertThrowingScript('from an event handler');
            });
            setTimeout(() => document.body.dispatchEvent(new Event('insert')), 0);
            'scheduled'
            "#
        )
        .await,
        json!("scheduled")
    );
    expect_one_exception_thrown(
        &mut ctx,
        1_117_303,
        execution_context_id,
        "from an event handler",
    )
    .await;
    assert_eq!(
        evaluate_value(&mut ctx, 1_117_304, "globalThis.handlerInsert").await,
        json!("returned"),
        "the insert returns normally inside the handler"
    );
    assert_eq!(
        evaluate_value(&mut ctx, 1_117_305, "JSON.stringify(uncaughtErrorEvents)").await,
        json!(r#"["from an event handler"]"#),
        "the window error event fires once with the error"
    );
}
