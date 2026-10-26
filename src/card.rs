//! Un aviso armado como en vato y rola: un recuadro con el look (bordes,
//! mayúsculas, barra) dibujado con ratatui en una cuadrícula de celdas, que
//! luego `paint` pasa a pixeles con la fuente de kitty.

use crate::look::Look;
use crate::store::Note;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

/// Ancho de un aviso, en celdas.
pub const COLS: u16 = 44;
const SUMMARY_LINES: usize = 2;
const BODY_LINES: usize = 5;
/// El cuadro del ícono, en celdas (a la izquierda del texto).
const ICON_COLS: u16 = 5;
const ICON_ROWS: u16 = 2;

pub struct Card {
    pub buf: Buffer,
    /// botones: dónde quedaron (en celdas, dentro del aviso) y su clave
    pub buttons: Vec<(Rect, String)>,
    /// dónde va el ícono, si hay
    pub icon: Option<Rect>,
}

/// Partir en renglones de `w` celdas, por palabras; lo que no quepa en
/// `max` renglones termina en "…".
fn wrap(text: &str, w: usize, max: usize) -> Vec<String> {
    let width = |s: &str| Span::raw(s).width();
    let mut lines: Vec<String> = Vec::new();
    for para in text.lines() {
        let mut cur = String::new();
        for word in para.split_whitespace() {
            let sep = if cur.is_empty() { 0 } else { 1 };
            if width(&cur) + sep + width(word) <= w {
                if sep == 1 {
                    cur.push(' ');
                }
                cur.push_str(word);
                continue;
            }
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            // una palabra más larga que el renglón: a pedazos
            for c in word.chars() {
                if width(&cur) + width(c.encode_utf8(&mut [0; 4])) > w {
                    lines.push(std::mem::take(&mut cur));
                }
                cur.push(c);
            }
        }
        if !cur.is_empty() || para.is_empty() {
            lines.push(cur);
        }
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    if lines.len() > max {
        lines.truncate(max);
        let last = lines.last_mut().unwrap();
        while width(last) + 1 > w {
            last.pop();
        }
        last.push('…');
    }
    lines
}

pub fn card(n: &Note, look: &Look, with_icon: bool) -> Card {
    let t = look.theme;
    // borde y un espacio a cada lado; con ícono, también su cuadro y un espacio
    let shift = if with_icon { ICON_COLS + 1 } else { 0 };
    let inner_w = (COLS - 4 - shift) as usize;
    let summary = wrap(&n.summary, inner_w, SUMMARY_LINES);
    let body = wrap(&n.body, inner_w, BODY_LINES);
    let buttons: Vec<&(String, String)> = n.actions.iter().filter(|a| a.0 != "default" && !a.1.is_empty()).collect();
    let rows = summary.len() + body.len() + n.value.is_some() as usize + (!buttons.is_empty()) as usize;
    let min_rows = if with_icon { ICON_ROWS as usize } else { 1 };
    let area = Rect::new(0, 0, COLS, rows.max(min_rows) as u16 + 2);
    let mut buf = Buffer::empty(area);
    buf.set_style(area, Style::new().bg(t.base).fg(t.fg));

    let critical = n.urgency >= 2;
    let title = Span::styled(format!(" {} ", look.tx(&n.app)), Style::new().fg(if critical { t.red } else { t.primary }).bold());
    let block = look.block(false).border_style(Style::new().fg(if critical { t.red } else { t.border })).title(title);
    let inner = block.inner(area);
    block.render(area, &mut buf);
    let icon = with_icon.then(|| Rect::new(inner.x + 1, inner.y, ICON_COLS, ICON_ROWS));
    let text_area = Rect { x: inner.x + 1 + shift, width: inner.width.saturating_sub(2 + shift), ..inner };

    let mut lines: Vec<Line> = Vec::new();
    for s in summary {
        lines.push(Line::styled(s, Style::new().fg(t.fg).bold()));
    }
    for b in body {
        lines.push(Line::styled(b, Style::new().fg(t.muted)));
    }
    if let Some(v) = n.value {
        let (done, rest) = look.bar(v as f64 / 100.0, inner_w.saturating_sub(6));
        lines.push(Line::from(vec![
            Span::styled(done, Style::new().fg(t.primary)),
            Span::styled(rest, Style::new().fg(t.border)),
            Span::styled(format!(" {v:>3}%"), Style::new().fg(t.fg)),
        ]));
    }
    let mut hits = Vec::new();
    if !buttons.is_empty() {
        let y = text_area.y + lines.len() as u16;
        let mut x = text_area.x;
        let mut spans = Vec::new();
        for (key, label) in buttons {
            let label = format!(" {} ", look.tx(label));
            let w = Span::raw(&label).width() as u16;
            if x + w > text_area.right() {
                break;
            }
            hits.push((Rect::new(x, y, w, 1), key.clone()));
            spans.push(Span::styled(label, Style::new().fg(t.base).bg(t.primary).bold()));
            spans.push(Span::raw(" "));
            x += w + 1;
        }
        lines.push(Line::from(spans));
    }
    Paragraph::new(lines).render(text_area, &mut buf);
    Card { buf, buttons: hits, icon }
}

#[cfg(test)]
mod tests {
    use super::wrap;

    #[test]
    fn wraps_by_words() {
        assert_eq!(wrap("hola que tal amigo", 9, 5), vec!["hola que", "tal amigo"]);
        assert_eq!(wrap("uno\n\ndos", 10, 5), vec!["uno", "", "dos"]);
        assert_eq!(wrap("abcdefghij", 4, 5), vec!["abcd", "efgh", "ij"]);
        assert_eq!(wrap("a b c d e f", 3, 2), vec!["a b", "c …"]);
    }
}
