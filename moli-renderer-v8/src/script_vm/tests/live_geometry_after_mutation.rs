use super::*;

// pilot's layout policy. Upstream's OnDemand keeps its frozen snapshot.
const LIVE: moli_page_types::LayoutPolicy = moli_page_types::LayoutPolicy::Live;

fn mousemove_at(vm: &mut StandaloneScriptVmHarness, x: f64, y: f64) {
    vm.dispatch_mouse_event_at_point(x, y, "mousemove", 0, Some(0), 0.0, 0.0)
        .unwrap();
}

// 2026-11-02: OnDemand layout froze the first geometry snapshot, so a node
// appended or restyled after the first geometry read kept its old (or 0x0)
// box. Playwright then treats the node as invisible: no MCP snapshot ref, and
// check/hover/click time out (upstream #1080, #1289, #2069).
#[test]
fn geometry_reads_see_nodes_appended_and_restyled_after_the_first_layout() {
    let mut vm = new_storage_html_test_vm("https://live-geometry.test/");
    vm.set_layout_policy(LIVE);
    assert_eq!(
        vm.eval("document.body.getBoundingClientRect().width > 0")
            .unwrap(),
        "true",
        "the first geometry read builds a real layout"
    );

    let appended = vm
        .eval(
            r#"
            globalThis.late = document.createElement('button');
            late.style.cssText = 'display:block;width:120px;height:40px;margin:0';
            late.textContent = 'Late';
            document.body.append(late);
            const box = late.getBoundingClientRect();
            `${box.width}x${box.height}`
            "#,
        )
        .unwrap();
    assert_eq!(appended, "120x40", "a node appended after the first read");

    let restyled = vm
        .eval("late.style.width = '200px'; String(late.getBoundingClientRect().width)")
        .unwrap();
    assert_eq!(restyled, "200", "a node restyled after the last read");

    let hit = vm
        .eval(
            r#"
            (() => {
                const center = late.getBoundingClientRect();
                const x = center.x + center.width / 2;
                const y = center.y + center.height / 2;
                return String(document.elementFromPoint(x, y) === late);
            })()
            "#,
        )
        .unwrap();
    assert_eq!(hit, "true", "hit testing uses the current layout");
}

// 2026-11-02: input hit-tested the last published layout, so a mouse event
// aimed at a restyled node's new box missed it (and one at the old box hit it).
#[test]
fn mouse_input_hit_tests_a_restyled_node_at_its_new_position() {
    let mut vm = new_parsed_test_vm(
        "https://live-geometry.test/",
        // The body must cover the old box, or a move there hits no element.
        "<!doctype html><style>body{min-height:100px}</style><button id=target style='position:absolute;left:20px;top:20px;width:100px;height:100px'>go</button>",
    );
    vm.set_layout_policy(LIVE);
    refresh_layout_for_test(&mut vm);
    vm.eval(
        "window.hits=[];document.onmousemove=e=>hits.push(e.target.id);target.style.left='300px'",
    )
    .unwrap();
    mousemove_at(&mut vm, 60.0, 60.0);
    mousemove_at(&mut vm, 350.0, 60.0);
    assert_eq!(
        vm.eval("JSON.stringify(hits)").unwrap(),
        r#"["","target"]"#,
        "the old box is empty and the new box hits the node"
    );
}

// Live layout must not cost a layout pass per read: only a DOM change since
// the latest layout rebuilds it.
#[test]
fn geometry_reads_without_dom_changes_reuse_one_layout() {
    let mut vm = new_parsed_test_vm(
        "https://live-geometry.test/",
        "<!doctype html><button id=target style='width:100px;height:40px'>go</button>",
    );
    vm.set_layout_policy(LIVE);
    vm.eval("target.getBoundingClientRect()").unwrap();
    let built = vm.layout_pass_observability_for_test().1;
    let reads = "[target.getBoundingClientRect().width, target.offsetWidth, \
                 document.elementFromPoint(20, 20) === target, target.checkVisibility()].join()";
    for _ in 0..3 {
        assert_eq!(vm.eval(reads).unwrap(), "100,100,true,true");
    }
    assert_eq!(
        vm.layout_pass_observability_for_test().1,
        built,
        "reads with no DOM change reuse the latest layout"
    );

    assert_eq!(
        vm.eval("target.style.width='150px'; String(target.offsetWidth)")
            .unwrap(),
        "150"
    );
    vm.eval(reads).unwrap();
    assert_eq!(
        vm.layout_pass_observability_for_test().1,
        built + 1,
        "one DOM change costs one rebuild"
    );
}
