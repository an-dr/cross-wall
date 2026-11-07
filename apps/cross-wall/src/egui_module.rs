//! Drives one `egui::Context` from raw platform events and draws it through
//! the public `draw-target` service (docs/adr/ADR-001). Knows nothing about
//! the screens: those arrive through [`App`].
use std::time::Instant;

use bones_kernel::bus::{Envelope, Handler, Module, ModuleContext, PlatformEvent};
use bones_kernel::draw_target::{DrawTarget, DrawTargetService, UiMesh, UiVertex};
use bones_kernel::logging::Logger;
use sdl3::event::Event as E;

/// The screens, called once per frame.
pub trait App: Send {
    /// Runs before the first frame. Fonts set during a frame only apply to the next.
    fn setup(&mut self, _ctx: &egui::Context) {}
    fn ui(&mut self, ctx: &egui::Context);
    /// Physical pixels per interface point. bones reports no display scale.
    fn scale(&self) -> f32 {
        1.0
    }
}

pub struct EguiModule<A: App> {
    ctx: egui::Context,
    app: A,
    target: Option<DrawTargetService>,
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
    start: Instant,
    logger: Logger,
}

impl<A: App> EguiModule<A> {
    pub fn new(mut app: A, logger: Logger) -> Self {
        let ctx = egui::Context::default();
        app.setup(&ctx);
        Self {
            ctx,
            app,
            target: None,
            events: Vec::new(),
            modifiers: egui::Modifiers::default(),
            start: Instant::now(),
            logger,
        }
    }

    fn pos(&self, x: f32, y: f32) -> egui::Pos2 {
        egui::pos2(x, y) / self.app.scale()
    }

    /// Buffers one event for the next frame and says whether it was input.
    /// Window and quit events return false so that the kernel still acts on them.
    fn feed(&mut self, event: &E) -> bool {
        match event {
            E::MouseMotion { x, y, .. } => {
                self.events
                    .push(egui::Event::PointerMoved(self.pos(*x, *y)));
            }
            E::MouseButtonDown {
                mouse_btn, x, y, ..
            }
            | E::MouseButtonUp {
                mouse_btn, x, y, ..
            } => {
                let button = match mouse_btn {
                    sdl3::mouse::MouseButton::Left => egui::PointerButton::Primary,
                    sdl3::mouse::MouseButton::Right => egui::PointerButton::Secondary,
                    sdl3::mouse::MouseButton::Middle => egui::PointerButton::Middle,
                    _ => return false,
                };
                self.events.push(egui::Event::PointerButton {
                    pos: self.pos(*x, *y),
                    button,
                    pressed: matches!(event, E::MouseButtonDown { .. }),
                    modifiers: self.modifiers,
                });
            }
            E::MouseWheel { x, y, .. } => self.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(*x, *y),
                modifiers: self.modifiers,
            }),
            E::TextInput { text, .. } => self.events.push(egui::Event::Text(text.clone())),
            E::KeyDown {
                keycode: Some(key),
                repeat,
                keymod,
                ..
            }
            | E::KeyUp {
                keycode: Some(key),
                repeat,
                keymod,
                ..
            } => {
                self.modifiers = modifiers(*keymod);
                let Some(mapped) = map_key(*key) else {
                    return false;
                };
                let pressed = matches!(event, E::KeyDown { .. });
                if pressed && self.modifiers.ctrl {
                    match mapped {
                        egui::Key::V => {
                            if let Ok(text) =
                                arboard::Clipboard::new().and_then(|mut c| c.get_text())
                            {
                                self.events.push(egui::Event::Paste(text));
                            }
                        }
                        egui::Key::C => self.events.push(egui::Event::Copy),
                        egui::Key::X => self.events.push(egui::Event::Cut),
                        _ => {}
                    }
                }
                self.events.push(egui::Event::Key {
                    key: mapped,
                    physical_key: None,
                    pressed,
                    repeat: *repeat,
                    modifiers: self.modifiers,
                });
            }
            _ => return false,
        }
        true
    }

    fn frame(&mut self) {
        let Some((width, height)) = self.target.as_ref().map(|t| t.size()) else {
            return;
        };
        let scale = self.app.scale();
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width as f32, height as f32) / scale,
            )),
            time: Some(self.start.elapsed().as_secs_f64()),
            modifiers: self.modifiers,
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        raw.viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(scale);
        let app = &mut self.app;
        let output = self.ctx.run(raw, |ctx| app.ui(ctx));
        apply_platform_output(&output.platform_output);
        let clipped = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        let target = self.target.as_mut().expect("checked at the top of frame");
        if let Err(error) = draw(
            target.as_mut(),
            &output.textures_delta,
            &clipped,
            output.pixels_per_point,
        ) {
            self.logger.error("gui", &error);
        }
    }
}

fn apply_platform_output(output: &egui::PlatformOutput) {
    for command in &output.commands {
        match command {
            egui::OutputCommand::OpenUrl(open) => {
                let _ = open::that_detached(&open.url);
            }
            egui::OutputCommand::CopyText(text) => {
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let _ = clipboard.set_text(text);
                }
            }
            _ => {}
        }
    }
}

/// egui's Managed and User ids both start at 0, so they share one key space only when split.
fn texture_key(id: egui::TextureId) -> u64 {
    match id {
        egui::TextureId::Managed(n) => n << 1,
        egui::TextureId::User(n) => (n << 1) | 1,
    }
}

fn draw(
    target: &mut dyn DrawTarget,
    textures: &egui::TexturesDelta,
    clipped: &[egui::ClippedPrimitive],
    scale: f32,
) -> Result<(), String> {
    for (id, delta) in &textures.set {
        let egui::ImageData::Color(image) = &delta.image;
        let rgba: Vec<u8> = image
            .pixels
            .iter()
            .flat_map(|p| p.to_srgba_unmultiplied())
            .collect();
        let (w, h) = (image.size[0] as u32, image.size[1] as u32);
        match delta.pos {
            None => target.set_ui_texture(texture_key(*id), w, h, &rgba)?,
            Some([x, y]) => target.update_ui_texture_region(
                texture_key(*id),
                x as u32,
                y as u32,
                w,
                h,
                &rgba,
            )?,
        }
    }
    for primitive in clipped {
        let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive else {
            continue;
        };
        if mesh.indices.is_empty() {
            continue;
        }
        let clip = primitive.clip_rect;
        target.draw_ui_mesh(&UiMesh {
            vertices: mesh
                .vertices
                .iter()
                .map(|v| {
                    let [r, g, b, a] = v.color.to_srgba_unmultiplied();
                    UiVertex {
                        x: v.pos.x * scale,
                        y: v.pos.y * scale,
                        u: v.uv.x,
                        v: v.uv.y,
                        r,
                        g,
                        b,
                        a,
                    }
                })
                .collect(),
            indices: mesh.indices.clone(),
            texture: texture_key(mesh.texture_id),
            clip: (
                (clip.min.x * scale).round() as i32,
                (clip.min.y * scale).round() as i32,
                (clip.width() * scale).round().max(0.0) as u32,
                (clip.height() * scale).round().max(0.0) as u32,
            ),
        })?;
    }
    for id in &textures.free {
        target.free_ui_texture(texture_key(*id));
    }
    Ok(())
}

fn modifiers(keymod: sdl3::keyboard::Mod) -> egui::Modifiers {
    use sdl3::keyboard::Mod;
    let ctrl = keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD);
    egui::Modifiers {
        alt: keymod.intersects(Mod::LALTMOD | Mod::RALTMOD),
        ctrl,
        shift: keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD),
        mac_cmd: false,
        command: ctrl,
    }
}

fn map_key(key: sdl3::keyboard::Keycode) -> Option<egui::Key> {
    use sdl3::keyboard::Keycode as K;
    Some(match key {
        K::Backspace => egui::Key::Backspace,
        K::Delete => egui::Key::Delete,
        K::Return | K::Return2 | K::KpEnter => egui::Key::Enter,
        K::Escape => egui::Key::Escape,
        K::Tab => egui::Key::Tab,
        K::Left => egui::Key::ArrowLeft,
        K::Right => egui::Key::ArrowRight,
        K::Up => egui::Key::ArrowUp,
        K::Down => egui::Key::ArrowDown,
        K::Home => egui::Key::Home,
        K::End => egui::Key::End,
        K::A => egui::Key::A,
        K::C => egui::Key::C,
        K::V => egui::Key::V,
        K::X => egui::Key::X,
        K::Z => egui::Key::Z,
        _ => return None,
    })
}

impl<A: App + 'static> Module for EguiModule<A> {
    fn name(&self) -> &str {
        "gui"
    }

    fn init(&mut self, ctx: &mut ModuleContext) -> Result<(), String> {
        self.target = Some(
            ctx.consume_service::<DrawTargetService>()
                .ok_or("gui needs a draw-target service (configure .renderer() first)")?,
        );
        Ok(())
    }

    fn filter_event(&mut self, event: &PlatformEvent) -> bool {
        self.feed(event)
    }

    fn render(&mut self) {
        self.frame();
    }
}

impl<A: App> Handler for EguiModule<A> {
    fn handle(&mut self, _envelope: &Envelope) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Seen {
        textures: usize,
        meshes: usize,
        max_x: f32,
    }
    struct FakeTarget(Arc<Mutex<Seen>>);
    impl DrawTarget for FakeTarget {
        fn size(&self) -> (u32, u32) {
            (800, 600)
        }
        fn set_ui_texture(&mut self, _: u64, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
            assert_eq!(rgba.len(), (w * h * 4) as usize);
            self.0.lock().unwrap().textures += 1;
            Ok(())
        }
        fn update_ui_texture_region(
            &mut self,
            _: u64,
            _: u32,
            _: u32,
            _: u32,
            _: u32,
            _: &[u8],
        ) -> Result<(), String> {
            Ok(())
        }
        fn free_ui_texture(&mut self, _: u64) {}
        fn draw_ui_mesh(&mut self, mesh: &UiMesh) -> Result<(), String> {
            let mut seen = self.0.lock().unwrap();
            seen.meshes += 1;
            for v in &mesh.vertices {
                seen.max_x = seen.max_x.max(v.x);
            }
            Ok(())
        }
    }

    struct Clicks {
        clicked: Arc<Mutex<bool>>,
        scale: f32,
    }
    impl App for Clicks {
        fn ui(&mut self, ctx: &egui::Context) {
            egui::CentralPanel::default().show(ctx, |ui| {
                let button = ui.add_sized([ui.available_width(), 100.0], egui::Button::new("x"));
                if button.clicked() {
                    *self.clicked.lock().unwrap() = true;
                }
            });
        }
        fn scale(&self) -> f32 {
            self.scale
        }
    }

    fn module(scale: f32) -> (EguiModule<Clicks>, Arc<Mutex<Seen>>, Arc<Mutex<bool>>) {
        let seen = Arc::new(Mutex::new(Seen::default()));
        let clicked = Arc::new(Mutex::new(false));
        let mut m = EguiModule::new(
            Clicks {
                clicked: clicked.clone(),
                scale,
            },
            Logger::default(),
        );
        m.target = Some(Box::new(FakeTarget(seen.clone())));
        (m, seen, clicked)
    }

    fn step(m: &mut EguiModule<Clicks>) {
        m.frame();
    }

    #[test]
    fn a_frame_uploads_textures_and_draws() {
        let (mut m, seen, _) = module(1.0);
        step(&mut m);
        let seen = seen.lock().unwrap();
        assert!(seen.textures > 0 && seen.meshes > 0);
    }

    #[test]
    fn scale_keeps_output_in_physical_pixels() {
        let (mut m, seen, _) = module(2.0);
        step(&mut m);
        let max_x = seen.lock().unwrap().max_x;
        assert!(max_x > 700.0 && max_x <= 801.0, "{max_x}");
    }

    #[test]
    fn pointer_input_reaches_widgets() {
        let (mut m, _, clicked) = module(1.0);
        step(&mut m);
        let pos = m.pos(100.0, 40.0);
        m.events.push(egui::Event::PointerMoved(pos));
        step(&mut m);
        for pressed in [true, false] {
            m.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
            step(&mut m);
        }
        assert!(*clicked.lock().unwrap());
    }
}
