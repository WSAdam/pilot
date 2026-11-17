use super::*;

// 2026-11-17: the main Document ran an inserted inline script with no TryCatch and no "report the exception" step (execute_committed_inline_classic_script), so its exception was lost inside the insertion: no error event, no Runtime.exceptionThrown.
#[test]
fn main_dynamic_inline_script_exceptions_are_reported_before_insertion_returns() {
    let mut vm = new_storage_test_vm("https://main-inline-errors.test/");
    let result = vm
        .eval(
            r#"
            (() => {
              const root = document.documentElement || document.appendChild(document.createElement('html'));
              const body = document.body || root.appendChild(document.createElement('body'));
              const events = [];
              window.onerror = (message, source, line, column, error) => {
                events.push(error.name + ':' + (error instanceof Error) + ':' +
                            document.currentScript.id);
                return true;
              };
              for (const [id, source] of [['syntax', '{'], ['runtime', "throw new TypeError('test')"]]) {
                const script = document.createElement('script');
                script.id = id;
                script.textContent = source;
                script.onload = () => events.push('unexpected load');
                script.onerror = () => events.push('unexpected element error');
                try {
                  body.appendChild(script);
                  events.push('returned');
                } catch (error) {
                  events.push('escaped:' + error.name);
                }
              }
              return JSON.stringify(events);
            })()
            "#,
        )
        .unwrap();
    assert_eq!(
        result,
        r#"["SyntaxError:true:syntax","returned","TypeError:true:runtime","returned"]"#
    );
}
