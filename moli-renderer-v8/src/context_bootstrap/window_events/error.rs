use super::*;
use moli_webapi_declare::WebApiObject;

#[derive(WebApiObject)]
#[webapi(plain)]
struct ErrorEventInitDeclaration<'scope> {
    #[webapi(data_property, enumerable)]
    cancelable: bool,
    #[webapi(data_property, enumerable)]
    bubbles: bool,
    #[webapi(data_property, enumerable)]
    message: v8::Local<'scope, v8::Value>,
    #[webapi(data_property, enumerable)]
    filename: v8::Local<'scope, v8::Value>,
    #[webapi(data_property, enumerable)]
    lineno: u32,
    #[webapi(data_property, enumerable)]
    colno: u32,
    #[webapi(data_property, enumerable)]
    error: v8::Local<'scope, v8::Value>,
}

const BODY_ONERROR_RESOLUTION_GUARD_SLOT: &str = "__moliBodyOnerrorResolutionGuard";

pub(super) fn ensure_window_reflecting_body_onerror_handler(scope: &mut v8::PinScope<'_, '_>) {
    let global = scope.get_current_context().global(scope);
    if global_hidden_value(scope, WINDOW_ONERROR_SLOT).is_some_and(|handler| handler.is_function())
    {
        return;
    }
    if get_private_value(scope, global, BODY_ONERROR_RESOLUTION_GUARD_SLOT)
        .is_some_and(|value| value.boolean_value(scope))
    {
        return;
    }
    set_private_value(
        scope,
        global,
        BODY_ONERROR_RESOLUTION_GUARD_SLOT,
        v8::Boolean::new(scope, true).into(),
    );
    let body = global
        .get(scope, v8str(scope, "document").into())
        .and_then(|document| v8::Local::<v8::Object>::try_from(document).ok())
        .and_then(|document| document.get(scope, v8str(scope, "body").into()))
        .and_then(|body| v8::Local::<v8::Object>::try_from(body).ok());
    if let Some(body) = body {
        let _ = body.get(scope, v8str(scope, "onerror").into());
    }
    set_private_value(
        scope,
        global,
        BODY_ONERROR_RESOLUTION_GUARD_SLOT,
        v8::Boolean::new(scope, false).into(),
    );
}

struct ReportedExceptionDetails {
    message: String,
    filename: String,
    lineno: u32,
    colno: u32,
    error: v8::Global<v8::Value>,
    /// pilot: where V8's message for the value says it was thrown, so a value
    /// with no stack of its own reaches DevTools as its own remote object.
    throw_site: crate::exception_reporting::ExceptionThrowSite,
}

fn reported_exception_details<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    host_ptr: *mut JsContextHost,
    reason: v8::Local<'s, v8::Value>,
) -> ReportedExceptionDetails {
    // `reportError()` feeds the exception directly into HTML's exception
    // reporting algorithm. V8's Message preserves the engine's internal
    // exception metadata without observing author-defined `name`, `message`,
    // or location getters on the value.
    let exception_message = v8::Exception::create_message(scope, reason);
    let message = exception_message.get(scope).to_rust_string_lossy(scope);
    // pilot: a script without a URL has the resource name undefined, which
    // is an empty filename rather than "undefined", as in Chrome.
    let filename = exception_message
        .get_script_resource_name(scope)
        .filter(|value| !value.is_null_or_undefined())
        .and_then(|value| value.to_string(scope))
        .map(|value| value.to_rust_string_lossy(scope))
        .unwrap_or_default();
    let lineno = exception_message
        .get_line_number(scope)
        .and_then(|line| u32::try_from(line).ok())
        .unwrap_or(0);
    let colno = exception_message
        .get_start_column()
        .checked_add(1)
        .and_then(|column| u32::try_from(column).ok())
        .unwrap_or(0);
    // pilot: no page code is running when a module script reports its
    // evaluation's exception, so V8's message for it has no script and no
    // line. Blink's CaptureSourceLocation then takes the top frame of the
    // message's stack: for an Error, where it was created (Build 11's second
    // gate run, build11-errors "a module script's Error is reported at the
    // top frame of its stack"). A value with no location of its own (a string
    // a module script threw, the reason its top-level await rejected with)
    // has no stack either, and Blink uses the execution context's URL, with
    // line and column 0; so does Runtime.exceptionThrown. Without this the
    // filename was empty and the event had no url (moli-core tests
    // *_tla_exotic_rejection_*).
    let stack_top_frame = if lineno == 0 {
        exception_message
            .get_stack_trace(scope)
            .filter(|stack_trace| stack_trace.get_frame_count() > 0)
            .and_then(|stack_trace| stack_trace.get_frame(scope, 0))
    } else {
        None
    };
    let (filename, lineno, colno) = if let Some(frame) = stack_top_frame {
        let frame_url = frame
            .get_script_name_or_source_url(scope)
            .map(|name| name.to_rust_string_lossy(scope))
            .unwrap_or_default();
        let frame_lineno = u32::try_from(frame.get_line_number()).unwrap_or(0);
        let frame_colno = u32::try_from(frame.get_column()).unwrap_or(0);
        (frame_url, frame_lineno, frame_colno)
    } else if filename.is_empty() && lineno == 0 {
        (current_realm_document_url(scope, host_ptr), 0, 0)
    } else {
        (filename, lineno, colno)
    };
    let throw_site = crate::exception_reporting::ExceptionThrowSite::from_message(
        scope,
        Some(exception_message),
    );

    ReportedExceptionDetails {
        message,
        filename,
        lineno,
        colno,
        error: v8::Global::new(scope, reason),
        throw_site,
    }
}

/// pilot: the URL of the current realm's Document (an iframe's, a popup's or
/// the page's), which Blink reports for an exception whose message has no
/// location.
fn current_realm_document_url(
    scope: &mut v8::PinScope<'_, '_>,
    host_ptr: *mut JsContextHost,
) -> String {
    let host = unsafe { &*host_ptr };
    if let Some(handle) =
        crate::context_bootstrap::child_browsing_context_handle_for_current_realm_scope(scope)
    {
        return host
            .child_browsing_context_current_url(handle)
            .map(|url| url.to_string())
            .unwrap_or_default();
    }
    if let Some(popup_id) = crate::native_bridge::active_lightweight_popup_id(scope)
        && let Some(url) = host.lightweight_popup_document_url(popup_id)
    {
        return url.to_string();
    }
    host.dom_host()
        .document_url()
        .cloned()
        .unwrap_or_else(|| host.document_url().clone())
        .to_string()
}

/// pilot: what HTML's "deliver resize loop error notification" fires at the
/// window when a ResizeObserver loop leaves notifications for the next
/// rendering update.
const RESIZE_OBSERVER_LOOP_ERROR_MESSAGE: &str =
    "ResizeObserver loop completed with undelivered notifications.";

/// pilot: HTML's "deliver resize loop error notification": an ErrorEvent with
/// that message at the current realm's window. Chrome gives it the
/// document's URL, line and column 0 and a null error, so it reaches neither
/// DevTools nor Playwright's `pageerror`.
pub(crate) fn dispatch_resize_observer_loop_error<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    host_ptr: *mut JsContextHost,
) -> std::result::Result<(), String> {
    let filename = current_realm_document_url(scope, host_ptr);
    dispatch_window_error_event_with_details(
        scope,
        host_ptr,
        RESIZE_OBSERVER_LOOP_ERROR_MESSAGE,
        &filename,
        0,
        0,
        None,
    )
}

pub(crate) fn dispatch_window_report_error_message<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    host_ptr: *mut JsContextHost,
    message: &str,
    filename: Option<&str>,
) -> std::result::Result<(), String> {
    let message_value = v8_string(scope, message)
        .ok_or_else(|| "failed to allocate reportError message".to_owned())?;
    let reason = v8::Exception::error(scope, message_value);
    if let Some(filename) = filename
        && let Some(filename_value) = v8_string(scope, filename)
        && let Ok(error_object) = v8::Local::<v8::Object>::try_from(reason)
    {
        let _ = error_object.set(
            scope,
            v8str(scope, "fileName").into(),
            filename_value.into(),
        );
    }
    dispatch_window_error_event_with_details(
        scope,
        host_ptr,
        message,
        filename.unwrap_or(""),
        0,
        0,
        Some(reason),
    )
}

/// pilot: HTML's "report an exception" for `exception` in the current realm,
/// after the `v8::TryCatch` that caught it is gone: the event's message and
/// location are V8's for the value, as for `reportError()`. A module script
/// whose evaluation threw reports its own error this way, as Blink's
/// `V8ScriptRunner::ReportException` does.
pub(crate) fn report_exception_to_window<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    host_ptr: *mut JsContextHost,
    exception: v8::Local<'s, v8::Value>,
) -> std::result::Result<(), String> {
    let details = reported_exception_details(scope, host_ptr, exception);
    dispatch_window_error_event_from_throw_site(
        scope,
        host_ptr,
        &details.message,
        &details.filename,
        details.lineno,
        details.colno,
        Some(exception),
        &details.throw_site,
    )
}

/// pilot: HTML's "report an exception" for an exception V8 caught in page
/// code, from what `report` kept of V8's message: where it was thrown, and
/// whether its script's errors are muted.
pub(crate) fn dispatch_window_error_event_for_exception_report<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    host_ptr: *mut JsContextHost,
    report: &crate::exception_reporting::V8ExceptionReport,
) -> std::result::Result<(), String> {
    let error_value = report
        .exception
        .as_ref()
        .map(|exception| v8::Local::new(scope, exception));
    dispatch_window_error_event_from_throw_site(
        scope,
        host_ptr,
        &report.summary,
        report.source.as_deref().unwrap_or(""),
        report.line.unwrap_or(0) as u32,
        report.column.unwrap_or(0) as u32,
        error_value,
        &report.throw_site,
    )
}

pub(crate) fn dispatch_window_error_event_with_details<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    host_ptr: *mut JsContextHost,
    message: &str,
    filename: &str,
    lineno: u32,
    colno: u32,
    error_value: Option<v8::Local<'s, v8::Value>>,
) -> std::result::Result<(), String> {
    dispatch_window_error_event_from_throw_site(
        scope,
        host_ptr,
        message,
        filename,
        lineno,
        colno,
        error_value,
        &crate::exception_reporting::ExceptionThrowSite::default(),
    )
}

/// pilot: what the window `error` event of a muted script error reads, as in
/// HTML ("Script error.", an empty filename, 0, 0 and a null error).
const MUTED_SCRIPT_ERROR_MESSAGE: &str = "Script error.";

#[allow(clippy::too_many_arguments)]
fn dispatch_window_error_event_from_throw_site<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    host_ptr: *mut JsContextHost,
    message: &str,
    filename: &str,
    lineno: u32,
    colno: u32,
    error_value: Option<v8::Local<'s, v8::Value>>,
    throw_site: &crate::exception_reporting::ExceptionThrowSite,
) -> std::result::Result<(), String> {
    ensure_window_reflecting_body_onerror_handler(scope);
    let (message_text, filename_text) = (message, filename);
    // pilot: an exception from a script fetched cross-origin without CORS is
    // announced to the page sanitized (HTML "muted errors"); DevTools still
    // gets it in full below, as Blink's ExceptionThrown does.
    let (event_message, event_filename, event_lineno, event_colno, event_error) =
        if throw_site.muted_errors {
            (MUTED_SCRIPT_ERROR_MESSAGE, "", 0, 0, None)
        } else {
            (message, filename, lineno, colno, error_value)
        };
    let event_error = event_error.unwrap_or_else(|| v8::null(scope).into());

    let message = v8_string(scope, event_message)
        .map(|s| s.into())
        .unwrap_or_else(|| v8::null(scope).into());
    let filename = v8_string(scope, event_filename)
        .map(|s| s.into())
        .unwrap_or_else(|| v8::null(scope).into());
    let init = ErrorEventInitDeclaration::new(
        true,
        false,
        message,
        filename,
        event_lineno,
        event_colno,
        event_error,
    )
    .bind(scope)
    .expect("ErrorEvent init declaration should bind");

    let event_type = v8str(scope, "error");
    let event = super::super::exposed_interfaces::ensure_intrinsic_interface_constructor(
        scope,
        "ErrorEvent",
    )
    .ok()
    .and_then(|ctor| ctor.new_instance(scope, &[event_type.into(), init.into()]));
    let Some(event) = event else {
        return Ok(());
    };
    mark_event_trusted(scope, event);

    let runtime = unsafe { &mut *host_ptr };
    if let Some(child_handle) =
        crate::context_bootstrap::child_browsing_context_handle_for_current_realm_scope(scope)
    {
        runtime.dispatch_child_window_event(scope, child_handle, "error", event);
        return Ok(());
    }
    runtime.dispatch_public_event(scope, host_ptr, EventTargetHandle::Window, event)?;
    // pilot: an exception no handler canceled is reported to the developer
    // console, which is how it reaches Playwright as a page error.
    let canceled = crate::context_bootstrap::event_backing(scope, event)
        .get(scope, v8str(scope, "defaultPrevented").into())
        .is_some_and(|value| value.boolean_value(scope));
    if let Some(error_value) = error_value
        && !canceled
    {
        let throw_site_stack_trace = throw_site
            .stack_trace
            .as_ref()
            .map(|stack_trace| v8::Local::new(scope, stack_trace));
        crate::script_vm::report_uncaught_exception_to_devtools(
            scope,
            message_text,
            filename_text,
            lineno,
            colno,
            error_value,
            throw_site_stack_trace,
            throw_site.muted_errors,
        );
    }

    let global = scope.get_current_context().global(scope);
    let global_value: v8::Local<'_, v8::Value> = global.into();
    let _ = crate::context_bootstrap::event_backing(scope, event).set(
        scope,
        v8str(scope, "target").into(),
        global_value,
    );
    let _ = crate::context_bootstrap::event_backing(scope, event).set(
        scope,
        v8str(scope, "currentTarget").into(),
        global_value,
    );
    Ok(())
}

pub(in crate::context_bootstrap) fn window_report_error_callback<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    _rv: v8::ReturnValue<'_, v8::Value>,
) {
    let Some(host_ptr) = context_host_ptr_from_global_bridge(scope) else {
        return;
    };

    let receiver = match crate::native_bridge::WindowOperationReceiver::capture_and_authorize(
        scope,
        args.this(),
        unsafe { &*host_ptr },
    ) {
        Ok(receiver) => receiver,
        Err(crate::native_bridge::WindowOperationReceiverCaptureError::IllegalInvocation) => {
            throw_type_error(scope, "Illegal invocation");
            return;
        }
        Err(crate::native_bridge::WindowOperationReceiverCaptureError::CrossOrigin) => {
            crate::native_bridge::throw_cross_origin_location_security_error(scope);
            return;
        }
    };

    if args.length() < 1 {
        throw_type_error(
            scope,
            &crate::webidl::WebIdlError::missing_required(crate::webidl::Context::argument(
                "Window.reportError",
                1,
            ))
            .to_string(),
        );
        return;
    }

    let details = reported_exception_details(scope, host_ptr, args.get(0));
    let Some(binding) = receiver.resolve_live_binding(unsafe { &*host_ptr }) else {
        // A discarded Window remains a valid receiver, but there is no live
        // global left on which to report the exception.
        return;
    };
    let dispatch_result = binding.with_current_scope(scope, host_ptr, |scope, _dispatch_scope| {
        let error = v8::Local::new(scope, &details.error);
        dispatch_window_error_event_from_throw_site(
            scope,
            host_ptr,
            &details.message,
            &details.filename,
            details.lineno,
            details.colno,
            Some(error),
            &details.throw_site,
        )
    });
    if let Some(Err(message)) = dispatch_result {
        throw_type_error(scope, &message);
    }
}
