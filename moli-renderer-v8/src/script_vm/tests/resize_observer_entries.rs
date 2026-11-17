use super::*;

#[tokio::test(flavor = "current_thread")]
async fn resize_observer_entries_preserve_webidl_snapshots_and_callback_realms() {
    let loader = ResourceRequestClient::new(&moli_fetch::FetchConfig::default()).unwrap();
    let mut vm = new_storage_page_task_executor_test_vm("https://resize-entries.test/");
    let count: usize = vm
        .eval(include_str!(
            "../../../tests/fixtures/resize-observer-entry-webidl.js"
        ))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(count, 12);
    vm.eval(
        "__resizeEntryChecks.resumeAfterLayout = null;
         __resizeEntryChecks.publishLayout = () => new Promise(resolve => {
           __resizeEntryChecks.resumeAfterLayout = resolve;
         });",
    )
    .unwrap();
    for index in 0..count {
        let name = vm
            .eval(&format!("__resizeEntryChecks.start({index})"))
            .unwrap();
        let mut publications = 0;
        loop {
            advance_page_task_executor_until_eval_equals(
                &mut vm,
                &loader,
                "String(__resizeEntryChecks.result !== null || __resizeEntryChecks.resumeAfterLayout !== null)",
                "true",
                &name,
            )
            .await;
            if vm.eval("__resizeEntryChecks.result !== null").unwrap() == "true" {
                break;
            }
            publications += 1;
            assert!(publications <= 2, "{name}: unexpected layout request");
            vm.publish_layout_for_test().unwrap();
            vm.eval(
                "(() => {
                   const resume = __resizeEntryChecks.resumeAfterLayout;
                   __resizeEntryChecks.resumeAfterLayout = null;
                   resume();
                 })()",
            )
            .unwrap();
        }
        let result = vm.eval("__resizeEntryChecks.result").unwrap();
        let result: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(result["total"].as_u64().unwrap() > 0, "{name}");
        assert_eq!(result["failures"], serde_json::json!([]), "{name}");
    }
}

#[test]
fn ondemand_ro_uses_published_css_box_axes_and_unzoomed_dimensions() {
    let mut vm = new_storage_html_test_vm("https://resize-published-axis.test/");
    vm.set_layout_policy(moli_page_types::LayoutPolicy::OnDemand);
    vm.eval(r#"
        document.body.innerHTML='<div id=target style="display:block;width:50px;height:100px;padding:0;border:2px solid;box-sizing:content-box;writing-mode:vertical-lr;zoom:2"></div>';
        window.samples=[];
    "#).unwrap();
    refresh_layout_for_test(&mut vm);
    let builds = vm.layout_pass_observability_for_test().1;
    vm.eval(
        r#"
        window.observer=new ResizeObserver(entries=>samples.push(...entries.map(e=>[
            e.contentRect.width,e.contentRect.height,
            e.contentBoxSize[0].inlineSize,e.contentBoxSize[0].blockSize,
            e.borderBoxSize[0].inlineSize,e.borderBoxSize[0].blockSize
        ])));
        observer.observe(target);
    "#,
    )
    .unwrap();
    assert_eq!(
        vm.eval("JSON.stringify(samples)").unwrap(),
        "[[50,100,100,50,104,54]]"
    );
    assert_eq!(vm.layout_pass_observability_for_test().1, builds);

    vm.eval("target.style.writingMode='horizontal-tb'").unwrap();
    assert_eq!(
        vm.eval("JSON.stringify(samples)").unwrap(),
        "[[50,100,100,50,104,54]]",
        "OnDemand must continue sampling the axes frozen with its previous layout"
    );
    assert_eq!(vm.layout_pass_observability_for_test().1, builds);
    refresh_layout_for_test(&mut vm);
    let builds = vm.layout_pass_observability_for_test().1;
    // Publishing a snapshot is not itself a legacy RO scheduling operation.
    // This mutation schedules its existing check without requesting layout.
    vm.eval("target.setAttribute('data-resample','yes')")
        .unwrap();
    assert_eq!(
        vm.eval("JSON.stringify(samples)").unwrap(),
        "[[50,100,100,50,104,54],[50,100,50,100,54,104]]"
    );
    assert_eq!(vm.layout_pass_observability_for_test().1, builds);
    assert_eq!(
        vm.layout_pass_observability_for_test()
            .3
            .unwrap()
            .paint_operation_count,
        0
    );
}

#[test]
fn real_ro_css_box_projection_excludes_non_replaced_inline_fragments() {
    let mut vm = new_storage_html_test_vm("https://resize-css-box-kind.test/");
    vm.set_layout_policy(moli_page_types::LayoutPolicy::OnDemand);
    vm.eval(r#"
        document.body.innerHTML='<span id=inline style="padding:0;border:2px solid">text</span><span id=atomic style="display:inline-block;width:50px;height:100px;padding:0;border:2px solid;box-sizing:content-box;writing-mode:vertical-lr"></span>';
        window.samples=[];
    "#).unwrap();
    refresh_layout_for_test(&mut vm);
    let builds = vm.layout_pass_observability_for_test().1;
    vm.eval(
        r#"
        window.observer=new ResizeObserver(entries=>samples.push(...entries.map(e=>[
            e.target.id,e.contentRect.width,e.contentRect.height,
            e.contentBoxSize[0].inlineSize,e.contentBoxSize[0].blockSize,
            e.borderBoxSize[0].inlineSize,e.borderBoxSize[0].blockSize
        ])));
        observer.observe(document.getElementById('inline'));
        observer.observe(document.getElementById('atomic'));
    "#,
    )
    .unwrap();
    assert_eq!(
        vm.eval("JSON.stringify(samples)").unwrap(),
        r#"[["inline",0,0,0,0,0,0],["atomic",50,100,100,50,104,54]]"#,
        "a non-replaced inline's CSSOM fragment union is not an RO CSS box"
    );
    assert_eq!(vm.layout_pass_observability_for_test().1, builds);
}

// 2026-11-17: a listbox row named its <option> only as a geometry and hit source, never as the option's principal box, so ResizeObserver found no CSS box for the option and reported a 0x0 contentRect.
#[test]
fn listbox_option_resize_observer_reports_its_row() {
    let mut vm = new_storage_html_test_vm("https://resize-listbox-option.test/");
    vm.set_layout_policy(moli_page_types::LayoutPolicy::OnDemand);
    vm.eval(r#"
        document.body.innerHTML='<select id=list multiple size=3 style="display:block;width:140px;margin:0;padding:0;border:1px solid;font:16px/20px sans-serif"><option>Foo</option><option>Bar</option><option>Baz</option></select>';
        window.samples=[];
    "#).unwrap();
    refresh_layout_for_test(&mut vm);
    vm.eval(
        r#"
        window.observer=new ResizeObserver(entries=>samples.push(...entries.map(e=>[
            e.contentRect.width,e.contentRect.height,
            e.contentBoxSize[0].blockSize,e.borderBoxSize[0].blockSize
        ])));
        for (const option of document.getElementById('list').options) observer.observe(option);
    "#,
    )
    .unwrap();
    let samples: Vec<[f64; 4]> =
        serde_json::from_str(&vm.eval("JSON.stringify(samples)").unwrap()).unwrap();
    assert_eq!(
        samples.len(),
        3,
        "each listbox option is observed: {samples:?}"
    );
    for (index, [width, height, content_block, border_block]) in samples.iter().copied().enumerate()
    {
        assert!(
            width > 0.0 && height > 0.0 && content_block > 0.0 && border_block > 0.0,
            "option {index} reports its row's CSS box: {samples:?}"
        );
    }
}
