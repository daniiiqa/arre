//! Los avisos en pantalla: una superficie de Wayland (layer-shell, espacio de
//! nombres "arre") arriba al centro con los avisos apilados, el más nuevo
//! arriba; en la capa de encima de todo, para que se vean aunque haya algo en
//! pantalla completa. Clic izquierdo hace lo que pide el aviso (si pide algo) y lo quita,
//! clic en un botón usa ese botón, clic derecho lo quita y clic medio quita
//! todos. Con el puntero encima no se van solos.

use crate::bus::{CTL_IFACE, CTL_PATH, Ev, IFACE, PATH, Shared};
use crate::card::{Card, card};
use crate::icon::{Icon, Icons};
use crate::look::Look;
use crate::paint::Painter;
use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::reexports::calloop::channel::{Channel, Event};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A lo mucho tantos a la vez; si llegan más, se van los más viejos.
const MAX: usize = 5;
/// Separación entre avisos y con la orilla de la pantalla, en pixeles.
const GAP: u32 = 8;
const MARGIN: i32 = 10;

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

/// Uno en pantalla: cuándo se va (None: hasta que lo quiten) y dónde quedó.
struct Shown {
    id: u32,
    until: Option<Instant>,
    y: (f64, f64),
    buttons: Vec<((f64, f64, f64, f64), String)>,
}

struct App {
    registry: RegistryState,
    seat: SeatState,
    output: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    pool: Option<SlotPool>,
    layer: Option<LayerSurface>,
    configured: bool,
    size: (u32, u32),
    scale: i32,
    pointer: Option<wl_pointer::WlPointer>,
    hover: bool,
    shown: Vec<Shown>,
    shared: Arc<Shared>,
    dbus: zbus::blocking::Connection,
    look: Look,
    painter: Painter,
    icons: Icons,
    qh: QueueHandle<App>,
}

/// Cuánto dura uno que no dice: las de baja prisa menos, las críticas nunca.
fn lifetime(expire: i32, urgency: u8) -> Option<Duration> {
    match expire {
        0 => None,
        ms if ms > 0 => Some(Duration::from_millis(ms as u64)),
        _ => match urgency {
            0 => Some(Duration::from_secs(4)),
            1 => Some(Duration::from_secs(6)),
            _ => None,
        },
    }
}

impl App {
    fn signal(&self, path: &str, iface: &str, name: &str, body: &(impl serde::Serialize + zbus::zvariant::DynamicType)) {
        let _ = self.dbus.emit_signal(None::<&str>, path, iface, name, body);
    }

    fn on_ev(&mut self, ev: Ev) {
        match ev {
            Ev::Show(id) => self.show(id),
            Ev::Close(id, reason) => self.close(id, reason),
            Ev::CloseAll => {
                for id in self.shown.iter().map(|s| s.id).collect::<Vec<_>>() {
                    self.close(id, 2);
                }
            }
            Ev::Invoke(id, key) => self.invoke(id, &key),
            Ev::Changed => {}
        }
        self.signal(CTL_PATH, CTL_IFACE, "Changed", &());
        self.redraw();
    }

    fn show(&mut self, id: u32) {
        let store = self.shared.store.lock().unwrap();
        let Some(n) = store.get(id) else { return };
        if store.dnd && !n.bypass_dnd {
            return;
        }
        let until = lifetime(n.expire, n.urgency).map(|d| Instant::now() + d);
        drop(store);
        // reemplazo: se queda en su lugar con su tiempo nuevo; nuevo: hasta arriba
        if let Some(s) = self.shown.iter_mut().find(|s| s.id == id) {
            s.until = until;
            return;
        }
        self.shown.insert(0, Shown { id, until, y: (0.0, 0.0), buttons: Vec::new() });
        while self.shown.len() > MAX {
            let old = self.shown.last().unwrap().id;
            self.close(old, 1);
        }
    }

    /// Se va de la pantalla. 1 se le acabó el tiempo, 2 la quitó el usuario,
    /// 3 la quitó la app (esa también se va del historial).
    fn close(&mut self, id: u32, reason: u32) {
        self.shown.retain(|s| s.id != id);
        let mut store = self.shared.store.lock().unwrap();
        if reason == 3 || store.get(id).is_some_and(|n| n.transient) {
            store.remove(id);
        }
        drop(store);
        self.signal(PATH, IFACE, "NotificationClosed", &(id, reason));
    }

    fn invoke(&mut self, id: u32, key: &str) {
        self.signal(PATH, IFACE, "ActionInvoked", &(id, key));
        self.close(id, 2);
    }

    /// Los que ya cumplieron su tiempo (si el puntero no está encima).
    fn tick(&mut self) {
        if self.hover {
            return;
        }
        let now = Instant::now();
        let done: Vec<u32> = self.shown.iter().filter(|s| s.until.is_some_and(|t| t <= now)).map(|s| s.id).collect();
        if !done.is_empty() {
            for id in done {
                self.close(id, 1);
            }
            self.redraw();
        }
    }

    fn cards(&mut self) -> Vec<(Card, Option<Icon>)> {
        let store = self.shared.store.lock().unwrap();
        let mut gone = Vec::new();
        let cards = self
            .shown
            .iter()
            .filter_map(|s| match store.get(s.id) {
                Some(n) => {
                    let icon = self.icons.get(&n.icon);
                    Some((card(n, &self.look, icon.is_some()), icon))
                }
                None => {
                    gone.push(s.id);
                    None
                }
            })
            .collect();
        drop(store);
        self.shown.retain(|s| !gone.contains(&s.id));
        cards
    }

    /// Volver a armar la superficie: tamaño nuevo si cambió (y esperar a que el
    /// compositor diga que sí), o dibujar de una vez.
    fn redraw(&mut self) {
        let cards = self.cards();
        if cards.is_empty() {
            // nada que mostrar: fuera la superficie
            self.layer = None;
            self.configured = false;
            return;
        }
        let w = self.painter.size(cards[0].0.buf.area.width, 1).0;
        let h = cards.iter().map(|c| self.painter.size(1, c.0.buf.area.height).1).sum::<u32>() + GAP * (cards.len() as u32 - 1);
        if self.layer.is_none() {
            let surface = self.compositor.create_surface(&self.qh);
            let layer = self.layer_shell.create_layer_surface(&self.qh, surface, Layer::Overlay, Some("arre"), None);
            layer.set_anchor(Anchor::TOP);
            layer.set_margin(MARGIN, 0, 0, 0);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            layer.set_size(w, h);
            layer.commit();
            self.layer = Some(layer);
            self.size = (w, h);
            self.configured = false;
            return;
        }
        if self.size != (w, h) {
            self.size = (w, h);
            let layer = self.layer.as_ref().unwrap();
            layer.set_size(w, h);
            layer.commit();
            // el configure que responde el compositor dibuja
            self.configured = false;
            return;
        }
        if self.configured {
            self.draw(cards);
        }
    }

    fn draw(&mut self, cards: Vec<(Card, Option<Icon>)>) {
        let Some(layer) = self.layer.clone() else { return };
        let s = self.scale.max(1);
        let (w, h) = (self.size.0 as i32 * s, self.size.1 as i32 * s);
        let Some(mut pix) = tiny_skia::Pixmap::new(w as u32, h as u32) else { return };
        let mut y = 0.0f32;
        for ((c, icon), shown) in cards.iter().zip(self.shown.iter_mut()) {
            self.painter.card(&mut pix, &c.buf, &self.look, 0.0, y * s as f32, s as f32);
            if let (Some(icon), Some(cell)) = (icon, c.icon) {
                self.painter.icon(&mut pix, icon, cell, self.look.theme.fg, 0.0, y * s as f32, s as f32);
            }
            let ch = self.painter.size(1, c.buf.area.height).1 as f32;
            shown.y = (y as f64, (y + ch) as f64);
            let (cw, chh) = (self.painter.cw as f64, self.painter.ch as f64);
            shown.buttons = c
                .buttons
                .iter()
                .map(|(r, key)| {
                    let x0 = r.x as f64 * cw;
                    let y0 = y as f64 + r.y as f64 * chh;
                    ((x0, y0, x0 + r.width as f64 * cw, y0 + chh), key.clone())
                })
                .collect();
            y += ch + GAP as f32;
        }

        let pool = match &mut self.pool {
            Some(p) => p,
            None => self.pool.insert(SlotPool::new((w * h * 4) as usize, &self.shm).expect("memoria compartida")),
        };
        let Ok((buffer, canvas)) = pool.create_buffer(w, h, w * 4, wl_shm::Format::Argb8888) else { return };
        // tiny-skia: RGBA premultiplicado; Wayland ARGB8888 en little endian: BGRA
        for (dst, src) in canvas.chunks_exact_mut(4).zip(pix.data().chunks_exact(4)) {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }
        let surface = layer.wl_surface();
        surface.set_buffer_scale(s);
        surface.damage_buffer(0, 0, w, h);
        if buffer.attach_to(surface).is_ok() {
            layer.commit();
        }
    }

    fn click(&mut self, x: f64, y: f64, button: u32) {
        let Some(s) = self.shown.iter().find(|s| y >= s.y.0 && y < s.y.1) else { return };
        let id = s.id;
        match button {
            BTN_LEFT => {
                if let Some((_, key)) = s.buttons.iter().find(|((x0, y0, x1, y1), _)| x >= *x0 && x < *x1 && y >= *y0 && y < *y1) {
                    let key = key.clone();
                    self.invoke(id, &key);
                } else if self.shared.store.lock().unwrap().get(id).is_some_and(|n| n.actions.iter().any(|a| a.0 == "default")) {
                    self.invoke(id, "default");
                } else {
                    self.close(id, 2);
                }
            }
            BTN_RIGHT => self.close(id, 2),
            BTN_MIDDLE => {
                for id in self.shown.iter().map(|s| s.id).collect::<Vec<_>>() {
                    self.close(id, 2);
                }
            }
            _ => return,
        }
        self.signal(CTL_PATH, CTL_IFACE, "Changed", &());
        self.redraw();
    }
}

pub fn run(shared: Arc<Shared>, rx: Channel<Ev>, dbus: zbus::blocking::Connection) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("Wayland: {e}"))?;
    let (globals, queue) = registry_queue_init(&conn).map_err(|e| format!("Wayland: {e}"))?;
    let qh = queue.handle();
    let mut event_loop: EventLoop<App> = EventLoop::try_new().map_err(|e| e.to_string())?;
    let handle = event_loop.handle();
    WaylandSource::new(conn.clone(), queue).insert(handle.clone()).map_err(|e| e.to_string())?;
    handle
        .insert_source(rx, |ev, _, app: &mut App| {
            if let Event::Msg(ev) = ev {
                app.on_ev(ev);
            }
        })
        .map_err(|e| e.to_string())?;

    let mut app = App {
        registry: RegistryState::new(&globals),
        seat: SeatState::new(&globals, &qh),
        output: OutputState::new(&globals, &qh),
        compositor: CompositorState::bind(&globals, &qh).map_err(|_| "el compositor no tiene wl_compositor")?,
        layer_shell: LayerShell::bind(&globals, &qh).map_err(|_| "el compositor no tiene layer-shell")?,
        shm: Shm::bind(&globals, &qh).map_err(|_| "el compositor no tiene wl_shm")?,
        pool: None,
        layer: None,
        configured: false,
        size: (0, 0),
        scale: 1,
        pointer: None,
        hover: false,
        shown: Vec::new(),
        shared,
        dbus,
        look: Look::load(),
        painter: Painter::new(),
        icons: Icons::new(),
        qh,
    };
    let mut last_look = Instant::now();
    loop {
        event_loop.dispatch(Duration::from_millis(200), &mut app).map_err(|e| e.to_string())?;
        app.tick();
        // otro look: los que están en pantalla se redibujan con él
        if last_look.elapsed() > Duration::from_secs(1) {
            last_look = Instant::now();
            if app.look.refresh() {
                app.painter = Painter::new();
                app.size = (0, 0);
                app.redraw();
            }
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, factor: i32) {
        if factor != self.scale {
            self.scale = factor;
            self.redraw();
        }
    }
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.layer = None;
        self.configured = false;
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface, cfg: LayerSurfaceConfigure, _: u32) {
        if self.layer.as_ref() != Some(layer) {
            return;
        }
        if cfg.new_size.0 > 0 && cfg.new_size.1 > 0 {
            self.size = cfg.new_size;
        }
        self.configured = true;
        let cards = self.cards();
        if !cards.is_empty() {
            self.draw(cards);
        }
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat.get_pointer(qh, &seat).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Pointer {
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for App {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for e in events {
            if self.layer.as_ref().is_none_or(|l| &e.surface != l.wl_surface()) {
                continue;
            }
            match e.kind {
                PointerEventKind::Enter { .. } => self.hover = true,
                PointerEventKind::Leave { .. } => {
                    self.hover = false;
                    // al salir, que alcance a verse un rato más
                    let min = Instant::now() + Duration::from_secs(2);
                    for s in &mut self.shown {
                        if let Some(t) = &mut s.until {
                            *t = (*t).max(min);
                        }
                    }
                }
                PointerEventKind::Press { button, .. } => self.click(e.position.0, e.position.1, button),
                _ => {}
            }
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(App);

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(App);
