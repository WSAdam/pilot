//! Synchronous exception reporting that completes classic-script evaluation.
//!
//! This checkpoint is part of the classic-script evaluation algorithm. It is
//! neither the script element's later load/error terminal nor the enclosing
//! selected Page task completion.

use std::pin::pin;

use anyhow::Result;

use super::ScriptVm;
use crate::context_bootstrap::dispatch_window_error_event_for_exception_report;
use crate::exception_reporting::{V8ExceptionReport, log_uncaught_script_exception};
use crate::native_bridge::JsContextHost;

impl ScriptVm {
    pub(super) fn report_classic_script_exception_and_finish_evaluation_best_effort(
        &mut self,
        report: &V8ExceptionReport,
    ) {
        log_uncaught_script_exception(report);
        if let Err(error) = self.dispatch_classic_script_exception_and_finish_evaluation(report) {
            self.record_runtime_warning(format_args!(
                "classic script exception reporting failed: {error}"
            ));
        }
    }

    fn dispatch_classic_script_exception_and_finish_evaluation(
        &mut self,
        report: &V8ExceptionReport,
    ) -> Result<()> {
        let context_ptr: *const v8::Global<v8::Context> = &self.page_default_runtime.context;
        let context_host = self._context_host.clone();
        self.renderer_document_isolate
            .with_renderer_document_isolate_mut(|isolate| {
                let scope = pin!(v8::HandleScope::new(isolate));
                let scope = &mut scope.init();
                let context = unsafe { v8::Local::new(scope, &*context_ptr) };
                let scope = &mut v8::ContextScope::new(scope, context);
                // SAFETY: as_ptr() — V8 callbacks are re-entrant; borrow_mut() panics. See util.rs.
                let host_ptr: *mut JsContextHost = (*context_host).as_ptr();
                // pilot: with where the value was thrown and whether the
                // script's errors are muted (exception_reporting.rs).
                let dispatch_result =
                    dispatch_window_error_event_for_exception_report(scope, host_ptr, report)
                        .map_err(anyhow::Error::msg);
                let checkpoint_result = Self::perform_microtask_checkpoints(scope, None);
                dispatch_result?;
                checkpoint_result
            })
    }
}
