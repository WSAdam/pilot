use super::*;

// 2026-11-03: setDeviceMetricsOverride rejected mobile=true, so Playwright
// contexts with a phone's device descriptor (Playwright MCP's --device) could
// not open. Under pilot's Live layout, mobile emulation lays the page out at
// the device width; there is no viewport-meta processing yet.
#[tokio::test(flavor = "multi_thread")]
async fn live_layout_accepts_mobile_device_metrics_at_the_device_width() {
    let mut ctx = TestContext::new_with_layout_policy(moli_core::LayoutPolicy::Live);
    load_session_page_for_pending_emulation_test(&mut ctx).await;
    expect_session_command_result(
        &mut ctx,
        88500,
        "SID-1",
        "Emulation.setDeviceMetricsOverride",
        json!({"width": 390, "height": 664, "deviceScaleFactor": 3, "mobile": true}),
    )
    .await;
    assert_eq!(
        evaluate(&mut ctx, "[innerWidth, innerHeight, devicePixelRatio]").await,
        json!([390, 664, 3])
    );
}

// 2026-11-10: a ResizeObserver on document.body never fired when the viewport
// changed size: ResizeObserver checks were queued only on DOM mutations and
// observe(), not on a viewport resize (Playwright MCP browser_resize).
#[tokio::test(flavor = "multi_thread")]
async fn a_viewport_resize_reaches_a_resize_observer_on_the_body() {
    let mut ctx = TestContext::new_with_layout_policy(moli_core::LayoutPolicy::Live);
    load_session_page_for_pending_emulation_test(&mut ctx).await;
    expect_session_command_result(
        &mut ctx,
        88510,
        "SID-1",
        "Emulation.setDeviceMetricsOverride",
        json!({"width": 1280, "height": 720, "deviceScaleFactor": 1, "mobile": false}),
    )
    .await;
    evaluate(
        &mut ctx,
        r#"
        globalThis.observedSizes = [];
        new ResizeObserver(() => observedSizes.push(innerWidth + 'x' + innerHeight))
            .observe(document.body || document.documentElement);
        new Promise(requestAnimationFrame).then(() => observedSizes.length)
    "#,
    )
    .await;
    expect_session_command_result(
        &mut ctx,
        88511,
        "SID-1",
        "Emulation.setDeviceMetricsOverride",
        json!({"width": 390, "height": 780, "deviceScaleFactor": 1, "mobile": false}),
    )
    .await;
    assert_eq!(
        evaluate(
            &mut ctx,
            r#"
            new Promise(resolve => {
                const check = () => observedSizes.includes('390x780')
                    ? resolve(true)
                    : requestAnimationFrame(check);
                check();
                setTimeout(() => resolve(observedSizes), 2000);
            })
        "#
        )
        .await,
        json!(true)
    );
}
