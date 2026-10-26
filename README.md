# arre

*Arre*: "va, enterado". Las notificaciones de discada: el servidor de
notificaciones del escritorio (lo que reciben `notify-send` y todas las apps)
con el mismo look que [vato](https://github.com/daniiiqa/vato) y
[rola](https://github.com/daniiiqa/rola). Cada aviso se arma como una pestaña
de vato (bordes, mayúsculas, barras y colores del look) y se dibuja con la
fuente y la opacidad de kitty, así se ve igual que la terminal en cada estilo
del rice.

```
arre                      el daemon (exec-once en Hyprland)
arre dnd [on|off|toggle]  no molestar
arre count                cuántas hay en el historial
arre list                 el historial en JSON
arre clear                borrar el historial y lo que está en pantalla
arre close <id>           quitar una
```

Salen arriba al centro, encima de todo (también de lo que esté en pantalla
completa). Clic hace lo que pide el aviso (si pide algo) y lo quita, clic en
un botón usa ese botón, clic derecho lo quita, clic medio quita todos. Con el
puntero encima no se van solos.

Entiende lo que mandan los scripts de discada: urgencia, tiempo, botones
(`-A`), barra (`-h int:value:70`), reemplazar en vez de apilar
(`x-canonical-private-synchronous`), avisos de paso que no se guardan (`-e`) y
saltarse el no molestar (`SWAYNC_BYPASS_DND` o urgencia crítica).

Íconos: una ruta (PNG, JPEG o SVG; se ve el formato por dentro, no por el
nombre), un nombre del tema de íconos (`media-record`: el de GTK y lo que
hereda, Adwaita, hicolor; si no hay de color, el *symbolic*, pintado del
color del look) o la imagen que manda la app (`image-data`: Discord, Spotify…,
guardada en `$XDG_RUNTIME_DIR/arre/`).

Para waybar, un módulo custom con `"exec": "$HOME/.local/bin/arre waybar"` y
`"return-type": "json"`: manda el contador y los mismos `alt` que swaync
(`notification`, `none`, `dnd-notification`, `dnd-none`), así los iconos de
cada look siguen sirviendo.

Para que D-Bus arranque arre solo (al iniciar sesión o si se cae) en vez de
otro daemon, en `~/.local/share/dbus-1/services/` van
`org.freedesktop.Notifications.service` y `org.discada.Arre.service` con
`Exec=` apuntando a arre.

El historial y el no molestar se guardan en `~/.local/state/arre/state.json`.
La superficie de Wayland se llama `arre`, por si quieres reglas de Hyprland
(`layerrule = blur, arre`).

## Instalar

```bash
cargo install --git https://github.com/daniiiqa/arre --root ~/.local --locked
```

Solo puede haber un servidor de notificaciones: antes de arrancar arre hay que
quitar swaync (o mako, dunst). Para probarlo con otro corriendo: `arre --test`
toma otro nombre en el bus y no recibe las de `notify-send`.

El look sale de `~/.config/arre/look.json` o, si no existe, del de rola, igual
que vato.

---

*Arre*: "got it". discada's notification daemon, styled like vato and rola.
