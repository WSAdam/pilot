use super::*;

fn press_at(vm: &mut StandaloneScriptVmHarness, x: f64, y: f64) {
    for event in ["mousedown", "mouseup"] {
        vm.dispatch_mouse_event_at_point(
            x,
            y,
            event,
            0,
            Some(i32::from(event == "mousedown")),
            0.0,
            0.0,
        )
        .unwrap();
    }
}

// 2026-11-03: pressing the mouse on non-focusable content moved focus to the
// body and scrolled the body into view, which for a page taller than the
// viewport jumped to the bottom. Chrome never scrolls for mouse focus: the
// pressed point is already on screen. Playwright's raw mouse drags then
// missed their target, and an agent watching the page saw it jump.
#[test]
fn a_mouse_press_on_text_or_a_tall_focusable_element_does_not_scroll_the_page() {
    let mut vm = new_parsed_test_vm(
        "https://mouse-focus.test/",
        "<!doctype html><body style='margin:0'><p style='margin:0;height:40px'>text</p>\
         <div id=editor tabindex=0 style='height:3000px'>tall focusable</div></body>",
    );
    refresh_layout_for_test(&mut vm);

    press_at(&mut vm, 10.0, 10.0);
    assert_eq!(vm.eval("String(scrollY)").unwrap(), "0", "a press on text");

    press_at(&mut vm, 10.0, 100.0);
    assert_eq!(
        vm.eval("String(document.activeElement.id)").unwrap(),
        "editor",
        "the press still focuses the element under it"
    );
    assert_eq!(
        vm.eval("String(scrollY)").unwrap(),
        "0",
        "a press on a tall focusable element"
    );
}
