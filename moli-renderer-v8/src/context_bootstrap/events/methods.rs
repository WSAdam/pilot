use super::*;

pub(in crate::context_bootstrap) fn set_event_default_prevented(
    scope: &mut v8::PinScope<'_, '_>,
    event: v8::Local<'_, v8::Object>,
) {
    let event = super::base::event_backing(scope, event);
    let key = v8str(scope, "defaultPrevented");
    let value = v8::Boolean::new(scope, true).into();
    let _ = event.define_own_property(scope, key.into(), value, Default::default());
}

pub(in crate::context_bootstrap) fn event_prevent_default_callback<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    _rv: v8::ReturnValue<'_, v8::Value>,
) {
    let event = args.this();
    if !crate::context_bootstrap::event_bool_attribute(scope, event, "cancelable") {
        return;
    }
    if event_internal_bool_flag(scope, event, EVENT_PASSIVE_SLOT) {
        return;
    }
    set_event_default_prevented(scope, event);
}

pub(in crate::context_bootstrap) fn event_return_value_getter_function<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'_, v8::Value>,
) {
    if event_initialized(scope, args.this()).is_none() {
        throw_type_error(scope, "Illegal invocation");
        return;
    }
    let default_prevented =
        crate::context_bootstrap::event_bool_attribute(scope, args.this(), "defaultPrevented");
    rv.set(v8::Boolean::new(scope, !default_prevented).into());
}

pub(in crate::context_bootstrap) fn event_return_value_setter_function<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    _rv: v8::ReturnValue<'_, v8::Value>,
) {
    let event = args.this();
    if event_initialized(scope, event).is_none() {
        throw_type_error(scope, "Illegal invocation");
        return;
    }
    if args.get(0).boolean_value(scope)
        || !crate::context_bootstrap::event_bool_attribute(scope, event, "cancelable")
        || event_internal_bool_flag(scope, event, EVENT_PASSIVE_SLOT)
    {
        return;
    }
    set_event_default_prevented(scope, event);
}

/// The DOMString `returnValue` of a BeforeUnloadEvent, on its private state.
pub(in crate::context_bootstrap) const BEFORE_UNLOAD_EVENT_RETURN_VALUE_SLOT: &str =
    "__moliBeforeUnloadEventReturnValue";

fn is_before_unload_event<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    event: v8::Local<'s, v8::Object>,
) -> bool {
    let backing = super::base::event_backing(scope, event);
    crate::web_api_interfaces::BeforeUnloadEvent::is_instance(scope, backing)
}

/// `BeforeUnloadEvent.prototype.returnValue` getter: the last string set,
/// initially empty.
pub(in crate::context_bootstrap) fn before_unload_event_return_value_getter_function<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'_, v8::Value>,
) {
    let event = args.this();
    if !is_before_unload_event(scope, event) {
        throw_type_error(scope, "Illegal invocation");
        return;
    }
    let value = crate::context_bootstrap::event_private_value(
        scope,
        event,
        BEFORE_UNLOAD_EVENT_RETURN_VALUE_SLOT,
    )
    .unwrap_or_else(|| v8str(scope, "").into());
    rv.set(value);
}

/// `BeforeUnloadEvent.prototype.returnValue` setter. Unlike Event's legacy
/// boolean, it stores a DOMString; any value besides the empty string asks
/// the user to confirm leaving once dispatch finishes.
pub(in crate::context_bootstrap) fn before_unload_event_return_value_setter_function<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    _rv: v8::ReturnValue<'_, v8::Value>,
) {
    let event = args.this();
    if !is_before_unload_event(scope, event) {
        throw_type_error(scope, "Illegal invocation");
        return;
    }
    // A failed DOMString conversion leaves its exception pending for script.
    let Some(value) = args.get(0).to_string(scope) else {
        return;
    };
    crate::context_bootstrap::set_event_private_value(
        scope,
        event,
        BEFORE_UNLOAD_EVENT_RETURN_VALUE_SLOT,
        value.into(),
    );
}

/// Whether a BeforeUnloadEvent's `returnValue` is a non-empty string, which
/// HTML treats as a request to confirm leaving, like a canceled event.
pub(in crate::context_bootstrap) fn before_unload_event_has_return_value<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    event: v8::Local<'s, v8::Object>,
) -> bool {
    crate::context_bootstrap::event_private_value(
        scope,
        event,
        BEFORE_UNLOAD_EVENT_RETURN_VALUE_SLOT,
    )
    .and_then(|value| v8::Local::<v8::String>::try_from(value).ok())
    .is_some_and(|value| value.length() > 0)
}

/// HTML's event handler processing for `beforeunload` on a BeforeUnloadEvent
/// (`OnBeforeUnloadEventHandler`): a return value other than null or
/// undefined, converted to a string, cancels the event and becomes its
/// `returnValue` while that is still empty. Returns false, leaving the
/// ordinary `return false` processing to the caller, when `event` is not a
/// BeforeUnloadEvent.
pub(crate) fn process_before_unload_handler_return_value<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    event: v8::Local<'s, v8::Object>,
    returned: v8::Local<'s, v8::Value>,
) -> bool {
    if !is_before_unload_event(scope, event) {
        return false;
    }
    if returned.is_null_or_undefined() {
        return true;
    }
    // A conversion that throws (a Symbol, a throwing toString) leaves the
    // event as it was, as a failed callback return conversion would.
    let text = {
        let try_catch = std::pin::pin!(v8::TryCatch::new(scope));
        let try_scope = try_catch.init();
        returned
            .to_string(&try_scope)
            .map(|text| text.to_rust_string_lossy(&try_scope))
    };
    let Some(text) = text else {
        return true;
    };
    set_event_default_prevented(scope, event);
    if !before_unload_event_has_return_value(scope, event)
        && let Some(value) = v8_string(scope, &text)
    {
        crate::context_bootstrap::set_event_private_value(
            scope,
            event,
            BEFORE_UNLOAD_EVENT_RETURN_VALUE_SLOT,
            value.into(),
        );
    }
    true
}

pub(in crate::context_bootstrap) fn event_cancel_bubble_getter_function<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'_, v8::Value>,
) {
    if event_initialized(scope, args.this()).is_none() {
        throw_type_error(scope, "Illegal invocation");
        return;
    }
    let stopped = event_internal_bool_flag(scope, args.this(), EVENT_STOP_PROPAGATION_SLOT);
    rv.set(v8::Boolean::new(scope, stopped).into());
}

pub(in crate::context_bootstrap) fn event_cancel_bubble_setter_function<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    _rv: v8::ReturnValue<'_, v8::Value>,
) {
    if event_initialized(scope, args.this()).is_none() {
        throw_type_error(scope, "Illegal invocation");
        return;
    }
    if !args.get(0).boolean_value(scope) {
        return;
    }
    set_event_internal_flag(scope, args.this(), EVENT_STOP_PROPAGATION_SLOT, true);
}

pub(in crate::context_bootstrap) fn event_stop_propagation_callback<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    _rv: v8::ReturnValue<'_, v8::Value>,
) {
    let event = args.this();
    set_event_internal_flag(scope, event, EVENT_STOP_PROPAGATION_SLOT, true);
}

pub(in crate::context_bootstrap) fn event_stop_immediate_propagation_callback<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    _rv: v8::ReturnValue<'_, v8::Value>,
) {
    let event = args.this();
    set_event_internal_flag(scope, event, EVENT_STOP_PROPAGATION_SLOT, true);
    set_event_internal_flag(scope, event, EVENT_STOP_IMMEDIATE_PROPAGATION_SLOT, true);
}

pub(in crate::context_bootstrap) fn event_composed_path_callback<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'_, v8::Value>,
) {
    let value = super::base::event_composed_path_value(scope, args.this());
    rv.set(value);
}

pub(in crate::context_bootstrap) fn event_time_stamp_getter_function<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<'_, v8::Value>,
) {
    let time_stamp = super::base::event_time_stamp(scope, args.this());
    rv.set(v8::Number::new(scope, time_stamp).into());
}
