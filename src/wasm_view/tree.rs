//! A frame's tree as GPUI elements: what the JavaScript bridge's `node()` does. Lengths
//! are the protocol's pixels at the app's zoom ([`z`]).

use std::collections::BTreeMap;
use std::ops::Range;

use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, Hsla, MouseButton, MouseDownEvent, SharedString, Stateful, Window, div, relative, uniform_list,
};
use gpui_base::input::Input;
use gpui_base::{Scrollbar, ScrollbarMode};
use wasm_mods_protocol::wire::Shown;
use wasm_mods_protocol::{
    Action, Align, ButtonKind, Event, Font, Justify, Layout, ListItem, ListStyle, ListWindow, Node, Pad, Paint, Role, Rounding, Scroll, Size,
    TextStyle, Variant, Wrap,
};

use super::{Colors, Hint, WasmView, canvas, hsla, z};

/// Where a node is drawn: in a row (else a column), in a virtual list's row (whose
/// clicks are the list's).
#[derive(Clone, Copy, Default)]
pub(crate) struct Ctx {
    pub in_row: bool,
    pub virtual_row: bool,
}

pub(crate) struct Build<'a> {
    pub popups: Vec<AnyElement>,
    pub images: &'a BTreeMap<String, Shown>,
    pub t: &'a Colors,
    pub mono: String,
    pub images_dir: Option<std::path::PathBuf>,
}

fn role(r: Role, t: &Colors) -> Hsla {
    match r {
        Role::Text | Role::Heading => t.fg,
        Role::Subtext => t.muted,
        Role::Muted => t.dim,
        Role::Accent => t.accent,
        Role::Danger => t.ansi[1],
        Role::Success => t.ansi[2],
        Role::Warning => t.ansi[3],
        Role::Info => t.ansi[4],
        Role::Space => t.bg,
        Role::Surface => t.surface,
        Role::Border | Role::Divider => t.border,
    }
}

fn paint(p: &Paint, t: &Colors) -> Hsla {
    match p {
        Paint::Rgba(c) => hsla(*c),
        Paint::Role(r) => role(*r, t),
    }
}

fn pad(p: Pad) -> Option<f32> {
    match p {
        Pad::Auto => None,
        Pad::None => Some(0.0),
        Pad::Xs => Some(4.0),
        Pad::S => Some(6.0),
        Pad::M => Some(10.0),
        Pad::L => Some(14.0),
    }
}

/// Width and height from a Layout, in a row or a column.
pub(crate) fn sized<E: Styled>(mut el: E, layout: &Layout, in_row: bool) -> E {
    match layout.width {
        // Filling part of a row: clipped to its share (side by side halves don't overlap).
        Size::Fill if in_row => el = el.flex_1().min_w_0().overflow_hidden(),
        Size::Fill => el = el.w_full(),
        Size::Fixed(w) => el = el.w(z(w)).flex_shrink_0(),
        _ => {}
    }
    match layout.height {
        Size::Fill if in_row => el = el.h_full(),
        Size::Fill => el = el.flex_1().min_h_0(),
        Size::Fixed(h) => el = el.h(z(h)),
        _ => {}
    }
    el
}

/// Whether the tree holds a virtual list (which scrolls itself and fills its room).
pub(crate) fn holds_virtual(n: &Node) -> bool {
    match n {
        Node::List { window, .. } => window.is_some(),
        Node::Column { children, .. } | Node::Row { children, .. } => children.iter().any(holds_virtual),
        _ => false,
    }
}

pub(crate) fn node(v: &mut WasmView, b: &mut Build, n: &Node, ctx: Ctx, window: &mut Window, cx: &mut Context<WasmView>) -> AnyElement {
    match n {
        Node::Empty => div().into_any_element(),
        Node::Column { .. } | Node::Row { .. } => container(v, b, n, ctx, window, cx),
        Node::Text { text: s, style, spans, color, size, bold, font, wrap } => text(b, s, *style, spans, color.as_ref(), *size, *bold, *font, *wrap, ctx, cx),
        Node::TextField { id, layout, monospace, .. } => field(v, b, id, layout, *monospace, ctx.in_row),
        Node::List { window: Some(w), .. } => virtual_list(v, b, n, w, ctx, cx),
        Node::List { id, items, selected, layout, dense, style, font, .. } => {
            let sel = v.lists.get(id).map_or(*selected, |l| l.selected);
            let soft = *style == ListStyle::Soft;
            let rows: Vec<AnyElement> =
                items.iter().enumerate().map(|(i, it)| list_row(v, b, id, it, i, sel, *dense, soft, None, window, cx)).collect();
            let mut el = div().flex().flex_col().gap(z(1.0)).children(rows);
            if *font == Font::Mono {
                el = el.font_family(SharedString::from(b.mono.clone()));
            }
            el = sized(el, layout, ctx.in_row);
            if layout.width == Size::Auto {
                el = el.w_full();
            }
            el.into_any_element()
        }
        Node::Canvas { .. } => canvas::canvas(v, b, n, ctx.in_row, cx),
        Node::Popup { id, modal, child, style, .. } => {
            v.open_popup = Some(id.clone());
            let layout = Layout { padding: Pad::M, ..Default::default() };
            let panel = Node::Column { children: vec![(**child).clone()], gap: 0.0, layout, style: *style, id: None, tooltip: None };
            let t = b.t;
            // A popup's box: the bridge's own look (the protocol has no variant for it).
            let panel = div()
                .flex()
                .flex_col()
                .bg(t.popover)
                .border_1()
                .border_color(t.border)
                .rounded(z(10.0))
                .child(container(v, b, &panel, Ctx::default(), window, cx));
            let (dismiss, modal) = (id.clone(), *modal);
            let mut overlay = div()
                .id(SharedString::from(format!("popup-{id}")))
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(move |v, _: &ClickEvent, _, _| {
                    if !modal {
                        v.send_events(vec![Event::Dismissed { id: dismiss.clone() }], None);
                    }
                }));
            if modal {
                overlay = overlay.bg(t.bg.opacity(0.6));
            }
            b.popups.push(
                overlay
                    .child(
                        div()
                            .id(SharedString::from(format!("popup-panel-{id}")))
                            .bg(t.bg)
                            .rounded(z(10.0))
                            .shadow_lg()
                            .max_w(relative(0.9))
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(panel),
                    )
                    .into_any_element(),
            );
            div().into_any_element()
        }
    }
}

fn container(v: &mut WasmView, b: &mut Build, n: &Node, ctx: Ctx, window: &mut Window, cx: &mut Context<WasmView>) -> AnyElement {
    let (row, children, gap, layout, style, id) = match n {
        Node::Column { children, gap, layout, style, id, .. } => (false, children, *gap, layout, style, id.as_deref()),
        Node::Row { children, gap, layout, style, id, .. } => (true, children, *gap, layout, style, id.as_deref()),
        _ => return div().into_any_element(),
    };
    let t = b.t;
    let mut el = div().flex().gap(z(gap));
    el = if row { el.flex_row() } else { el.flex_col() };
    let mut auto_pad = (0.0, 0.0);
    let mut hover: Option<Hsla> = None;
    let mut hover_dim = false;
    match style.variant {
        Variant::Plain => {}
        Variant::Card => {
            el = el.bg(t.surface).border_1().border_color(t.border).rounded(z(8.0));
            auto_pad = (10.0, 10.0);
        }
        Variant::Field => {
            el = el.bg(t.bg).border_1().border_color(t.input).rounded(z(6.0));
            auto_pad = (6.0, 6.0);
        }
        Variant::Pill => {
            el = el.bg(t.raised).rounded_full().text_size(z(11.5));
            auto_pad = (2.0, 8.0);
        }
        Variant::Selected => {
            el = el.bg(t.selected).rounded(z(6.0));
            auto_pad = (6.0, 6.0);
        }
        Variant::Button(kind) => {
            el = el.rounded(z(6.0)).items_center().justify_center().text_size(z(12.5)).font_weight(FontWeight::MEDIUM);
            auto_pad = (5.0, 12.0);
            match kind {
                ButtonKind::Primary => {
                    el = el.bg(t.solid).text_color(t.solid_fg);
                    hover_dim = true;
                }
                ButtonKind::Ghost => hover = Some(t.hover),
                ButtonKind::Danger => {
                    el = el.border_1().border_color(t.ansi[1]).text_color(t.ansi[1]);
                    hover = Some(t.hover);
                }
                ButtonKind::Secondary => {
                    el = el.bg(t.raised).border_1().border_color(t.input);
                    hover = Some(t.hover);
                }
            }
        }
    }
    el = match pad(layout.padding) {
        Some(p) => el.p(z(p)),
        None => el.py(z(auto_pad.0)).px(z(auto_pad.1)),
    };
    if let Some(bg) = &style.bg {
        el = el.bg(paint(bg, t));
    }
    if let Some(fg) = &style.fg {
        el = el.text_color(paint(fg, t));
    }
    if let Some(border) = &style.border {
        el = el.border_1().border_color(paint(border, t));
    }
    if let Some(r) = style.rounding {
        el = el.rounded(z(match r {
            Rounding::None => 0.0,
            Rounding::S => 4.0,
            Rounding::M => 6.0,
            Rounding::L => 10.0,
            Rounding::Full => 999.0,
        }));
    }
    if let Some(o) = style.opacity {
        el = el.opacity(o);
    }
    match layout.align {
        Align::Center => el = el.items_center(),
        Align::Auto if row => el = el.items_center(),
        Align::End => el = el.items_end(),
        Align::Start => el = el.items_start(),
        Align::Auto => {}
    }
    match layout.justify {
        Justify::Center => el = el.justify_center(),
        Justify::End => el = el.justify_end(),
        Justify::SpaceBetween => el = el.justify_between(),
        Justify::Start => {
            if row && layout.wrap != Some(false) {
                el = el.flex_wrap();
            }
        }
    }
    el = sized(el, layout, ctx.in_row);
    // Holding a virtual list (which scrolls itself): it takes the height it's given.
    if layout.height == Size::Auto && holds_virtual(n) {
        el = if ctx.in_row { el.h_full() } else { el.flex_1().min_h_0() };
    }
    let kid_ctx = Ctx { in_row: row, virtual_row: ctx.virtual_row };
    let kids: Vec<AnyElement> = children.iter().map(|c| node(v, b, c, kid_ctx, window, cx)).collect();
    let el = el.children(kids);
    let scroll = layout.scroll;
    let Some(id) = id.filter(|_| !ctx.virtual_row) else {
        if scroll == Scroll::None {
            return el.into_any_element();
        }
        // Scrolling needs an id; this one is only stable while the tree's shape is.
        return scrolled(el.id(SharedString::from(format!("s-{:p}", children.as_ptr()))), scroll).into_any_element();
    };
    let (click, secondary) = (id.to_owned(), id.to_owned());
    let mut el = el
        .id(SharedString::from(format!("n-{id}")))
        .cursor_pointer()
        .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
            v.send_events(vec![Event::Clicked { id: click.clone() }], None);
            cx.stop_propagation();
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |v, e: &MouseDownEvent, _, cx| {
                let zm = super::zoom();
                let (x, y) = (f32::from(e.position.x) / zm, f32::from(e.position.y) / zm);
                v.send_events(vec![Event::Secondary { id: secondary.clone(), x, y }], None);
                cx.stop_propagation();
            }),
        );
    if let Some(h) = hover {
        el = el.hover(move |s| s.bg(h));
    } else if hover_dim {
        el = el.hover(|s| s.opacity(0.88));
    }
    scrolled(el, scroll).into_any_element()
}

fn scrolled(el: Stateful<Div>, scroll: Scroll) -> Stateful<Div> {
    match scroll {
        Scroll::None => el,
        Scroll::Vertical => el.overflow_y_scroll(),
        Scroll::Horizontal => el.overflow_x_scroll(),
        Scroll::Both => el.overflow_scroll(),
    }
}

#[allow(clippy::too_many_arguments)]
fn text(
    b: &Build,
    s: &str,
    style: TextStyle,
    spans: &[wasm_mods_protocol::Span],
    color: Option<&Paint>,
    size: Option<f32>,
    bold: bool,
    font: Font,
    wrap: Wrap,
    ctx: Ctx,
    cx: &mut Context<WasmView>,
) -> AnyElement {
    let t = b.t;
    let mut el = div();
    let (mut sz, mut c) = (13.0, None);
    match style {
        TextStyle::Heading => {
            sz = 15.0;
            el = el.font_weight(FontWeight::SEMIBOLD);
        }
        TextStyle::Title => {
            sz = 18.0;
            el = el.font_weight(FontWeight::SEMIBOLD);
        }
        TextStyle::Muted => c = Some(t.muted),
        TextStyle::Accent => c = Some(t.accent),
        TextStyle::Small => {
            sz = 11.0;
            c = Some(t.dim);
        }
        TextStyle::Mono => sz = 12.5,
        TextStyle::Body => {}
    }
    if style == TextStyle::Mono || font == Font::Mono {
        el = el.font_family(SharedString::from(b.mono.clone()));
    }
    if let Some(s) = size {
        sz = s;
    }
    if bold {
        el = el.font_weight(FontWeight::SEMIBOLD);
    }
    if let Some(p) = color {
        c = Some(paint(p, t));
    }
    el = el.text_size(z(sz));
    if let Some(c) = c {
        el = el.text_color(c);
    }
    if spans.is_empty() {
        match wrap {
            Wrap::Extend => el = el.whitespace_nowrap().flex_shrink_0(),
            // One line cut with … : it takes the room its row leaves, or its container's width.
            Wrap::Truncate => el = el.w_full().min_w_0().flex_1().truncate(),
            Wrap::Wrap => {}
        }
        return el.child(s.to_owned()).into_any_element();
    }
    // On one line when it extends (a diff line) or is cut, else wrapping between runs.
    el = match wrap {
        Wrap::Extend => el.flex().flex_row().whitespace_nowrap().flex_shrink_0(),
        Wrap::Truncate => el.flex().flex_row().w_full().min_w_0().overflow_hidden().whitespace_nowrap(),
        Wrap::Wrap => el.flex().flex_row().flex_wrap(),
    };
    el.children(spans.iter().enumerate().map(|(i, s)| {
        let mut span = div().flex_shrink_0().child(s.text.clone());
        if let Some(col) = s.color {
            span = span.text_color(hsla(col));
        }
        if let Some(bg) = s.bg {
            span = span.bg(hsla(bg)).rounded(z(3.0));
        }
        match &s.link {
            Some(target) if !ctx.virtual_row => {
                let target = target.clone();
                span.id(SharedString::from(format!("link-{i}-{target}")))
                    .cursor_pointer()
                    .hover(|st| st.underline())
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| open_link(&target, cx)))
                    .into_any_element()
            }
            _ => span.into_any_element(),
        }
    }))
    .into_any_element()
}

/// A link in a text: an http(s) or file URL, or an absolute or `~/` path.
fn open_link(target: &str, cx: &mut Context<WasmView>) {
    if target.starts_with("http://") || target.starts_with("https://") || target.starts_with("file://") {
        cx.open_url(target);
    } else if let Some(rest) = target.strip_prefix("~/") {
        cx.open_url(&format!("file://{}/{rest}", std::env::var("HOME").unwrap_or_default()));
    } else if target.starts_with('/') {
        cx.open_url(&format!("file://{target}"));
    }
}

fn field(v: &mut WasmView, b: &Build, id: &str, layout: &Layout, monospace: bool, in_row: bool) -> AnyElement {
    let t = b.t;
    let Some(f) = v.fields.get(id) else { return div().into_any_element() };
    let mut input = div()
        .flex()
        .items_center()
        .h(z(32.0))
        .px(z(8.0))
        .rounded(z(6.0))
        .border_1()
        .border_color(if f.focused { t.accent } else { t.input })
        .bg(t.bg)
        .text_size(z(12.8))
        .text_color(t.fg)
        .w_full()
        .child(Input::new(&f.state));
    if monospace {
        input = input.font_family(SharedString::from(b.mono.clone()));
    }
    // The press is the field's: the view's own focus (for keys) mustn't take it back.
    let wrap = div().flex().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(input);
    sized(wrap, layout, in_row).into_any_element()
}

/// One row of a list; `i` counts from the list's first row. `row_h`: a virtual list's
/// row, that tall, whose clicks select, send `Clicked "<list>#<row>"` and, twice, open.
#[allow(clippy::too_many_arguments)]
fn list_row(
    v: &mut WasmView,
    b: &mut Build,
    id: &str,
    it: &ListItem,
    i: usize,
    sel: Option<usize>,
    dense: bool,
    soft: bool,
    row_h: Option<f32>,
    window: &mut Window,
    cx: &mut Context<WasmView>,
) -> AnyElement {
    let t = b.t;
    if it.header {
        let mut h = div().px(z(8.0)).pt(z(8.0)).pb(z(2.0)).text_size(z(11.0)).text_color(t.dim).child(it.label.clone());
        if let Some(rh) = row_h {
            h = h.h(z(rh)).overflow_hidden();
        }
        return h.into_any_element();
    }
    let is_sel = sel == Some(i);
    let fg = if is_sel && !soft { t.accent_fg } else if it.disabled { t.dim } else { t.fg };
    // A virtual row with content of its own is flush: its lines and lanes meet.
    let flush = row_h.is_some() && it.child.is_some();
    let mut row = div()
        .id(SharedString::from(format!("l-{id}-{i}")))
        .flex()
        .flex_row()
        .w_full()
        .gap(z(8.0))
        .px(z(8.0))
        .py(z(if flush { 0.0 } else if dense { 3.0 } else { 6.0 }))
        .rounded(z(if flush { 0.0 } else { 6.0 }))
        .items_center()
        .text_color(fg);
    if let Some(rh) = row_h {
        row = row.h(z(rh)).flex_shrink_0().overflow_hidden();
    }
    if is_sel {
        row = row.bg(if soft { t.selected } else { t.accent });
    } else {
        let h = t.hover;
        row = row.hover(move |s| s.bg(h));
    }
    if it.disabled {
        row = row.opacity(0.5);
    } else {
        let (list_id, virtual_row) = (id.to_owned(), row_h.is_some());
        row = row.cursor_pointer().on_click(cx.listener(move |v, e: &ClickEvent, window, cx| {
            if let Some(l) = v.lists.get_mut(&list_id) {
                l.selected = Some(i);
                l.local = true;
            }
            v.key_list = Some(list_id.clone());
            let mut events = vec![Event::Selected { id: list_id.clone(), index: i }];
            if virtual_row {
                // A click, not the keys (a folder opens on one click).
                events.push(Event::Clicked { id: format!("{list_id}#{i}") });
            }
            if e.click_count() >= 2 {
                events.push(Event::Activated { id: list_id.clone(), index: i });
            }
            v.send_events(events, None);
            v.focus.focus(window, cx);
            cx.notify();
        }));
    }
    if let Some(child) = &it.child {
        let inner = node(v, b, child, Ctx { in_row: false, virtual_row: row_h.is_some() }, window, cx);
        return row.child(div().flex_1().min_w_0().overflow_hidden().child(inner)).into_any_element();
    }
    if !it.tree.is_empty() {
        row = row.child(div().text_color(t.dim).child(it.tree.clone()));
    }
    if let Some(dot) = it.dot {
        let c = if is_sel && !soft { fg } else { hsla(dot) };
        let d = div().w(z(8.0)).h(z(8.0)).rounded_full().flex_shrink_0();
        row = row.child(if it.ring { d.border_1().border_color(c) } else { d.bg(c) });
    }
    if !it.icon.is_empty() {
        let mark = match it.icon.as_str() {
            "terminal" => ">_",
            "widget" | "connected" => "▦",
            other => other,
        };
        row = row.child(div().text_color(it.icon_color.map_or(t.dim, hsla)).child(mark.to_owned()));
    }
    let mut label = if it.label_spans.is_empty() {
        div().child(it.label.clone())
    } else {
        div().flex().flex_row().children(it.label_spans.iter().map(|s| div().text_color(s.color.map_or(fg, hsla)).child(s.text.clone())))
    };
    if let Some(r) = it.role
        && !is_sel
    {
        label = label.text_color(role(r, t));
    }
    if it.bold {
        label = label.font_weight(FontWeight::SEMIBOLD);
    }
    let mut main = div().flex().flex_col().flex_1().min_w_0().child(label);
    if !it.sub.is_empty() {
        main = main.child(div().flex().flex_row().text_size(z(11.5)).children(it.sub.iter().map(|s| div().text_color(s.color.map_or(t.dim, hsla)).child(s.text.clone()))));
    }
    row = row.child(main);
    if !it.detail.is_empty() {
        let c = if is_sel && !soft { fg } else { it.detail_color.map_or(t.dim, hsla) };
        row = row.child(div().text_size(z(12.0)).text_color(c).child(it.detail.clone()));
    }
    if !it.key.is_empty() {
        row = row.child(div().text_size(z(11.5)).text_color(t.accent).child(it.key.clone()));
    }
    if let Some(tr) = &it.trailing {
        let tid = tr.id.clone();
        let (hover_bg, hover_fg) = (t.hover, t.fg);
        row = row.child(
            div()
                .id(SharedString::from(format!("l-{id}-{i}-t")))
                .px(z(6.0))
                .rounded(z(4.0))
                .text_color(if is_sel && !soft { fg } else { t.dim })
                .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
                .cursor_pointer()
                .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
                    v.send_events(vec![Event::Clicked { id: tid.clone() }], None);
                    cx.stop_propagation();
                }))
                .child(tr.text.clone()),
        );
    }
    row.into_any_element()
}

/// One height for every row of a virtual list: the list places rows by it.
fn row_height(dense: bool) -> f32 {
    if dense { 26.0 } else { 36.0 }
}

/// A list of `window.total` rows of which the frame sent `items` (rows `window.first..`):
/// only the rows on screen are built ([`virtual_rows`]), the module hears which those are
/// (VisibleRange) and sends them for the next frame. It fills the room it's given, with a
/// scrollbar on the right (always shown on the frame's page).
fn virtual_list(v: &mut WasmView, b: &Build, n: &Node, w: &ListWindow, ctx: Ctx, cx: &mut Context<WasmView>) -> AnyElement {
    let Node::List { id, layout, font, pages, .. } = n else { return div().into_any_element() };
    let total = w.total as usize;
    let Some(vl) = v.virtual_lists.get_mut(id) else { return div().into_any_element() };
    if let Some(st) = w.scroll_to
        && vl.revision != Some(st.revision)
    {
        vl.revision = Some(st.revision);
        let row = (st.row.min(total.saturating_sub(1) as u64)) as usize;
        vl.handle.scroll_to_item(row, gpui::ScrollStrategy::Top);
    }
    let handle = vl.handle.clone();
    let list_id = id.clone();
    let rows = uniform_list(
        SharedString::from(format!("list-{id}")),
        total,
        cx.processor(move |v: &mut WasmView, range: Range<usize>, window, cx| virtual_rows(v, &list_id, range, window, cx)),
    )
    .track_scroll(&handle)
    .size_full();
    let mut bar = Scrollbar::vertical(&handle).id(SharedString::from(format!("bar-{id}")));
    if *pages {
        bar = bar.mode(ScrollbarMode::Always);
    }
    let mut el = div().relative().w_full().child(rows).child(bar);
    if *font == Font::Mono {
        el = el.font_family(SharedString::from(b.mono.clone()));
    }
    el = match layout.height {
        Size::Fixed(h) => el.h(z(h)),
        _ if ctx.in_row => el.h_full(),
        _ => el.flex_1().min_h_0(),
    };
    match layout.width {
        Size::Fill if ctx.in_row => el = el.flex_1().min_w_0(),
        Size::Fixed(w) => el = el.w(z(w)).flex_shrink_0(),
        _ => {}
    }
    el.into_any_element()
}

/// The rows of virtual list `id` in `range` (rows the module hasn't sent yet are blank),
/// and the module told that they show.
fn virtual_rows(v: &mut WasmView, id: &str, range: Range<usize>, window: &mut Window, cx: &mut Context<WasmView>) -> Vec<AnyElement> {
    let total = v.lists.get(id).map_or(0, |l| l.count);
    let count = range.end - range.start;
    // (A one-row range is the list measuring itself, not what shows.)
    if let Some(vl) = v.virtual_lists.get_mut(id)
        && (count > 1 || total <= 1)
        && vl.sent != Some((range.start, count))
    {
        vl.sent = Some((range.start, count));
        v.send_events(vec![Event::VisibleRange { id: id.to_owned(), first: range.start as u64, count: count as u64 }], None);
    }
    let Some(frame) = v.frame.take() else { return range.map(|_| div().into_any_element()).collect() };
    let Some(Node::List { items, selected, dense, style, window: Some(w), .. }) = find_list(&frame.ui, id) else {
        v.frame = Some(frame);
        return range.map(|_| div().into_any_element()).collect();
    };
    let t = Colors::now();
    let mut b = Build {
        popups: Vec::new(),
        images: &frame.images,
        t: &t,
        mono: crate::settings::get().font_family.clone(),
        images_dir: v.props.images.as_ref().map(std::path::PathBuf::from),
    };
    let (first, rh, soft) = (w.first as usize, row_height(*dense), *style == ListStyle::Soft);
    let sel = v.lists.get(id).map_or(*selected, |l| l.selected);
    let rows = range
        .map(|i| match i.checked_sub(first).and_then(|k| items.get(k)) {
            Some(it) => list_row(v, &mut b, id, it, i, sel, *dense, soft, Some(rh), window, cx),
            None => div().w_full().h(z(rh)).into_any_element(),
        })
        .collect();
    v.frame = Some(frame);
    rows
}

/// The list with this id in the tree.
fn find_list<'a>(n: &'a Node, id: &str) -> Option<&'a Node> {
    match n {
        Node::List { id: lid, .. } if lid == id => Some(n),
        Node::List { items, .. } => items.iter().filter_map(|it| it.child.as_deref()).find_map(|c| find_list(c, id)),
        Node::Column { children, .. } | Node::Row { children, .. } => children.iter().find_map(|c| find_list(c, id)),
        Node::Popup { child, .. } => find_list(child, id),
        _ => None,
    }
}

/// What the user can do now: the module's offers, clickable.
pub(crate) fn hints(actions: &[Hint], t: &Colors, cx: &mut Context<WasmView>) -> AnyElement {
    let fg = t.fg;
    div()
        .id("wasm-view-hints")
        .flex()
        .flex_row()
        .flex_shrink_0()
        .px(z(14.0))
        .py(z(6.0))
        .gap(z(14.0))
        .border_t_1()
        .border_color(t.border)
        .text_size(z(11.5))
        .text_color(t.dim)
        .children(actions.iter().enumerate().map(|(i, o)| {
            let glyph = match &o.action {
                Action::Back => "esc".to_owned(),
                Action::Open => "⏎".to_owned(),
                Action::Primary => "space".to_owned(),
                Action::Secondary => "tab".to_owned(),
                Action::Move(_) => "↑↓".to_owned(),
                Action::Key(_) => o.key.clone().unwrap_or_default(),
            };
            let action = o.action.clone();
            div()
                .id(("hint", i))
                .flex()
                .flex_row()
                .gap(z(5.0))
                .cursor_pointer()
                .hover(move |s| s.text_color(fg))
                .on_click(cx.listener(move |v, _: &ClickEvent, _, _| v.send_events(vec![Event::Action(action.clone())], None)))
                .child(div().px(z(5.0)).rounded(z(4.0)).bg(t.raised).text_color(t.muted).child(glyph))
                .child(o.label.clone())
        }))
        .into_any_element()
}
