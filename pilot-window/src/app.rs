//! The window itself: winit and softbuffer on the main thread, the
//! controller on the tokio runtime.

use std::{
    num::NonZeroU32,
    rc::Rc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use moli_protocol_server::LocalCdpConnector;
use tokio::sync::mpsc;
use vello_cpu::{
    Pixmap, RenderContext, Resources,
    color::{AlphaColor, Srgb},
    kurbo::{BezPath, Rect as Box2, RoundedRect, Shape, Stroke},
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton as WinitButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    keyboard::{Key as WinitKey, NamedKey as WinitNamedKey},
    window::{Window, WindowId},
};

use crate::{
    controller::{Command, Update, run_controller},
    frame::Frame,
    geometry::{ChromeLayout, FrameMetadata, Rect, letterbox, window_to_page},
    input::{self, Key, Modifiers, MouseButton, NamedKey},
    tabs::Tab,
    text::TextRenderer,
};

const BACKDROP: u32 = 0x202124;
const DOUBLE_CLICK: Duration = Duration::from_millis(500);
const WHEEL_LINE: f64 = 40.0;
const LABEL_SIZE: f32 = 13.0;

fn rgb(red: u8, green: u8, blue: u8) -> AlphaColor<Srgb> {
    AlphaColor::from_rgb8(red, green, blue)
}

enum AppEvent {
    Update(Update),
    BrowserStopped,
}

/// Opens the window on this thread (the main thread on macOS) and runs it
/// until the browser stops. The controller runs on `runtime`. Fails, without
/// starting anything, when no window can be opened (no display).
pub fn run_window(connector: LocalCdpConnector, runtime: tokio::runtime::Handle) -> Result<()> {
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .context("cannot open a window")?;
    let proxy = event_loop.create_proxy();
    let (commands, command_rx) = mpsc::unbounded_channel();
    spawn_controller(&runtime, connector, command_rx, proxy);
    let mut app = App::new(commands);
    event_loop
        .run_app(&mut app)
        .context("window event loop failed")
}

fn spawn_controller(
    runtime: &tokio::runtime::Handle,
    connector: LocalCdpConnector,
    command_rx: mpsc::UnboundedReceiver<Command>,
    proxy: EventLoopProxy<AppEvent>,
) {
    let updates = proxy.clone();
    runtime.spawn(async move {
        let result = run_controller(connector, command_rx, move |update| {
            let _ = updates.send_event(AppEvent::Update(update));
        })
        .await;
        if let Err(error) = result {
            tracing::error!(error = %format!("{error:#}"), "❌ window controller failed");
        }
        let _ = proxy.send_event(AppEvent::BrowserStopped);
    });
}

struct Surface {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
}

struct App {
    commands: mpsc::UnboundedSender<Command>,
    surface: Option<Surface>,
    text: Option<TextRenderer>,
    tabs: Vec<Tab>,
    active: Option<usize>,
    frame: Option<(Frame, FrameMetadata)>,
    agent_active: bool,
    /// The URL bar's text while it has focus.
    url_edit: Option<String>,
    cursor: (f64, f64),
    modifiers: Modifiers,
    buttons: u32,
    last_press: Option<(Instant, (f64, f64), u32)>,
}

impl App {
    fn new(commands: mpsc::UnboundedSender<Command>) -> Self {
        Self {
            commands,
            surface: None,
            text: None,
            tabs: Vec::new(),
            active: None,
            frame: None,
            agent_active: false,
            url_edit: None,
            cursor: (0.0, 0.0),
            modifiers: Modifiers::default(),
            buttons: 0,
            last_press: None,
        }
    }

    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    fn layout(&self) -> Option<ChromeLayout> {
        let window = &self.surface.as_ref()?.window;
        let size = window.inner_size();
        Some(ChromeLayout::new(
            size.width,
            size.height,
            window.scale_factor(),
        ))
    }

    fn request_redraw(&self) {
        if let Some(surface) = &self.surface {
            surface.window.request_redraw();
        }
    }

    fn shown_frame(&self, layout: &ChromeLayout) -> Option<(Rect, FrameMetadata)> {
        let (frame, metadata) = self.frame.as_ref()?;
        Some((
            letterbox(frame.width, frame.height, layout.content),
            *metadata,
        ))
    }

    /// The page CSS pixel under a window point, when it is on the page.
    fn page_point(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let layout = self.layout()?;
        let (shown, metadata) = self.shown_frame(&layout)?;
        window_to_page(x, y, shown, metadata)
    }

    fn active_url(&self) -> String {
        self.active
            .and_then(|index| self.tabs.get(index))
            .map(|tab| tab.url.clone())
            .unwrap_or_default()
    }

    fn update(&mut self, update: Update) {
        match update {
            Update::Tabs { tabs, active } => {
                self.tabs = tabs;
                self.active = active;
                if let (Some(surface), Some(tab)) =
                    (&self.surface, active.and_then(|index| self.tabs.get(index)))
                {
                    let title = if tab.title.is_empty() {
                        &tab.url
                    } else {
                        &tab.title
                    };
                    surface.window.set_title(&format!("{title} — pilot"));
                }
            }
            Update::Frame { frame, metadata } => self.frame = Some((frame, metadata)),
            Update::AgentActive(active) => self.agent_active = active,
        }
        self.request_redraw();
    }

    fn mouse_button(&mut self, state: ElementState, button: WinitButton) {
        let button = match button {
            WinitButton::Left => MouseButton::Left,
            WinitButton::Right => MouseButton::Right,
            WinitButton::Middle => MouseButton::Middle,
            _ => return,
        };
        let (x, y) = self.cursor;
        let pressed = state == ElementState::Pressed;
        let Some(layout) = self.layout() else { return };
        if pressed && !layout.content.contains(x, y) {
            if button == MouseButton::Left {
                self.click_chrome(&layout, x, y);
            }
            return;
        }
        let Some((page_x, page_y)) = self.page_point(x, y) else {
            return;
        };
        if pressed {
            self.url_edit = None;
            self.buttons |= button.mask();
        } else {
            self.buttons &= !button.mask();
        }
        let click_count = if pressed {
            self.click_count(x, y)
        } else {
            self.last_press.map_or(1, |press| press.2)
        };
        self.send(Command::Mouse(input::mouse_button(
            pressed,
            page_x,
            page_y,
            button,
            self.buttons,
            click_count,
            self.modifiers,
        )));
    }

    /// 1 for a click, 2 for a double click, 3 for a triple click.
    fn click_count(&mut self, x: f64, y: f64) -> u32 {
        let now = Instant::now();
        let count = match self.last_press {
            Some((at, (last_x, last_y), count))
                if now.duration_since(at) < DOUBLE_CLICK
                    && (x - last_x).abs() < 4.0
                    && (y - last_y).abs() < 4.0 =>
            {
                (count % 3) + 1
            }
            _ => 1,
        };
        self.last_press = Some((now, (x, y), count));
        count
    }

    fn click_chrome(&mut self, layout: &ChromeLayout, x: f64, y: f64) {
        if let Some(index) =
            (0..self.tabs.len()).find(|&index| layout.tab(index, self.tabs.len()).contains(x, y))
        {
            self.url_edit = None;
            self.send(Command::ActivateTab(index));
        } else if layout.back.contains(x, y) {
            self.send(Command::Back);
        } else if layout.forward.contains(x, y) {
            self.send(Command::Forward);
        } else if layout.reload.contains(x, y) {
            self.send(Command::Reload);
        } else if layout.url_bar.contains(x, y) {
            self.url_edit = Some(self.active_url());
        } else {
            self.url_edit = None;
        }
        self.request_redraw();
    }

    fn wheel(&self, delta: MouseScrollDelta) {
        let (x, y) = self.cursor;
        let Some((page_x, page_y)) = self.page_point(x, y) else {
            return;
        };
        let scale = self
            .surface
            .as_ref()
            .map_or(1.0, |surface| surface.window.scale_factor());
        // winit's positive y scrolls toward the top; CDP's positive deltaY
        // scrolls down.
        let (delta_x, delta_y) = match delta {
            MouseScrollDelta::LineDelta(x, y) => {
                (-f64::from(x) * WHEEL_LINE, -f64::from(y) * WHEEL_LINE)
            }
            MouseScrollDelta::PixelDelta(position) => (-position.x / scale, -position.y / scale),
        };
        self.send(Command::Mouse(input::mouse_wheel(
            page_x,
            page_y,
            delta_x,
            delta_y,
            self.modifiers,
        )));
    }

    fn key(&mut self, logical_key: &WinitKey, pressed: bool) {
        if let Some(edit) = &mut self.url_edit {
            if pressed {
                match logical_key {
                    WinitKey::Named(WinitNamedKey::Enter) => {
                        let typed = std::mem::take(edit);
                        self.url_edit = None;
                        if let Some(url) = input::url_from_bar(&typed) {
                            self.send(Command::Navigate(url));
                        }
                    }
                    WinitKey::Named(WinitNamedKey::Escape) => self.url_edit = None,
                    WinitKey::Named(WinitNamedKey::Backspace) => {
                        edit.pop();
                    }
                    WinitKey::Named(WinitNamedKey::Space) => edit.push(' '),
                    WinitKey::Character(text) if !self.modifiers.ctrl && !self.modifiers.meta => {
                        edit.push_str(text);
                    }
                    _ => {}
                }
                self.request_redraw();
            }
            return;
        }
        let key = match logical_key {
            WinitKey::Character(text) => Key::Character(text.as_str()),
            WinitKey::Named(named) => match named_key(*named) {
                Some(named) => Key::Named(named),
                None => return,
            },
            _ => return,
        };
        self.send(Command::Key(input::key(key, pressed, self.modifiers)));
    }

    fn resized(&mut self) {
        let Some(surface) = &mut self.surface else {
            return;
        };
        let size = surface.window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        if let Err(error) = surface.surface.resize(width, height) {
            tracing::warn!(%error, "⚠️ could not resize the window surface");
        }
        let layout = ChromeLayout::new(size.width, size.height, surface.window.scale_factor());
        self.send(Command::SetFrameLimit {
            width: layout.content.width.round() as u32,
            height: layout.content.height.round() as u32,
        });
        self.request_redraw();
    }

    fn draw(&mut self) {
        let Some(layout) = self.layout() else { return };
        let chrome = self.paint_chrome(&layout);
        let shown = self.shown_frame(&layout).map(|(shown, _)| shown);
        let Some(surface) = &mut self.surface else {
            return;
        };
        let size = surface.window.inner_size();
        let (width, height) = (size.width as usize, size.height as usize);
        let mut buffer = match surface.surface.buffer_mut() {
            Ok(buffer) => buffer,
            Err(error) => {
                tracing::warn!(%error, "⚠️ could not get the window buffer");
                return;
            }
        };
        if buffer.len() != width * height {
            return;
        }
        buffer.fill(BACKDROP);
        if let (Some((frame, _)), Some(shown)) = (&self.frame, shown) {
            frame.blit(&mut buffer, width, height, shown);
        }
        if let Some(chrome) = chrome {
            let chrome_width = usize::from(chrome.width());
            for (index, pixel) in chrome.data().iter().enumerate() {
                let (x, y) = (index % chrome_width, index / chrome_width);
                if x < width && y < height {
                    buffer[y * width + x] =
                        u32::from(pixel.r) << 16 | u32::from(pixel.g) << 8 | u32::from(pixel.b);
                }
            }
        }
        if let Err(error) = buffer.present() {
            tracing::warn!(%error, "⚠️ could not present the window buffer");
        }
    }

    /// The tab strip and toolbar, as an opaque pixmap as wide as the window.
    fn paint_chrome(&self, layout: &ChromeLayout) -> Option<Pixmap> {
        let width = u16::try_from(layout.tab_strip.width.round() as u32).ok()?;
        let height = u16::try_from(layout.content.y.round() as u32).ok()?;
        if width == 0 || height == 0 {
            return None;
        }
        let scale = layout.scale();
        let label = LABEL_SIZE * scale as f32;
        let mut context = RenderContext::new(width, height);
        context.set_paint(rgb(222, 225, 230));
        context.fill_rect(&Box2::new(0.0, 0.0, f64::from(width), f64::from(height)));
        context.set_paint(rgb(255, 255, 255));
        context.fill_rect(&Box2::new(
            0.0,
            layout.tab_strip.bottom(),
            f64::from(width),
            f64::from(height),
        ));

        for (index, tab) in self.tabs.iter().enumerate() {
            let area = layout.tab(index, self.tabs.len());
            let active = Some(index) == self.active;
            if active {
                context.set_paint(rgb(255, 255, 255));
                context.fill_path(&rounded(area, 6.0 * scale, 2.0 * scale).to_path(0.1));
            }
            if let Some(text) = &self.text {
                let title = if tab.title.is_empty() {
                    &tab.url
                } else {
                    &tab.title
                };
                let (title, title_color) = if title.is_empty() {
                    ("New tab", rgb(95, 99, 104))
                } else if active {
                    (title.as_str(), rgb(32, 33, 36))
                } else {
                    (title.as_str(), rgb(95, 99, 104))
                };
                context.set_paint(title_color);
                let baseline = area.y + (area.height + text.ascent(label) * 0.7) / 2.0;
                text.draw(
                    &mut context,
                    title,
                    area.x + 12.0 * scale,
                    baseline,
                    label,
                    area.width - 24.0 * scale,
                );
            }
        }

        context.set_stroke(Stroke::new(2.0 * scale));
        context.set_paint(rgb(95, 99, 104));
        context.stroke_path(&chevron(layout.back, -1.0));
        context.stroke_path(&chevron(layout.forward, 1.0));
        context.stroke_path(&reload_arrow(layout.reload));

        context.set_paint(rgb(241, 243, 244));
        context.fill_path(&rounded(layout.url_bar, layout.url_bar.height / 2.0, 0.0).to_path(0.1));
        if let Some(text) = &self.text {
            let (shown, color) = match &self.url_edit {
                Some(edit) => (format!("{edit}|"), rgb(32, 33, 36)),
                None => (self.active_url(), rgb(60, 64, 67)),
            };
            context.set_paint(color);
            let baseline =
                layout.url_bar.y + (layout.url_bar.height + text.ascent(label) * 0.7) / 2.0;
            text.draw(
                &mut context,
                &shown,
                layout.url_bar.x + 14.0 * scale,
                baseline,
                label,
                layout.url_bar.width - 28.0 * scale,
            );

            let (badge, badge_text, badge_color) = if self.agent_active {
                (rgb(26, 115, 232), "● Agent active", rgb(255, 255, 255))
            } else {
                (rgb(241, 243, 244), "Agent idle", rgb(95, 99, 104))
            };
            context.set_paint(badge);
            context.fill_path(
                &rounded(layout.agent_badge, layout.agent_badge.height / 2.0, 0.0).to_path(0.1),
            );
            context.set_paint(badge_color);
            let badge_baseline =
                layout.agent_badge.y + (layout.agent_badge.height + text.ascent(label) * 0.7) / 2.0;
            text.draw(
                &mut context,
                badge_text,
                layout.agent_badge.x + 12.0 * scale,
                badge_baseline,
                label,
                layout.agent_badge.width - 24.0 * scale,
            );
        }

        let mut pixmap = Pixmap::new(width, height);
        context.flush();
        context.render(&mut pixmap, &mut Resources::new());
        Some(pixmap)
    }
}

fn rounded(area: Rect, radius: f64, inset: f64) -> RoundedRect {
    RoundedRect::new(
        area.x + inset,
        area.y + inset,
        area.right() - inset,
        area.bottom() + radius,
        radius,
    )
}

/// `<` (direction -1) or `>` (direction 1) centered in `area`.
fn chevron(area: Rect, direction: f64) -> BezPath {
    let (center_x, center_y) = (area.x + area.width / 2.0, area.y + area.height / 2.0);
    let size = area.width * 0.18;
    let mut path = BezPath::new();
    path.move_to((center_x - direction * size / 2.0, center_y - size));
    path.line_to((center_x + direction * size / 2.0, center_y));
    path.line_to((center_x - direction * size / 2.0, center_y + size));
    path
}

/// An open circle with an arrowhead, centered in `area`.
fn reload_arrow(area: Rect) -> BezPath {
    let (center_x, center_y) = (area.x + area.width / 2.0, area.y + area.height / 2.0);
    let radius = area.width * 0.22;
    let mut path = vello_cpu::kurbo::Arc::new(
        (center_x, center_y),
        (radius, radius),
        -std::f64::consts::FRAC_PI_4,
        std::f64::consts::PI * 1.6,
        0.0,
    )
    .into_path(0.1);
    let tip = (center_x + radius * 0.71, center_y - radius * 0.71);
    path.move_to((tip.0 - radius * 0.5, tip.1));
    path.line_to(tip);
    path.line_to((tip.0, tip.1 + radius * 0.5));
    path
}

fn named_key(key: WinitNamedKey) -> Option<NamedKey> {
    Some(match key {
        WinitNamedKey::Enter => NamedKey::Enter,
        WinitNamedKey::Backspace => NamedKey::Backspace,
        WinitNamedKey::Tab => NamedKey::Tab,
        WinitNamedKey::Escape => NamedKey::Escape,
        WinitNamedKey::Space => NamedKey::Space,
        WinitNamedKey::ArrowLeft => NamedKey::ArrowLeft,
        WinitNamedKey::ArrowUp => NamedKey::ArrowUp,
        WinitNamedKey::ArrowRight => NamedKey::ArrowRight,
        WinitNamedKey::ArrowDown => NamedKey::ArrowDown,
        WinitNamedKey::Delete => NamedKey::Delete,
        WinitNamedKey::Home => NamedKey::Home,
        WinitNamedKey::End => NamedKey::End,
        WinitNamedKey::PageUp => NamedKey::PageUp,
        WinitNamedKey::PageDown => NamedKey::PageDown,
        _ => return None,
    })
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.surface.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("pilot")
            .with_inner_size(LogicalSize::new(1280.0, 800.0));
        let opened = event_loop
            .create_window(attributes)
            .map_err(anyhow::Error::from)
            .and_then(|window| {
                let window = Rc::new(window);
                let context = softbuffer::Context::new(window.clone())
                    .map_err(|error| anyhow::anyhow!("{error}"))?;
                let surface = softbuffer::Surface::new(&context, window.clone())
                    .map_err(|error| anyhow::anyhow!("{error}"))?;
                Ok(Surface { window, surface })
            });
        match opened {
            Ok(surface) => {
                tracing::info!("✅ watch-the-agent window open");
                self.surface = Some(surface);
                self.text = TextRenderer::system();
                self.resized();
            }
            Err(error) => {
                tracing::warn!(error = %format!("{error:#}"), "⚠️ could not open the window; the browser keeps running headless");
                self.send(Command::Detach);
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Update(update) => self.update(update),
            AppEvent::BrowserStopped => event_loop.exit(),
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                if let Some(surface) = &self.surface {
                    surface.window.set_visible(false);
                }
                self.send(Command::Close);
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => self.resized(),
            WindowEvent::Occluded(occluded) => self.send(Command::SetVisible(!occluded)),
            WindowEvent::RedrawRequested => self.draw(),
            WindowEvent::ModifiersChanged(modifiers) => {
                let state = modifiers.state();
                self.modifiers = Modifiers {
                    alt: state.alt_key(),
                    ctrl: state.control_key(),
                    meta: state.super_key(),
                    shift: state.shift_key(),
                };
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
                if let Some((x, y)) = self.page_point(position.x, position.y) {
                    self.send(Command::Mouse(input::mouse_moved(
                        x,
                        y,
                        self.buttons,
                        self.modifiers,
                    )));
                }
            }
            WindowEvent::MouseInput { state, button, .. } => self.mouse_button(state, button),
            WindowEvent::MouseWheel { delta, .. } => self.wheel(delta),
            WindowEvent::KeyboardInput { event, .. } => {
                self.key(&event.logical_key, event.state == ElementState::Pressed);
            }
            _ => {}
        }
    }
}
