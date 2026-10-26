//! arre — "arre, enterado": las notificaciones de discada. Es el servidor de
//! notificaciones del escritorio (lo que reciben notify-send y las apps) y
//! las muestra con el mismo look que vato y rola.
//!
//!   arre                     el daemon (para el exec-once de Hyprland)
//!   arre dnd [on|off|toggle] no molestar (sin nada: dice cómo está)
//!   arre count               cuántas hay en el historial
//!   arre list                el historial en JSON
//!   arre clear               borrar el historial y lo que está en pantalla
//!   arre close <id>          quitar una
//!   arre waybar              para un módulo custom de waybar: JSON con cuántas
//!                            hay y si está en no molestar, cada que cambia
//!
//! `--test` usa otro nombre en el bus (org.discada.Arre.Test) y no toma
//! org.freedesktop.Notifications, para probar con otro daemon corriendo.

mod bus;
mod card;
mod icon;
mod look;
mod paint;
mod popup;
mod store;

use bus::{CTL_IFACE, CTL_PATH, Control, Notifications, PATH, Shared};
use smithay_client_toolkit::reexports::calloop::channel::channel;
use std::sync::{Arc, Mutex};
use store::Store;

/// Idioma de discada (hypr/discada.conf → LANGUAGE=es|en); por defecto inglés.
pub fn spanish() -> bool {
    static ES: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ES.get_or_init(|| {
        let f = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/hypr/discada.conf");
        std::fs::read_to_string(f)
            .ok()
            .and_then(|t| t.lines().find_map(|l| l.strip_prefix("LANGUAGE=").map(|v| v.trim_start().starts_with("es"))))
            .unwrap_or(false)
    })
}

/// es / en
pub fn l(es: &'static str, en: &'static str) -> &'static str {
    if spanish() { es } else { en }
}

fn bus_name(test: bool) -> &'static str {
    if test { "org.discada.Arre.Test" } else { "org.discada.Arre" }
}

fn daemon(test: bool) -> Result<(), String> {
    let (tx, rx) = channel();
    let shared = Arc::new(Shared { store: Mutex::new(Store::load()), tx: Mutex::new(tx) });
    let mut b = zbus::blocking::connection::Builder::session()
        .and_then(|b| b.name(bus_name(test)))
        .and_then(|b| b.serve_at(PATH, Notifications(shared.clone())))
        .and_then(|b| b.serve_at(CTL_PATH, Control(shared.clone())))
        .map_err(|e| e.to_string())?;
    if !test {
        b = b.name("org.freedesktop.Notifications").map_err(|e| e.to_string())?;
    }
    let conn = b.build().map_err(|e| match e {
        zbus::Error::NameTaken => l(
            "ya hay otro servidor de notificaciones (¿swaync, mako, dunst u otro arre?)",
            "another notification server is running (swaync, mako, dunst or another arre?)",
        )
        .to_string(),
        e => e.to_string(),
    })?;
    popup::run(shared, rx, conn)
}

fn client(test: bool, method: &str, body: &(impl serde::Serialize + zbus::zvariant::DynamicType)) -> Result<zbus::message::Message, String> {
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    conn.call_method(Some(bus_name(test)), CTL_PATH, Some(CTL_IFACE), method, body).map_err(|e| match e {
        zbus::Error::MethodError(name, ..) if name.as_str().ends_with("ServiceUnknown") => {
            l("arre no está corriendo", "arre isn't running").to_string()
        }
        e => e.to_string(),
    })
}

/// Como `swaync-client -swb`: un renglón JSON para waybar cada que algo
/// cambia (mismos `alt`: notification, none, dnd-notification, dnd-none).
/// Si arre no está, preguntarle lo arranca (activación de D-Bus); si se cae,
/// espera y vuelve a intentar.
fn waybar(test: bool) -> ! {
    use std::io::Write;
    // con otro nombre, `pkill arre` (para reiniciar el daemon) no se lleva el módulo
    let _ = std::fs::write("/proc/self/comm", "arre-waybar");
    let print = |count: u32, dnd: bool| {
        let alt = match (dnd, count > 0) {
            (false, true) => "notification",
            (false, false) => "none",
            (true, true) => "dnd-notification",
            (true, false) => "dnd-none",
        };
        let tooltip = match (count, dnd) {
            (0, false) => l("Sin notificaciones", "No notifications").to_string(),
            (1, false) => format!("1 {}", l("notificación", "notification")),
            (n, false) => format!("{n} {}", l("notificaciones", "notifications")),
            (n, true) => format!("{n} · {}", l("no molestar", "do not disturb")),
        };
        let tooltip = format!("{tooltip}\n{}", l("Clic derecho: no molestar", "Right click: do not disturb"));
        let line = serde_json::json!({ "text": count.to_string(), "alt": alt, "tooltip": tooltip, "class": alt });
        println!("{line}");
        let _ = std::io::stdout().flush();
    };
    loop {
        let r: Result<(), String> = (|| {
            let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
            let rule = zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface(CTL_IFACE)
                .and_then(|b| b.member("Changed"))
                .map_err(|e| e.to_string())?
                .build();
            let mut changes = zbus::blocking::MessageIterator::for_match_rule(rule, &conn, Some(16)).map_err(|e| e.to_string())?;
            loop {
                let ask = |m: &str| conn.call_method(Some(bus_name(test)), CTL_PATH, Some(CTL_IFACE), m, &()).map_err(|e| e.to_string());
                let count: u32 = ask("Count")?.body().deserialize().map_err(|e| e.to_string())?;
                let dnd: bool = ask("Dnd")?.body().deserialize().map_err(|e| e.to_string())?;
                print(count, dnd);
                changes.next().ok_or("bus")?.map_err(|e| e.to_string())?;
            }
        })();
        let _ = r;
        print(0, false);
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let test = args.iter().any(|a| a == "--test");
    args.retain(|a| a != "--test");
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result: Result<(), String> = (|| match args.as_slice() {
        [] => daemon(test),
        ["dnd", rest @ ..] => {
            let now: bool = client(test, "Dnd", &())?.body().deserialize().map_err(|e| e.to_string())?;
            let want = match rest {
                [] => now,
                ["on"] => true,
                ["off"] => false,
                ["toggle"] => !now,
                _ => return Err("arre dnd [on|off|toggle]".into()),
            };
            if want != now {
                client(test, "SetDnd", &(want,))?;
            }
            println!("{}", if want { "on" } else { "off" });
            Ok(())
        }
        ["count"] => {
            let n: u32 = client(test, "Count", &())?.body().deserialize().map_err(|e| e.to_string())?;
            println!("{n}");
            Ok(())
        }
        ["list"] => {
            let s: String = client(test, "List", &())?.body().deserialize().map_err(|e| e.to_string())?;
            println!("{s}");
            Ok(())
        }
        ["clear"] => client(test, "Clear", &()).map(|_| ()),
        ["waybar"] => waybar(test),
        ["close", id] => {
            let id: u32 = id.parse().map_err(|_| "arre close <id>".to_string())?;
            client(test, "Dismiss", &(id,)).map(|_| ())
        }
        ["-h" | "--help" | "help"] => {
            println!("arre [dnd [on|off|toggle] | count | list | clear | close <id> | waybar] [--test]");
            Ok(())
        }
        _ => Err(format!("{}: arre --help", l("no entendí", "unknown command"))),
    })();
    if let Err(e) = result {
        eprintln!("arre: {e}");
        std::process::exit(1);
    }
}
