use super::*;
use crate::domains::page::{
    PageScreencastCaptureCompletion, PageScreencastCaptureStart, PageScreencastRegistration,
};
use moli_core::page::RendererVisualStateToken;

const SESSION: &str = "SID-VIEW-CAPTURE";
const HTML: &str = r#"<!doctype html><style>
    html { background: blue; scrollbar-width: none }
    body { margin: 0 }
    #stripe { width: 40px; height: 100px; background: red }
</style><div id="stripe"></div>"#;

async fn command(ctx: &mut TestContext, method: &str, params: Value) -> Value {
    ctx.process_async(json!({
        "id": 88500, "sessionId": SESSION, "method": method, "params": params
    }))
    .await;
    let response = ctx
        .wait_for_scheduler_message(method, |message| message["id"] == 88500)
        .await;
    assert!(response["error"].is_null(), "{method}: {response}");
    assert!(
        response["result"]["exceptionDetails"].is_null(),
        "{method}: {response}"
    );
    response["result"].clone()
}

async fn set_metrics(ctx: &mut TestContext, extra: Value) {
    let mut params = json!({
        "width": 200, "height": 100, "deviceScaleFactor": 1, "mobile": false
    });
    params
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    command(ctx, "Emulation.setDeviceMetricsOverride", params).await;
}

async fn install_page(ctx: &mut TestContext) {
    install_active_screenshot_page(
        ctx,
        "BID-VIEW-CAPTURE",
        "TID-VIEW-CAPTURE",
        SESSION,
        &screenshot_data_url(HTML),
    )
    .await;
    set_metrics(ctx, json!({})).await;
}

fn start_screencast(ctx: &mut TestContext, extra: Value) -> PageScreencastRegistration {
    let mut params = json!({"format": "png"});
    params
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let raw = json!({
        "id": 88501, "sessionId": SESSION, "method": "Page.startScreencast", "params": params
    })
    .to_string();
    let CdpCommandTaskStep::Complete(outcome) =
        ctx.with_fixture(|conn| conn.start_command_dispatch(&raw))
    else {
        panic!("screencast registration should complete synchronously");
    };
    let (messages, events) = outcome.into_parts();
    assert!(
        messages
            .iter()
            .any(|message| message["id"] == 88501 && message["result"] == json!({}))
    );
    events
        .into_iter()
        .find_map(|event| match event {
            CdpSchedulerEvent::PageScreencastStarted { registration } => Some(registration),
            _ => None,
        })
        .expect("screencast registration")
}

async fn poll_screencast(
    ctx: &mut TestContext,
    registration: &PageScreencastRegistration,
    previous: Option<RendererVisualStateToken>,
) -> PageScreencastCaptureCompletion {
    let PageScreencastCaptureStart::Pending(capture) =
        ctx.with_fixture(|conn| conn.start_page_screencast_frame_capture(registration, previous))
    else {
        panic!("registered screencast should start a renderer capture");
    };
    let completed = capture.wait().await;
    ctx.with_fixture(|conn| conn.complete_page_screencast_frame_capture(completed))
}

fn take_frame(
    ctx: &mut TestContext,
    registration: &PageScreencastRegistration,
    completion: PageScreencastCaptureCompletion,
) -> (Value, RendererVisualStateToken) {
    let PageScreencastCaptureCompletion::Frame {
        event,
        visual_state,
    } = completion
    else {
        panic!("changed presentation must produce a frame");
    };
    let (event, _) = event.into_parts();
    ctx.with_fixture(|conn| {
        assert_eq!(
            conn.acknowledge_page_screencast_frame_for_session_owner(
                Some(SESSION),
                registration.generation(),
            ),
            Some(true)
        )
    });
    (event["params"].clone(), visual_state)
}

fn png(data: &Value) -> Vec<u8> {
    BASE64_STANDARD
        .decode(data.as_str().expect("encoded PNG"))
        .unwrap()
}

#[track_caller]
fn assert_blue_boundary(bytes: &[u8], dimensions: (u32, u32), boundary: Option<u32>) {
    assert_png_dimensions(bytes, dimensions.0, dimensions.1);
    let image = moli_image::decode_png(bytes).unwrap();
    let row = 10 * image.width as usize * 4;
    let first_blue = image.rgba[row..row + image.width as usize * 4]
        .as_chunks::<4>()
        .0
        .iter()
        .position(|pixel| *pixel == [0, 0, 255, 255])
        .map(|x| x as u32);
    assert_eq!(first_blue, boundary);
}

#[tokio::test(flavor = "multi_thread")]
async fn emulated_view_capture_distinguishes_layout_widget_and_explicit_clip() {
    let mut ctx = TestContext::new();
    install_page(&mut ctx).await;

    // Expected pixels were checked against Chromium. In particular, an
    // ordinary surface screenshot resets preview scale but keeps a forced
    // viewport; a clip supplied on captureScreenshot replaces that viewport.
    let viewport = json!({"x": 30, "y": 0, "width": 60, "height": 50, "scale": 2});
    let cases = [
        (
            json!({"scale": 0.5}),
            (200, 100),
            Some(20),
            (200, 100),
            Some(40),
            1,
        ),
        (
            json!({"scale": 2}),
            (200, 100),
            Some(80),
            (200, 100),
            Some(40),
            1,
        ),
        (
            json!({"viewport": viewport}),
            (120, 100),
            Some(20),
            (200, 100),
            Some(20),
            1,
        ),
        (
            json!({"deviceScaleFactor": 2, "viewport": viewport}),
            (240, 200),
            Some(40),
            (400, 200),
            Some(200),
            2,
        ),
        (
            json!({"scale": 0.5, "viewport": viewport}),
            (120, 100),
            Some(0),
            (200, 100),
            Some(20),
            1,
        ),
        (
            json!({"dontSetVisibleSize": true, "viewport": viewport}),
            (200, 100),
            Some(20),
            (200, 100),
            Some(20),
            1,
        ),
        (
            json!({"viewport": {"x": 30, "y": 0, "width": 0, "height": 0, "scale": 0}}),
            (200, 100),
            Some(0),
            (200, 100),
            Some(0),
            1,
        ),
        (
            json!({"viewport": {"x": 30, "y": 0, "width": 60, "height": 50, "scale": -1}}),
            (200, 100),
            Some(0),
            (200, 100),
            Some(0),
            1,
        ),
        (
            json!({"viewport": {"x": -1, "y": 0, "width": 60, "height": 50, "scale": 2}}),
            (120, 100),
            Some(40),
            (200, 100),
            Some(40),
            1,
        ),
    ];
    for (params, widget_size, widget_boundary, surface_size, surface_boundary, dpr) in cases {
        eprintln!("capture presentation: {params}");
        set_metrics(&mut ctx, json!({})).await;
        set_metrics(&mut ctx, params.clone()).await;
        let registration = start_screencast(&mut ctx, json!({}));
        let completion = poll_screencast(&mut ctx, &registration, None).await;
        let (frame, _) = take_frame(&mut ctx, &registration, completion);
        let image = png(&frame["data"]);
        assert_blue_boundary(&image, widget_size, widget_boundary);
        assert_eq!(
            frame["metadata"]["deviceWidth"].as_f64(),
            Some(f64::from(widget_size.0))
        );
        assert_eq!(
            frame["metadata"]["deviceHeight"].as_f64(),
            Some(f64::from(widget_size.1))
        );
        if params == json!({"scale": 0.5}) {
            assert_eq!(
                decode_png_pixel(&image, 150, 80),
                [0, 0, 255, 255],
                "the propagated solid canvas color fills the compositor surface"
            );
        }
        command(&mut ctx, "Page.stopScreencast", json!({})).await;

        let screenshot = command(&mut ctx, "Page.captureScreenshot", json!({})).await;
        assert_blue_boundary(&png(&screenshot["data"]), surface_size, surface_boundary);
        let clipped = command(
            &mut ctx,
            "Page.captureScreenshot",
            json!({
                "clip": {"x": 10, "y": 0, "width": 60, "height": 30, "scale": 1}
            }),
        )
        .await;
        assert_blue_boundary(&png(&clipped["data"]), (60 * dpr, 30 * dpr), Some(30 * dpr));
        let layout = command(
            &mut ctx,
            "Runtime.evaluate",
            json!({
                "expression": "[innerWidth, innerHeight, devicePixelRatio, visualViewport.scale]",
                "returnByValue": true
            }),
        )
        .await;
        assert_eq!(
            layout["result"]["value"],
            json!([200, 100, dpr, 1]),
            "{params}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn emulated_view_capture_invalidates_frames_survives_navigation_and_clears() {
    let mut ctx = TestContext::new();
    install_page(&mut ctx).await;
    let registration = start_screencast(&mut ctx, json!({"maxWidth": 100, "maxHeight": 50}));
    let completion = poll_screencast(&mut ctx, &registration, None).await;
    let (_, original) = take_frame(&mut ctx, &registration, completion);

    set_metrics(&mut ctx, json!({"scale": 0.5})).await;
    let completion = poll_screencast(&mut ctx, &registration, Some(original.clone())).await;
    let (frame, scaled) = take_frame(&mut ctx, &registration, completion);
    assert_ne!(original, scaled);
    assert_blue_boundary(&png(&frame["data"]), (100, 50), Some(10));
    assert_eq!(frame["metadata"]["deviceWidth"].as_f64(), Some(200.0));
    assert_eq!(frame["metadata"]["deviceHeight"].as_f64(), Some(100.0));
    assert!(matches!(
        poll_screencast(&mut ctx, &registration, Some(scaled)).await,
        PageScreencastCaptureCompletion::Unchanged
    ));

    ctx.install_navigation_fixture_for_session_owner(&screenshot_data_url(HTML), Some(SESSION))
        .await;
    let completion = poll_screencast(&mut ctx, &registration, None).await;
    let (frame, _) = take_frame(&mut ctx, &registration, completion);
    assert_blue_boundary(&png(&frame["data"]), (100, 50), Some(10));

    command(&mut ctx, "Emulation.clearDeviceMetricsOverride", json!({})).await;
    let layout = command(
        &mut ctx,
        "Runtime.evaluate",
        json!({
            "expression": "[innerWidth, innerHeight]", "returnByValue": true
        }),
    )
    .await;
    let completion = poll_screencast(&mut ctx, &registration, None).await;
    let (frame, _) = take_frame(&mut ctx, &registration, completion);
    assert_eq!(
        frame["metadata"]["deviceWidth"].as_f64(),
        layout["result"]["value"][0].as_f64()
    );
    assert_eq!(
        frame["metadata"]["deviceHeight"].as_f64(),
        layout["result"]["value"][1].as_f64()
    );
    let image = moli_image::decode_png(&png(&frame["data"])).unwrap();
    assert!(image.width <= 100 && image.height <= 50);
}

#[tokio::test(flavor = "multi_thread")]
async fn emulated_view_capture_resolves_zero_dimensions_and_scroll_at_layout() {
    let mut ctx = TestContext::new();
    install_page(&mut ctx).await;
    set_metrics(
        &mut ctx,
        json!({
            "width": 0, "height": 0, "scale": 2,
            "viewport": {"x": 30, "y": 0, "width": 60, "height": 50, "scale": 2}
        }),
    )
    .await;
    let layout = command(
        &mut ctx,
        "Runtime.evaluate",
        json!({
            "expression": "[innerWidth, innerHeight]", "returnByValue": true
        }),
    )
    .await;
    assert_eq!(layout["result"]["value"], json!([60, 50]));
    let screenshot = command(&mut ctx, "Page.captureScreenshot", json!({})).await;
    assert_blue_boundary(&png(&screenshot["data"]), (60, 50), Some(20));

    ctx.install_navigation_fixture_for_session_owner(
        &screenshot_data_url(&format!("{HTML}<style>body {{ height: 500px }}</style>")),
        Some(SESSION),
    )
    .await;
    set_metrics(
        &mut ctx,
        json!({
            "viewport": {"x": 0, "y": 30, "width": 60, "height": 50, "scale": 1}
        }),
    )
    .await;
    command(&mut ctx, "Page.captureScreenshot", json!({})).await;
    let scroll = command(
        &mut ctx,
        "Runtime.evaluate",
        json!({
            "expression": "scrollTo(0, 50); new Promise(requestAnimationFrame).then(() => scrollY)",
            "returnByValue": true, "awaitPromise": true
        }),
    )
    .await;
    assert_eq!(scroll["result"]["value"], 50);
    command(
        &mut ctx,
        "Emulation.setDefaultBackgroundColorOverride",
        json!({
            "color": {"r": 0, "g": 0, "b": 0, "a": 0}
        }),
    )
    .await;
    let registration = start_screencast(&mut ctx, json!({}));
    let completion = poll_screencast(&mut ctx, &registration, None).await;
    let (frame, _) = take_frame(&mut ctx, &registration, completion);
    let image = png(&frame["data"]);
    assert_png_dimensions(&image, 60, 50);
    assert_eq!(decode_png_pixel(&image, 10, 0), [0, 0, 255, 255]);
    assert_eq!(decode_png_pixel(&image, 10, 25), [255, 0, 0, 255]);
    assert_eq!(decode_png_pixel(&image, 50, 25), [0, 0, 255, 255]);
}

#[tokio::test(flavor = "multi_thread")]
async fn emulated_view_capture_clips_background_images_over_a_transparent_base() {
    let mut ctx = TestContext::new();
    install_page(&mut ctx).await;
    let gradient =
        screenshot_data_url("<style>html{background:linear-gradient(blue,blue)}</style>");
    ctx.install_navigation_fixture_for_session_owner(&gradient, Some(SESSION))
        .await;
    command(
        &mut ctx,
        "Emulation.setDefaultBackgroundColorOverride",
        json!({
            "color": {"r": 0, "g": 0, "b": 0, "a": 0}
        }),
    )
    .await;
    set_metrics(&mut ctx, json!({"scale": 0.5})).await;
    let registration = start_screencast(&mut ctx, json!({}));
    let completion = poll_screencast(&mut ctx, &registration, None).await;
    let (frame, _) = take_frame(&mut ctx, &registration, completion);
    let image = png(&frame["data"]);
    assert_png_dimensions(&image, 200, 100);
    assert_eq!(decode_png_pixel(&image, 10, 10), [0, 0, 255, 255]);
    assert_eq!(decode_png_pixel(&image, 150, 80), [0; 4]);

    set_metrics(
        &mut ctx,
        json!({
            "viewport": {"x": 0, "y": 0, "width": 0, "height": 0, "scale": 0}
        }),
    )
    .await;
    let completion = poll_screencast(&mut ctx, &registration, None).await;
    let (frame, _) = take_frame(&mut ctx, &registration, completion);
    let image = moli_image::decode_png(&png(&frame["data"])).unwrap();
    assert!(image.rgba.iter().all(|byte| *byte == 0));
}

// 2026-11-17: a phone screenshot showed the left 390px of the page unscaled; Chrome draws the whole 980px layout of a page without a viewport meta tag scaled into the 390px screen.
#[tokio::test(flavor = "multi_thread")]
async fn a_phone_screenshot_scales_the_980px_layout_into_the_screen() {
    let mut ctx = TestContext::new_with_layout_policy(moli_core::LayoutPolicy::Live);
    install_active_screenshot_page(
        &mut ctx,
        "BID-VIEW-CAPTURE",
        "TID-VIEW-CAPTURE",
        SESSION,
        &screenshot_data_url(
            r#"<!doctype html><style>
                html { background: blue; scrollbar-width: none }
                body { margin: 0 }
                #stripe { width: 490px; height: 100px; background: red }
            </style><div id="stripe"></div>"#,
        ),
    )
    .await;
    set_metrics(
        &mut ctx,
        json!({"width": 390, "height": 844, "mobile": true}),
    )
    .await;

    let screenshot = command(&mut ctx, "Page.captureScreenshot", json!({})).await;
    let image = png(&screenshot["data"]);
    assert_png_dimensions(&image, 390, 844);
    // At scale 390/980 the 490x100 stripe covers 195x40 screen pixels.
    assert_eq!(decode_png_pixel(&image, 150, 20), [255, 0, 0, 255]);
    assert_eq!(decode_png_pixel(&image, 250, 20), [0, 0, 255, 255]);
    assert_eq!(decode_png_pixel(&image, 150, 60), [0, 0, 255, 255]);

    // The capture laid the page out at its own 980px layout viewport, so the
    // geometry it published is the one scripts read.
    let layout = command(
        &mut ctx,
        "Runtime.evaluate",
        json!({
            "expression": "[innerWidth, document.documentElement.clientWidth, document.getElementById('stripe').getBoundingClientRect().width]",
            "returnByValue": true
        }),
    )
    .await;
    assert_eq!(layout["result"]["value"], json!([980, 980, 490]));
}

/// Playwright's `page.screenshot()` of a `viewport`-sized viewport (crPage
/// `takeScreenshot`): a clip of the visual viewport built from
/// `Page.getLayoutMetrics`, scaled by its `scale`, inside the viewport.
async fn playwright_viewport_screenshot(
    ctx: &mut TestContext,
    viewport: (f64, f64),
) -> (Value, Vec<u8>) {
    let metrics = command(ctx, "Page.getLayoutMetrics", json!({})).await;
    let visual = &metrics["visualViewport"];
    let scale = visual["scale"].as_f64().expect("visual viewport scale");
    let clip = json!({
        "x": visual["pageX"],
        "y": visual["pageY"],
        "width": (viewport.0 / scale + 1e-3).floor(),
        "height": (viewport.1 / scale + 1e-3).floor(),
        "scale": scale,
    });
    let screenshot = command(
        ctx,
        "Page.captureScreenshot",
        json!({"clip": clip, "captureBeyondViewport": false}),
    )
    .await;
    (metrics, png(&screenshot["data"]))
}

fn png_size(bytes: &[u8]) -> (u32, u32) {
    (
        u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
        u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
    )
}

// 2026-11-17: Page.getLayoutMetrics reported the device pixel ratio as the visual viewport's scale, so Playwright's screenshot clip at DPR 2 showed the top-left quarter of the page; Chrome reports scale 1 on a desktop.
#[tokio::test(flavor = "multi_thread")]
async fn a_dpr_2_screenshot_through_playwrights_clip_shows_the_whole_viewport() {
    let mut ctx = TestContext::new();
    install_active_screenshot_page(
        &mut ctx,
        "BID-VIEW-CAPTURE",
        "TID-VIEW-CAPTURE",
        SESSION,
        &screenshot_data_url(
            r#"<!doctype html><style>
                html { background: blue; scrollbar-width: none }
                body { margin: 0 }
                #corner {
                    position: absolute; left: 1180px; top: 620px;
                    width: 100px; height: 100px; background: red
                }
            </style><div id="corner"></div>"#,
        ),
    )
    .await;
    set_metrics(
        &mut ctx,
        json!({"width": 1280, "height": 720, "deviceScaleFactor": 2}),
    )
    .await;

    let (metrics, image) = playwright_viewport_screenshot(&mut ctx, (1280.0, 720.0)).await;
    for viewport in ["visualViewport", "cssVisualViewport"] {
        assert_eq!(metrics[viewport]["scale"], 1.0, "{metrics}");
        assert_eq!(metrics[viewport]["clientWidth"], 1280.0, "{metrics}");
        assert_eq!(metrics[viewport]["clientHeight"], 720.0, "{metrics}");
    }
    assert_eq!(
        metrics["cssLayoutViewport"]["clientWidth"], 1280,
        "{metrics}"
    );
    assert_eq!(
        metrics["cssLayoutViewport"]["clientHeight"], 720,
        "{metrics}"
    );
    assert_png_dimensions(&image, 2560, 1440);
    // The red square in the viewport's bottom-right corner is in the picture.
    assert_eq!(decode_png_pixel(&image, 2500, 1400), [255, 0, 0, 255]);
    assert_eq!(decode_png_pixel(&image, 100, 100), [0, 0, 255, 255]);
}

// 2026-11-17: Page.getLayoutMetrics reported the device pixel ratio as a phone's visual viewport scale; Chrome reports the page scale (390/980 without a viewport meta tag), so Playwright's clip takes the whole 980px layout at the screen's pixel size.
#[tokio::test(flavor = "multi_thread")]
async fn a_phone_screenshot_through_playwrights_clip_is_the_whole_layout_at_the_screen_size() {
    let mut ctx = TestContext::new_with_layout_policy(moli_core::LayoutPolicy::Live);
    install_active_screenshot_page(
        &mut ctx,
        "BID-VIEW-CAPTURE",
        "TID-VIEW-CAPTURE",
        SESSION,
        &screenshot_data_url(
            r#"<!doctype html><style>
                html { background: blue; scrollbar-width: none }
                body { margin: 0 }
                #stripe { width: 490px; height: 100px; background: red }
            </style><div id="stripe"></div>"#,
        ),
    )
    .await;
    set_metrics(
        &mut ctx,
        json!({"width": 390, "height": 844, "deviceScaleFactor": 3, "mobile": true}),
    )
    .await;

    let (metrics, image) = playwright_viewport_screenshot(&mut ctx, (390.0, 844.0)).await;
    // Chromium: scale 0.398, visual viewport 980 x 2120.8, layout viewport and
    // content 980 x 2121.
    for viewport in ["visualViewport", "cssVisualViewport"] {
        let visual = &metrics[viewport];
        let scale = visual["scale"].as_f64().unwrap_or_default();
        assert!((scale - 390.0 / 980.0).abs() < 1e-6, "{metrics}");
        assert_eq!(visual["clientWidth"], 980.0, "{metrics}");
        let height = visual["clientHeight"].as_f64().unwrap_or_default();
        assert!((height - 844.0 * 980.0 / 390.0).abs() < 1e-6, "{metrics}");
    }
    for layout in ["layoutViewport", "cssLayoutViewport"] {
        assert_eq!(metrics[layout]["clientWidth"], 980, "{metrics}");
        assert_eq!(metrics[layout]["clientHeight"], 2121, "{metrics}");
    }
    assert_eq!(metrics["cssContentSize"]["width"], 980.0, "{metrics}");
    assert_eq!(metrics["cssContentSize"]["height"], 2121.0, "{metrics}");

    // Chromium's PNG is 1170x2531: the 980x2120 clip at 3 * 390 / 980.
    let (width, height) = png_size(&image);
    assert!(
        width.abs_diff(1170) <= 1 && height.abs_diff(2531) <= 2,
        "screenshot {width}x{height}"
    );
    // The 490px stripe is half the layout: about 585 of the 1170 pixels.
    assert_eq!(decode_png_pixel(&image, 500, 20), [255, 0, 0, 255]);
    assert_eq!(decode_png_pixel(&image, 700, 20), [0, 0, 255, 255]);
    assert_eq!(decode_png_pixel(&image, 500, 200), [0, 0, 255, 255]);
}

// 2026-11-18: Page.screencastFrame metadata hard-coded pageScaleFactor 1.0; on a phone Chrome reports the page scale its viewport-meta layout is shown at (390/980 for a page without a tag on a 390px screen), and 1 on a desktop at any device pixel ratio.
#[tokio::test(flavor = "multi_thread")]
async fn a_phone_screencast_frame_carries_the_page_scale() {
    let mut ctx = TestContext::new_with_layout_policy(moli_core::LayoutPolicy::Live);
    install_active_screenshot_page(
        &mut ctx,
        "BID-VIEW-CAPTURE",
        "TID-VIEW-CAPTURE",
        SESSION,
        &screenshot_data_url(HTML),
    )
    .await;
    set_metrics(
        &mut ctx,
        json!({"width": 390, "height": 844, "deviceScaleFactor": 3, "mobile": true}),
    )
    .await;
    let registration = start_screencast(&mut ctx, json!({}));
    let completion = poll_screencast(&mut ctx, &registration, None).await;
    let (frame, _) = take_frame(&mut ctx, &registration, completion);
    let metadata = &frame["metadata"];
    let scale = metadata["pageScaleFactor"]
        .as_f64()
        .expect("pageScaleFactor");
    assert!((scale - 390.0 / 980.0).abs() < 1e-3, "{metadata}");
    assert_eq!(metadata["deviceWidth"].as_f64(), Some(390.0), "{metadata}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_desktop_screencast_frame_at_dpr_2_carries_page_scale_1() {
    let mut ctx = TestContext::new_with_layout_policy(moli_core::LayoutPolicy::Live);
    install_active_screenshot_page(
        &mut ctx,
        "BID-VIEW-CAPTURE",
        "TID-VIEW-CAPTURE",
        SESSION,
        &screenshot_data_url(HTML),
    )
    .await;
    set_metrics(&mut ctx, json!({"deviceScaleFactor": 2})).await;
    let registration = start_screencast(&mut ctx, json!({}));
    let completion = poll_screencast(&mut ctx, &registration, None).await;
    let (frame, _) = take_frame(&mut ctx, &registration, completion);
    let metadata = &frame["metadata"];
    assert_eq!(
        metadata["pageScaleFactor"].as_f64(),
        Some(1.0),
        "{metadata}"
    );
}
