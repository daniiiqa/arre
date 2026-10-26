//! D-Bus: arre es el servidor de notificaciones del escritorio
//! (`org.freedesktop.Notifications`, el que usan notify-send y todas las apps)
//! y tiene su propia interfaz (`org.discada.Arre`) para el comando `arre` y
//! para vato: lista, contar, borrar, "no molestar".
//!
//! Lo que pasa aquí corre en el hilo de zbus; lo que se ve (los avisos) lo
//! maneja el hilo de Wayland, al que se le avisa por un canal.

use crate::store::{Note, Store, now};
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zbus::zvariant::OwnedValue;

pub const PATH: &str = "/org/freedesktop/Notifications";
pub const IFACE: &str = "org.freedesktop.Notifications";
pub const CTL_PATH: &str = "/org/discada/Arre";
pub const CTL_IFACE: &str = "org.discada.Arre";

/// Lo que el hilo de D-Bus le pide al de los avisos.
pub enum Ev {
    /// llegó (o se reemplazó) una
    Show(u32),
    /// cerrarla con este motivo (2 la quitó el usuario, 3 la app)
    Close(u32, u32),
    /// quitar todas las que están en pantalla
    CloseAll,
    /// apretaron un botón (desde vato o `arre`)
    Invoke(u32, String),
    /// cambió algo (no molestar, historial): redibujar y avisar a vato
    Changed,
}

pub struct Shared {
    pub store: Mutex<Store>,
    pub tx: Mutex<Sender<Ev>>,
}

impl Shared {
    pub fn send(&self, ev: Ev) {
        let _ = self.tx.lock().unwrap().send(ev);
    }
}

/// "<b>hola</b> &amp; adiós" → "hola & adiós": arre no anuncia markup, pero
/// hay apps que lo mandan igual.
pub fn plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut tag = false;
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            // "<b>", "</i>": etiqueta; "a < b": texto
            '<' if !tag && it.peek().is_some_and(|n| n.is_ascii_alphabetic() || *n == '/') => tag = true,
            '>' if tag => tag = false,
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn hint<T: TryFrom<OwnedValue>>(hints: &mut HashMap<String, OwnedValue>, key: &str) -> Option<T> {
    T::try_from(hints.remove(key)?).ok()
}

pub struct Notifications(pub Arc<Shared>);

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Notifications {
    fn get_capabilities(&self) -> Vec<&str> {
        vec!["body", "actions", "persistence", "x-canonical-private-synchronous", "synchronous"]
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        mut hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let urgency: u8 = hint(&mut hints, "urgency").unwrap_or(1);
        let value = hint::<i32>(&mut hints, "value")
            .or_else(|| hint::<u32>(&mut hints, "value").map(|v| v as i32))
            .map(|v| v.clamp(0, 100));
        let tag = ["x-canonical-private-synchronous", "synchronous", "x-dunst-stack-tag"]
            .iter()
            .find_map(|k| hint::<String>(&mut hints, k))
            .filter(|t| !t.is_empty());
        let icon = if app_icon.is_empty() { hint::<String>(&mut hints, "image-path").unwrap_or_default() } else { app_icon };
        let n = Note {
            id: 0,
            app: app_name,
            icon,
            summary: plain(&summary),
            body: plain(&body),
            // vienen en pares: clave, texto
            actions: actions.chunks(2).filter(|p| p.len() == 2).map(|p| (p[0].clone(), p[1].clone())).collect(),
            urgency,
            value,
            tag,
            time: now(),
            transient: hint(&mut hints, "transient").unwrap_or(false),
            expire: expire_timeout,
            bypass_dnd: urgency >= 2 || hint(&mut hints, "SWAYNC_BYPASS_DND").unwrap_or(false),
        };
        let id = self.0.store.lock().unwrap().add(n, replaces_id);
        self.0.send(Ev::Show(id));
        id
    }

    fn close_notification(&self, id: u32) {
        self.0.send(Ev::Close(id, 3));
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        ("arre".into(), "discada".into(), env!("CARGO_PKG_VERSION").into(), "1.2".into())
    }
}

pub struct Control(pub Arc<Shared>);

#[zbus::interface(name = "org.discada.Arre")]
impl Control {
    /// el historial en JSON (la más nueva al final)
    fn list(&self) -> String {
        serde_json::to_string(&self.0.store.lock().unwrap().history()).unwrap_or_else(|_| "[]".into())
    }

    fn count(&self) -> u32 {
        self.0.store.lock().unwrap().history().len() as u32
    }

    fn dnd(&self) -> bool {
        self.0.store.lock().unwrap().dnd
    }

    fn set_dnd(&self, on: bool) {
        let mut s = self.0.store.lock().unwrap();
        s.dnd = on;
        s.save();
        drop(s);
        self.0.send(Ev::Changed);
    }

    /// borrar el historial y quitar lo que está en pantalla
    fn clear(&self) {
        self.0.store.lock().unwrap().clear();
        self.0.send(Ev::CloseAll);
    }

    /// quitar una (de la pantalla y del historial)
    fn dismiss(&self, id: u32) {
        self.0.store.lock().unwrap().remove(id);
        self.0.send(Ev::Close(id, 2));
    }

    fn invoke(&self, id: u32, key: String) {
        self.0.send(Ev::Invoke(id, key));
    }
}

#[cfg(test)]
mod tests {
    use super::plain;

    #[test]
    fn strips_markup() {
        assert_eq!(plain("<b>Hola</b> &amp; <a href=\"x\">adiós</a> 1 &lt; 2"), "Hola & adiós 1 < 2");
        assert_eq!(plain("a < b > c"), "a < b > c");
    }
}
