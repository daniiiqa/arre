//! Los ajustes de arre en ~/.config/arre/config.json (por ahora, dónde salen
//! los avisos). El daemon lo vuelve a leer cuando cambia, así que el menú de
//! discada o `arre position` lo mueven al momento.

use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::SystemTime;

/// Dónde salen: arriba o abajo, a la izquierda, al centro o a la derecha.
pub const POSITIONS: [&str; 6] = ["top-left", "top-center", "top-right", "bottom-left", "bottom-center", "bottom-right"];
const DEFAULT: &str = "top-center";

fn file() -> PathBuf {
    let cfg = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"));
    cfg.join("arre/config.json")
}

fn read() -> Value {
    std::fs::read_to_string(file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}))
}

pub fn stamp() -> Option<SystemTime> {
    std::fs::metadata(file()).and_then(|m| m.modified()).ok()
}

pub fn position() -> &'static str {
    let v = read();
    let p = v["position"].as_str().unwrap_or(DEFAULT);
    POSITIONS.iter().find(|x| **x == p).copied().unwrap_or(DEFAULT)
}

/// Guardar la posición (sin tocar lo demás que haya en el archivo).
pub fn set_position(p: &str) -> Result<(), String> {
    if !POSITIONS.contains(&p) {
        return Err(format!("arre position [{}]", POSITIONS.join("|")));
    }
    let mut v = read();
    if !v.is_object() {
        v = json!({});
    }
    v["position"] = json!(p);
    let f = file();
    if let Some(dir) = f.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&f, serde_json::to_string_pretty(&v).unwrap_or_default() + "\n").map_err(|e| e.to_string())
}
