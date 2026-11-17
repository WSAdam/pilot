use super::*;

// 2026-11-03: setDeviceMetricsOverride rejected mobile=true, so Playwright
// contexts with a phone's device descriptor (Playwright MCP's --device) could
// not open. Under pilot's Live layout a phone lays a width=device-width page
// out at the device width (a page without a viewport meta tag gets 980px).
#[tokio::test(flavor = "multi_thread")]
async fn live_layout_accepts_mobile_device_metrics_at_the_device_width() {
    let mut ctx = TestContext::new_with_layout_policy(moli_core::LayoutPolicy::Live);
    load_session_page_for_pending_emulation_test_at_url(
        &mut ctx,
        "data:text/html,<meta name=viewport content=width=device-width><body>emulation</body>",
    )
    .await;
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

// 2026-11-17: a ResizeObserver on document.body never fired after a viewport
// resize (Playwright MCP browser_resize). Observers only read a layout
// something else had built; on a page nothing had measured they read 0x0
// before and after the resize, saw no change, and stayed silent.
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
        globalThis.observed = [];
        globalThis.resizeEvents = [];
        addEventListener('resize', () => resizeEvents.push(innerWidth + 'x' + innerHeight));
        new ResizeObserver(entries => observed.push({
            size: innerWidth + 'x' + innerHeight,
            width: entries[entries.length - 1].contentRect.width,
        })).observe(document.body || document.documentElement);
        new Promise(requestAnimationFrame).then(() => observed.length)
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
                const seen = () => observed.some(entry =>
                    entry.size === '390x780' && entry.width > 0 && entry.width <= 390);
                const check = () => seen() ? resolve(true) : requestAnimationFrame(check);
                check();
                setTimeout(() => resolve({ observed, resizeEvents }), 2000);
            })
        "#
        )
        .await,
        json!(true)
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

/// `innerWidth`, `innerHeight` and the layout viewport (root client size).
const WINDOW_AND_LAYOUT: &str = "[innerWidth, innerHeight, document.documentElement.clientWidth, document.documentElement.clientHeight]";

/// Loads `html` and emulates a 390x844 screen at DPR 3, as Playwright does
/// for `isMobile: true` (a phone) or `false` (desktop).
async fn load_page_on_a_390px_screen(ctx: &mut TestContext, html: &str, mobile: bool, id: u64) {
    load_session_page_for_pending_emulation_test_at_url(ctx, &format!("data:text/html,{html}"))
        .await;
    expect_session_command_result(
        ctx,
        id,
        "SID-1",
        "Emulation.setDeviceMetricsOverride",
        json!({"width": 390, "height": 844, "deviceScaleFactor": 3, "mobile": mobile}),
    )
    .await;
}

// 2026-11-17: under phone emulation pilot laid a page without <meta name=viewport> out at the 390px device width; Chrome lays it out at 980px and scales it to fit.
#[tokio::test(flavor = "multi_thread")]
async fn a_phone_lays_out_a_page_without_a_viewport_meta_tag_at_980px() {
    let mut ctx = TestContext::new_with_layout_policy(moli_core::LayoutPolicy::Live);
    load_page_on_a_390px_screen(&mut ctx, "<p>no viewport meta</p>", true, 88530).await;
    // Chrome: [980, 2121, 980, 2120] (980 * 844 / 390 = 2120.8).
    assert_eq!(
        evaluate(&mut ctx, WINDOW_AND_LAYOUT).await,
        json!([980, 2121, 980, 2120])
    );
    assert_eq!(
        evaluate(
            &mut ctx,
            r#"[
                visualViewport.width,
                Math.abs(visualViewport.scale - 390 / 980) < 1e-6,
                matchMedia('(min-width: 900px)').matches,
                document.body.getBoundingClientRect().width,
                devicePixelRatio
            ]"#
        )
        .await,
        json!([980, true, true, 964, 3])
    );
}

// 2026-11-17: a phone must lay a width=device-width page out at the device width, not 980px (Playwright's "should support mobile meta viewport").
#[tokio::test(flavor = "multi_thread")]
async fn a_phone_lays_out_a_width_device_width_page_at_the_device_width() {
    let mut ctx = TestContext::new_with_layout_policy(moli_core::LayoutPolicy::Live);
    load_page_on_a_390px_screen(
        &mut ctx,
        "<meta name=viewport content=width=device-width,initial-scale=1><p>device width</p>",
        true,
        88540,
    )
    .await;
    assert_eq!(
        evaluate(&mut ctx, WINDOW_AND_LAYOUT).await,
        json!([390, 844, 390, 844])
    );
    assert_eq!(
        evaluate(&mut ctx, "[visualViewport.width, visualViewport.scale]").await,
        json!([390, 1])
    );
}

// 2026-11-17: under phone emulation pilot laid a width=600 page out at the 390px device width; Chrome lays it out at 600px.
#[tokio::test(flavor = "multi_thread")]
async fn a_phone_lays_out_a_width_600_page_at_600px() {
    let mut ctx = TestContext::new_with_layout_policy(moli_core::LayoutPolicy::Live);
    load_page_on_a_390px_screen(
        &mut ctx,
        "<meta name=viewport content=width=600><p>six hundred</p>",
        true,
        88550,
    )
    .await;
    // Chrome: [600, 1299, 600, 1298] (600 * 844 / 390 = 1298.5).
    assert_eq!(
        evaluate(&mut ctx, WINDOW_AND_LAYOUT).await,
        json!([600, 1299, 600, 1298])
    );
    assert_eq!(
        evaluate(&mut ctx, "document.body.getBoundingClientRect().width").await,
        json!(584)
    );
}

// 2026-11-17: desktop emulation (mobile: false) must keep ignoring the viewport meta tag, as Chrome does.
#[tokio::test(flavor = "multi_thread")]
async fn desktop_emulation_ignores_the_viewport_meta_tag() {
    let mut ctx = TestContext::new_with_layout_policy(moli_core::LayoutPolicy::Live);
    load_page_on_a_390px_screen(
        &mut ctx,
        "<meta name=viewport content=width=600><p>desktop</p>",
        false,
        88560,
    )
    .await;
    assert_eq!(
        evaluate(&mut ctx, WINDOW_AND_LAYOUT).await,
        json!([390, 844, 390, 844])
    );
    assert_eq!(
        evaluate(&mut ctx, "[visualViewport.width, visualViewport.scale]").await,
        json!([390, 1])
    );
}

// 2026-11-17: the phone layout width must follow the current Document's viewport meta tag after page.setContent rewrites it or a script edits the tag.
#[tokio::test(flavor = "multi_thread")]
async fn a_phone_layout_follows_the_viewport_meta_tag_of_the_current_document() {
    let mut ctx = TestContext::new_with_layout_policy(moli_core::LayoutPolicy::Live);
    load_page_on_a_390px_screen(&mut ctx, "<p>no viewport meta</p>", true, 88570).await;
    let layout_width = "[innerWidth, document.documentElement.clientWidth]";
    assert_eq!(evaluate(&mut ctx, layout_width).await, json!([980, 980]));
    // Playwright's page.setContent rewrites the Document this way.
    evaluate(
        &mut ctx,
        "document.open(); document.write('<meta name=viewport content=width=device-width><p>rewritten</p>'); document.close(); true",
    )
    .await;
    assert_eq!(evaluate(&mut ctx, layout_width).await, json!([390, 390]));
    evaluate(
        &mut ctx,
        "document.querySelector('meta[name=viewport]').content = 'width=700'; true",
    )
    .await;
    assert_eq!(evaluate(&mut ctx, layout_width).await, json!([700, 700]));
}
