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

// 2026-11-10: under Live layout, a viewport resize left the page laid out at
// the old width (body 1264px wide after resizing to 390px): the latest layout
// was reused because neither the DOM nor a scroll offset had changed.
#[tokio::test(flavor = "multi_thread")]
async fn live_layout_reflows_the_page_after_a_viewport_resize() {
    let mut ctx = TestContext::new_with_layout_policy(moli_core::LayoutPolicy::Live);
    load_session_page_for_pending_emulation_test(&mut ctx).await;
    expect_session_command_result(
        &mut ctx,
        88520,
        "SID-1",
        "Emulation.setDeviceMetricsOverride",
        json!({"width": 1280, "height": 720, "deviceScaleFactor": 1, "mobile": false}),
    )
    .await;
    assert_eq!(
        evaluate(
            &mut ctx,
            "document.documentElement.getBoundingClientRect().width"
        )
        .await,
        json!(1280)
    );
    expect_session_command_result(
        &mut ctx,
        88521,
        "SID-1",
        "Emulation.setDeviceMetricsOverride",
        json!({"width": 390, "height": 780, "deviceScaleFactor": 1, "mobile": false}),
    )
    .await;
    assert_eq!(
        evaluate(
            &mut ctx,
            "[innerWidth, document.documentElement.getBoundingClientRect().width]"
        )
        .await,
        json!([390, 390])
    );
}
