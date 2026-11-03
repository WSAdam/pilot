use super::*;

fn type_text(vm: &mut StandaloneScriptVmHarness, text: &str) {
    for character in text.chars() {
        let key = character.to_string();
        let code = format!("Key{}", character.to_ascii_uppercase());
        vm.dispatch_key_event("keydown", &key, &code, &key, 0, false, true)
            .unwrap();
        vm.dispatch_key_event("keyup", &key, &code, "", 0, false, false)
            .unwrap();
    }
}

fn press_enter(vm: &mut StandaloneScriptVmHarness) {
    vm.dispatch_key_event("keydown", "Enter", "Enter", "\r", 0, false, true)
        .unwrap();
    vm.dispatch_key_event("keyup", "Enter", "Enter", "", 0, false, false)
        .unwrap();
}

// 2026-11-03: Enter in a text input fired no change event, so apps that commit
// on change ignored it: TodoMVC's vanilla app never added a todo. Chrome
// fires change on Enter when the value changed since the last commit, then
// submits the form implicitly.
#[test]
fn enter_in_a_text_input_commits_the_value_with_one_change_event() {
    let mut vm = new_parsed_test_vm(
        "https://enter-commit.test/",
        "<!doctype html><input id=field>",
    );
    vm.eval(
        "window.events=[];field.addEventListener('change',()=>events.push('change:'+field.value));field.focus()",
    )
    .unwrap();
    type_text(&mut vm, "ab");
    press_enter(&mut vm);
    assert_eq!(
        vm.eval("events.join()").unwrap(),
        "change:ab",
        "Enter commits the edit"
    );

    vm.eval("field.blur()").unwrap();
    assert_eq!(
        vm.eval("events.join()").unwrap(),
        "change:ab",
        "blur after Enter has nothing new to commit"
    );

    vm.eval("field.focus()").unwrap();
    press_enter(&mut vm);
    assert_eq!(
        vm.eval("events.join()").unwrap(),
        "change:ab",
        "Enter without an edit fires nothing"
    );
}

#[test]
fn enter_in_a_form_field_fires_change_before_the_implicit_submission() {
    let mut vm = new_parsed_test_vm(
        "https://enter-commit.test/",
        "<!doctype html><form id=form><input id=field></form>",
    );
    vm.eval(
        "window.events=[];field.addEventListener('change',()=>events.push('change'));\
         form.addEventListener('submit',e=>{e.preventDefault();events.push('submit')});field.focus()",
    )
    .unwrap();
    type_text(&mut vm, "x");
    press_enter(&mut vm);
    assert_eq!(vm.eval("events.join()").unwrap(), "change,submit");
}
