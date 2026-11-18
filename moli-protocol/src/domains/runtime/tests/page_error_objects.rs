//! pilot: an uncaught error created by the page's own code reaches
//! `Runtime.exceptionThrown` as the error object itself, a remote object with
//! an objectId, as in Chrome. V8 15's Inspector includes the exception object
//! only when the script that created the error is shared cross-origin, and
//! Moli compiled page scripts without that flag. Build 11 adds thrown values
//! that are not Errors, module script errors, the text "Uncaught" and no url
//! for code without one.
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

/// A page whose inline script installs two timers that throw a value that is
/// not an Error: `throwStringFromPageTimer()` throws a string and
/// `throwObjectFromPageTimer()` a plain object it keeps in
/// `globalThis.lastThrown`.
const PAGE_WITH_TIMERS_THROWING_VALUES: &str = "<!doctype html><body><script>globalThis.throwStringFromPageTimer = () => setTimeout(() => { throw 'a string from a page timer'; }, 0); globalThis.throwObjectFromPageTimer = () => setTimeout(() => { globalThis.lastThrown = { code: 7 }; throw globalThis.lastThrown; }, 0);</script></body>";

/// Runs `expression`, which schedules page work and evaluates to "scheduled".
async fn schedule_with_runtime_evaluate(ctx: &mut TestContext, id: u64, expression: &str) {
    ctx.process_async(json!({
        "id": id,
        "method": "Runtime.evaluate",
        "params": {
            "expression": expression,
            "returnByValue": true
        }
    }))
    .await;
    let scheduled = take_response_by_id(ctx, id);
    assert_eq!(
        scheduled["result"]["result"]["value"],
        json!("scheduled"),
        "{scheduled}"
    );
}

/// Calls `function_declaration` on the remote object `object_id` and returns
/// its result by value.
async fn call_function_on(
    ctx: &mut TestContext,
    id: u64,
    object_id: &str,
    function_declaration: &str,
) -> serde_json::Value {
    ctx.process_async(json!({
        "id": id,
        "method": "Runtime.callFunctionOn",
        "params": {
            "objectId": object_id,
            "functionDeclaration": function_declaration,
            "returnByValue": true
        }
    }))
    .await;
    let response = wait_for_response_by_id_async(ctx, None, id).await;
    response["result"]["result"]["value"].clone()
}

/// Runs `expression` and returns its value.
async fn evaluate_by_value(ctx: &mut TestContext, id: u64, expression: &str) -> serde_json::Value {
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
    response["result"]["result"]["value"].clone()
}

// 2026-11-18: report_uncaught_exception_to_devtools passed V8's Inspector script id 0 and no stack for a thrown value that is not an Error, so V8 left the exception object out of Runtime.exceptionThrown (it needs a shared cross-origin script) and Moli rebuilt a stand-in Error from the text.
#[tokio::test(flavor = "multi_thread")]
async fn a_string_thrown_by_page_code_reaches_exception_thrown_as_the_string_itself() {
    let mut ctx = TestContext::new();
    with_loaded_document_async(&mut ctx, PAGE_WITH_TIMERS_THROWING_VALUES).await;
    let _ = enable_runtime_and_take_execution_context_id_async(&mut ctx, 1_118_200).await;
    ctx.sent.clear();

    schedule_with_runtime_evaluate(
        &mut ctx,
        1_118_201,
        "throwStringFromPageTimer(); 'scheduled'",
    )
    .await;
    wait_until_message(
        &mut ctx,
        None,
        "Runtime.exceptionThrown for the page timer's string",
        |event| {
            event["method"] == json!("Runtime.exceptionThrown")
                && event["params"]["exceptionDetails"]
                    .to_string()
                    .contains("a string from a page timer")
        },
    )
    .await;
    let details = exception_details_with(&ctx.sent, "a string from a page timer")
        .expect("the page timer's string was reported");
    assert_eq!(details["exception"]["type"], json!("string"), "{details}");
    assert_eq!(
        details["exception"]["value"],
        json!("a string from a page timer"),
        "{details}"
    );
    assert!(
        details["exception"].get("subtype").is_none(),
        "the string itself, not a stand-in Error: {details}"
    );
    assert_eq!(
        details["text"],
        json!("Uncaught"),
        "text is \"Uncaught\" beside the exception object, as in Chrome: {details}"
    );
}

// 2026-11-18: report_uncaught_exception_to_devtools passed V8's Inspector script id 0 and no stack for a thrown value that is not an Error, so a thrown plain object reached DevTools as a stand-in Error without an objectId.
#[tokio::test(flavor = "multi_thread")]
async fn a_plain_object_thrown_by_page_code_reaches_exception_thrown_as_that_object() {
    let mut ctx = TestContext::new();
    with_loaded_document_async(&mut ctx, PAGE_WITH_TIMERS_THROWING_VALUES).await;
    let _ = enable_runtime_and_take_execution_context_id_async(&mut ctx, 1_118_210).await;
    ctx.sent.clear();

    schedule_with_runtime_evaluate(
        &mut ctx,
        1_118_211,
        "throwObjectFromPageTimer(); 'scheduled'",
    )
    .await;
    let is_the_object_report = |event: &serde_json::Value| {
        event["method"] == json!("Runtime.exceptionThrown")
            && event["params"]["exceptionDetails"]["exception"]["className"] == json!("Object")
    };
    wait_until_message(
        &mut ctx,
        None,
        "Runtime.exceptionThrown for the page timer's plain object",
        is_the_object_report,
    )
    .await;
    let details = ctx
        .sent
        .iter()
        .find(|event| is_the_object_report(event))
        .map(|event| event["params"]["exceptionDetails"].clone())
        .expect("the page timer's plain object was reported");
    assert_eq!(details["exception"]["type"], json!("object"), "{details}");
    assert_eq!(details["text"], json!("Uncaught"), "{details}");
    let object_id = details["exception"]["objectId"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| panic!("the exception is a handle to the thrown object: {details}"));
    assert_eq!(
        call_function_on(
            &mut ctx,
            1_118_212,
            &object_id,
            "function () { return this === globalThis.lastThrown; }",
        )
        .await,
        json!(true),
        "the handle is the very object the page threw"
    );
}

/// A page whose inline script installs `insertThrowingModule()`: it inserts a
/// module script that throws an Error as it is evaluated and keeps it in
/// `globalThis.moduleError`. Window error events are recorded in
/// `globalThis.errorEvents` as "message | is the module's error".
const PAGE_WITH_A_MODULE_INSERTER: &str = "<!doctype html><body><script>globalThis.errorEvents = []; addEventListener('error', event => errorEvents.push(event.message + ' | ' + (event.error === globalThis.moduleError))); globalThis.insertThrowingModule = () => { const script = document.createElement('script'); script.type = 'module'; script.textContent = \"globalThis.moduleError = new Error('module at load'); throw globalThis.moduleError;\"; document.body.append(script); };</script></body>";

// 2026-11-18: an uncaught exception of a module script's evaluation became a ModuleLoadError carrying only its text (module_graph.rs), wrapped as "NativeEsmEvaluateFailed: native module graph evaluation rejected: ..." and reported as a new Error built from that text, so the error event, DevTools and Playwright's pageerror never saw the module's own error.
#[tokio::test(flavor = "multi_thread")]
async fn an_error_thrown_by_a_module_script_is_reported_as_the_modules_own_error() {
    let mut ctx = TestContext::new();
    with_loaded_document_async(&mut ctx, PAGE_WITH_A_MODULE_INSERTER).await;
    let _ = enable_runtime_and_take_execution_context_id_async(&mut ctx, 1_118_220).await;
    ctx.sent.clear();

    schedule_with_runtime_evaluate(&mut ctx, 1_118_221, "insertThrowingModule(); 'scheduled'")
        .await;
    wait_until_message(
        &mut ctx,
        None,
        "Runtime.exceptionThrown for the module script's error",
        |event| {
            event["method"] == json!("Runtime.exceptionThrown")
                && event["params"]["exceptionDetails"]
                    .to_string()
                    .contains("module at load")
        },
    )
    .await;
    let details = exception_details_with(&ctx.sent, "module at load")
        .expect("the module script's error was reported");
    assert!(
        details["exception"]["description"]
            .as_str()
            .is_some_and(|description| description.starts_with("Error: module at load")),
        "the module's own error, not Moli's wrapper: {details}"
    );
    let object_id = details["exception"]["objectId"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| panic!("the exception is a handle to the module's error: {details}"));
    assert_eq!(
        call_function_on(
            &mut ctx,
            1_118_222,
            &object_id,
            "function () { return this === globalThis.moduleError; }",
        )
        .await,
        json!(true),
        "the handle is the very error the module threw"
    );
    assert_eq!(
        evaluate_by_value(&mut ctx, 1_118_223, "globalThis.errorEvents").await,
        json!(["Uncaught Error: module at load | true"]),
        "one window error event, carrying the module's own error"
    );
}

/// A page whose inline script installs `throwFromEvalCode()`: a timer runs
/// eval code, which has no URL of its own, that throws an Error. Window error
/// events record their filename in `globalThis.errorFilenames`.
const PAGE_WITH_AN_EVAL_CODE_TIMER: &str = "<!doctype html><body><script>globalThis.errorFilenames = []; addEventListener('error', event => errorFilenames.push(event.filename)); globalThis.throwFromEvalCode = () => setTimeout(() => eval(\"throw new Error('from eval code')\"), 0);</script></body>";

// 2026-11-18: the exception report turned the resource name of eval code (undefined) into the text "undefined", so the error event's filename and Runtime.exceptionThrown's url read "undefined" where Chrome has an empty filename and no url.
#[tokio::test(flavor = "multi_thread")]
async fn an_error_from_eval_code_reaches_exception_thrown_without_a_url() {
    let mut ctx = TestContext::new();
    with_loaded_document_async(&mut ctx, PAGE_WITH_AN_EVAL_CODE_TIMER).await;
    let _ = enable_runtime_and_take_execution_context_id_async(&mut ctx, 1_118_230).await;
    ctx.sent.clear();

    schedule_with_runtime_evaluate(&mut ctx, 1_118_231, "throwFromEvalCode(); 'scheduled'").await;
    wait_until_message(
        &mut ctx,
        None,
        "Runtime.exceptionThrown for the eval code's error",
        |event| {
            event["method"] == json!("Runtime.exceptionThrown")
                && event["params"]["exceptionDetails"]
                    .to_string()
                    .contains("from eval code")
        },
    )
    .await;
    let details = exception_details_with(&ctx.sent, "from eval code")
        .expect("the eval code's error was reported");
    assert!(
        details.get("url").is_none(),
        "no url for code without one, as in Chrome: {details}"
    );
    assert_eq!(
        evaluate_by_value(&mut ctx, 1_118_232, "globalThis.errorFilenames").await,
        json!([""]),
        "the error event's filename is empty"
    );
}
