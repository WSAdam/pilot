//! pilot: an uncaught error created by the page's own code reaches
//! `Runtime.exceptionThrown` as the error object itself, a remote object with
//! an objectId, as in Chrome. V8 15's Inspector includes the exception object
//! only when the script that created the error is shared cross-origin, and
//! Moli compiled page scripts without that flag.
use super::*;

/// A page whose own inline script installs `throwFromPageTimer(message)`: a
/// timer callback compiled from that script creates an Error, keeps it in
/// `globalThis.lastThrown` and throws it.
const PAGE_WITH_A_THROWING_TIMER: &str = "<!doctype html><body><script>globalThis.throwFromPageTimer = message => setTimeout(() => { globalThis.lastThrown = new Error(message); throw globalThis.lastThrown; }, 0);</script></body>";

fn exception_details_with(sent: &[serde_json::Value], message: &str) -> Option<serde_json::Value> {
    sent.iter()
        .find(|event| {
            event["method"] == json!("Runtime.exceptionThrown")
                && event["params"]["exceptionDetails"]
                    .to_string()
                    .contains(message)
        })
        .map(|event| event["params"]["exceptionDetails"].clone())
}

// 2026-11-18: page scripts were compiled without V8's shared cross-origin flag (create_script_origin_with_base_url_and_nonce), and V8 15's Inspector sends Runtime.exceptionThrown without the exception object when the script that created the error is not shared cross-origin, so errors from page code reached DevTools as text alone.
#[tokio::test(flavor = "multi_thread")]
async fn an_error_from_a_page_script_timer_reaches_exception_thrown_as_the_error_object() {
    let mut ctx = TestContext::new();
    with_loaded_document_async(&mut ctx, PAGE_WITH_A_THROWING_TIMER).await;
    let execution_context_id =
        enable_runtime_and_take_execution_context_id_async(&mut ctx, 1_118_100).await;
    ctx.sent.clear();

    ctx.process_async(json!({
        "id": 1_118_101,
        "method": "Runtime.evaluate",
        "params": {
            "expression": "throwFromPageTimer('from a page timer'); 'scheduled'",
            "returnByValue": true
        }
    }))
    .await;
    let scheduled = take_response_by_id(&mut ctx, 1_118_101);
    assert_eq!(
        scheduled["result"]["result"]["value"],
        json!("scheduled"),
        "{scheduled}"
    );
    wait_until_message(
        &mut ctx,
        None,
        "Runtime.exceptionThrown for the page timer's error",
        |event| {
            event["method"] == json!("Runtime.exceptionThrown")
                && event["params"]["exceptionDetails"]
                    .to_string()
                    .contains("from a page timer")
        },
    )
    .await;
    let details = exception_details_with(&ctx.sent, "from a page timer")
        .expect("the page timer's error was reported");
    assert_eq!(
        details["executionContextId"],
        json!(execution_context_id),
        "{details}"
    );
    assert_eq!(details["exception"]["type"], json!("object"), "{details}");
    assert_eq!(details["exception"]["subtype"], json!("error"), "{details}");
    assert_eq!(
        details["exception"]["className"],
        json!("Error"),
        "{details}"
    );
    let object_id = details["exception"]["objectId"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            panic!("the exception is a handle to the thrown error, not only its text: {details}")
        });

    ctx.process_async(json!({
        "id": 1_118_102,
        "method": "Runtime.callFunctionOn",
        "params": {
            "objectId": object_id,
            "functionDeclaration": "function () { return this === globalThis.lastThrown; }",
            "returnByValue": true
        }
    }))
    .await;
    let same = wait_for_response_by_id_async(&mut ctx, None, 1_118_102).await;
    assert_eq!(
        same["result"]["result"]["value"],
        json!(true),
        "the handle is the very error the page threw: {same}"
    );
}
