use super::{PageVm, RendererLayoutMetrics};

impl PageVm {
    pub(crate) fn layout_metrics(&self) -> anyhow::Result<RendererLayoutMetrics> {
        let metrics = self.vm().document_metrics_for_current_document()?;
        let css_width = metrics.viewport.css_width;
        let css_height = metrics.viewport.css_height;
        let content_width = f64::from(metrics.content_size.width);
        let content_height = f64::from(metrics.content_size.height);
        // pilot: Chrome's visual viewport is the layout at the page scale, 1
        // on a desktop at any device pixel ratio. A phone shows its
        // viewport-meta layout scaled to the screen; its layout viewport and
        // the content around it are the screen at the minimum page scale
        // (`innerWidth` x `innerHeight`).
        let Some(mobile) = self.vm().mobile_viewport() else {
            return Ok(RendererLayoutMetrics {
                viewport_width: css_width,
                viewport_height: css_height,
                page_x: f64::from(metrics.viewport_scroll.x),
                page_y: f64::from(metrics.viewport_scroll.y),
                content_width,
                content_height,
                device_pixel_ratio: f64::from(metrics.viewport.device_pixel_ratio),
                page_scale: 1.0,
                visual_viewport_width: f64::from(css_width),
                visual_viewport_height: f64::from(css_height),
            });
        };
        Ok(RendererLayoutMetrics {
            viewport_width: mobile.frame_width,
            viewport_height: mobile.frame_height,
            page_x: f64::from(metrics.viewport_scroll.x),
            page_y: f64::from(metrics.viewport_scroll.y),
            content_width: content_width.max(f64::from(mobile.frame_width)),
            content_height: content_height.max(f64::from(mobile.frame_height)),
            device_pixel_ratio: f64::from(metrics.viewport.device_pixel_ratio),
            page_scale: mobile.page_scale,
            visual_viewport_width: mobile.visual_width,
            visual_viewport_height: mobile.visual_height,
        })
    }
}
