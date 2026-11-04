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
    geometry::{ChromeLayout, window_to_page},
    input::{self, Key, Modifiers, MouseButton, NamedKey},
    text::TextRenderer,
    view::View,
};

const DOUBLE_CLICK: Duration = Duration::from_millis(500);
const WHEEL_LINE: f64 = 40.0;

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
    view: View,
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
            view: View::default(),
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

    /// The page CSS pixel under a window point, when it is on the page.
    fn page_point(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let layout = self.layout()?;
        let (shown, metadata) = self.view.shown_frame(&layout)?;
        window_to_page(x, y, shown, metadata)
    }

    fn update(&mut self, update: Update) {
        let tabs_changed = matches!(update, Update::Tabs { .. });
        self.view.apply(update);
        if let (true, Some(surface), Some(tab)) =
            (tabs_changed, &self.surface, self.view.active_tab())
        {
            let title = if tab.title.is_empty() {
                &tab.url
            } else {
                &tab.title
            };
            surface.window.set_title(&format!("{title} — pilot"));
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
            self.view.url_edit = None;
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
        let count = self.view.tabs.len();
        if let Some(index) = (0..count).find(|&index| layout.tab(index, count).contains(x, y)) {
            self.view.url_edit = None;
            self.send(Command::ActivateTab(index));
        } else if layout.back.contains(x, y) {
            self.send(Command::Back);
        } else if layout.forward.contains(x, y) {
            self.send(Command::Forward);
        } else if layout.reload.contains(x, y) {
            self.send(Command::Reload);
        } else if layout.url_bar.contains(x, y) {
            self.view.url_edit = Some(self.view.active_url());
        } else {
            self.view.url_edit = None;
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
        if let Some(edit) = &mut self.view.url_edit {
            if pressed {
                match logical_key {
                    WinitKey::Named(WinitNamedKey::Enter) => {
                        let typed = std::mem::take(edit);
                        self.view.url_edit = None;
                        if let Some(url) = input::url_from_bar(&typed) {
                            self.send(Command::Navigate(url));
                        }
                    }
                    WinitKey::Named(WinitNamedKey::Escape) => self.view.url_edit = None,
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
        self.view
            .compose(&layout, self.text.as_ref(), &mut buffer, width, height);
        if let Err(error) = buffer.present() {
            tracing::warn!(%error, "⚠️ could not present the window buffer");
        }
    }
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
