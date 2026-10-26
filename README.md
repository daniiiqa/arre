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

En pantalla: clic hace lo que pide el aviso (si pide algo) y lo quita, clic en
un botón usa ese botón, clic derecho lo quita, clic medio quita todos. Con el
puntero encima no se van solos.

Entiende lo que mandan los scripts de discada: urgencia, tiempo, botones
(`-A`), barra (`-h int:value:70`), reemplazar en vez de apilar
(`x-canonical-private-synchronous`), avisos de paso que no se guardan (`-e`) y
saltarse el no molestar (`SWAYNC_BYPASS_DND` o urgencia crítica).

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
