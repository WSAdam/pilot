use super::*;

// pilot's layout policy. Upstream's OnDemand keeps its frozen snapshot.
const LIVE: moli_page_types::LayoutPolicy = moli_page_types::LayoutPolicy::Live;

fn live_vm(markup: &str) -> StandaloneScriptVmHarness {
    let mut vm = new_parsed_test_vm("https://live-scroll.test/", markup);
    vm.set_layout_policy(LIVE);
    vm
}

// 2026-11-02: the latest layout bakes in the scroll offsets it was built
// with, and a scroll records no DOM change, so after scrolling the viewport
// getBoundingClientRect and DOM.getContentQuads kept the pre-scroll position.
// Playwright scrolled a below-the-fold button into view, still saw it outside
// the viewport, and never clicked it.
#[test]
fn viewport_scroll_after_the_first_layout_moves_client_rects() {
    let mut vm = live_vm(
        "<!doctype html><body style='margin:0'><div style='height:2000px'></div><button id=below style='display:block;height:40px'>below</button><div style='height:2000px'></div>",
    );
    assert_eq!(
        vm.eval("String(below.getBoundingClientRect().top)")
            .unwrap(),
        "2000"
    );
    assert_eq!(
        vm.eval("scrollTo(0, 1500); String(below.getBoundingClientRect().top)")
            .unwrap(),
        "500",
        "a read after the scroll"
    );
    assert_eq!(
        vm.eval("String(document.elementFromPoint(10, 520) === below)")
            .unwrap(),
        "true",
        "hit testing uses the scrolled position"
    );
}

#[test]
fn element_scroll_after_the_first_layout_moves_its_content_rects() {
    let mut vm = live_vm(
        "<!doctype html><body style='margin:0'><div id=scroller style='height:100px;overflow:auto'><div style='height:300px'></div><button id=inner style='display:block;height:40px'>inner</button></div>",
    );
    assert_eq!(
        vm.eval("String(inner.getBoundingClientRect().top)")
            .unwrap(),
        "300"
    );
    assert_eq!(
        vm.eval("scroller.scrollTop = 200; String(inner.getBoundingClientRect().top)")
            .unwrap(),
        "100",
        "a read after the scroller moved"
    );
}

#[test]
fn reads_after_a_scroll_to_the_same_offset_reuse_the_layout() {
    let mut vm = live_vm(
        "<!doctype html><body style='margin:0'><div style='height:2000px'></div><button id=below>below</button><div style='height:2000px'></div>",
    );
    vm.eval("scrollTo(0, 1500); below.getBoundingClientRect()")
        .unwrap();
    let built = vm.layout_pass_observability_for_test().1;
    vm.eval("scrollTo(0, 1500); below.getBoundingClientRect(); below.getBoundingClientRect()")
        .unwrap();
    assert_eq!(vm.layout_pass_observability_for_test().1, built);
}
