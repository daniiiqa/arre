//! Las notificaciones: las que llegan, el historial y el "no molestar". Se
//! guarda en ~/.local/state/arre/state.json, así sobrevive reinicios y vato
//! lo puede leer.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Cuántas se guardan; las más viejas se van.
const KEEP: usize = 200;

#[derive(Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: u32,
    pub app: String,
    pub icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<(String, String)>, // clave, texto
    pub urgency: u8,                    // 0 baja, 1 normal, 2 crítica
    pub value: Option<i32>,             // una barra (volumen, brillo…)
    pub tag: Option<String>,            // misma etiqueta = reemplaza a la anterior
    pub time: u64,                      // unix
    /// no se queda en el historial (`notify-send -e`)
    #[serde(skip)]
    pub transient: bool,
    /// ms; -1 lo que diga arre, 0 nunca se va sola
    #[serde(skip)]
    pub expire: i32,
    /// sale aunque esté "no molestar"
    #[serde(skip)]
    pub bypass_dnd: bool,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub next: u32,
    #[serde(default)]
    pub notes: Vec<Note>, // la más nueva al final
    #[serde(default)]
    pub dnd: bool,
}

fn file() -> PathBuf {
    let state = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state"));
    state.join("arre/state.json")
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Store {
    pub fn load() -> Store {
        std::fs::read_to_string(file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        if cfg!(test) {
            return;
        }
        let f = file();
        let keep = Store { next: self.next, notes: self.history(), dnd: self.dnd };
        if let (Some(dir), Ok(text)) = (f.parent(), serde_json::to_string(&keep)) {
            let _ = std::fs::create_dir_all(dir);
            let tmp = f.with_extension("tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(tmp, f);
            }
        }
    }

    /// Las que se quedan (sin las de paso).
    pub fn history(&self) -> Vec<Note> {
        self.notes.iter().filter(|n| !n.transient).cloned().collect()
    }

    /// Una que llega. Si reemplaza a otra (por id o por etiqueta) toma su
    /// lugar y su id; si no, id nuevo. Devuelve el id.
    pub fn add(&mut self, mut n: Note, replaces: u32) -> u32 {
        let by_tag = || n.tag.as_ref().and_then(|t| self.notes.iter().find(|x| x.tag.as_ref() == Some(t)).map(|x| x.id));
        let id = match (replaces, by_tag()) {
            (0, Some(id)) => id,
            (0, None) => {
                self.next = self.next.wrapping_add(1).max(1);
                self.next
            }
            (r, _) => r,
        };
        n.id = id;
        self.notes.retain(|x| x.id != id);
        self.notes.push(n);
        if self.notes.len() > KEEP {
            self.notes.drain(..self.notes.len() - KEEP);
        }
        self.save();
        id
    }

    pub fn get(&self, id: u32) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }

    pub fn remove(&mut self, id: u32) {
        let before = self.notes.len();
        self.notes.retain(|n| n.id != id);
        if self.notes.len() != before {
            self.save();
        }
    }

    pub fn clear(&mut self) {
        self.notes.clear();
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(tag: Option<&str>) -> Note {
        Note {
            id: 0,
            app: "x".into(),
            icon: String::new(),
            summary: "s".into(),
            body: String::new(),
            actions: Vec::new(),
            urgency: 1,
            value: None,
            tag: tag.map(String::from),
            time: 0,
            transient: false,
            expire: -1,
            bypass_dnd: false,
        }
    }

    #[test]
    fn replaces_by_id_and_tag() {
        let mut s = Store::default();
        let a = s.add(note(None), 0);
        let b = s.add(note(Some("volume")), 0);
        assert_ne!(a, b);
        // misma etiqueta: mismo id, no se apila
        assert_eq!(s.add(note(Some("volume")), 0), b);
        // replaces_id: ese id, aunque ya no exista
        assert_eq!(s.add(note(None), a), a);
        assert_eq!(s.add(note(None), 77), 77);
        assert_eq!(s.notes.len(), 3);
    }
}
