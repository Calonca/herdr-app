//! Canvases: shapes painted straight into the window, panned and zoomed here (the module
//! draws in world coordinates and hears what shows through `Event::View`).

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, ClickEvent, Context, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, PathBuilder, Pixels,
    Point, ScrollWheelEvent, SharedString, TextRun, Window, div, fill, img, point, px, size,
};
use wasm_mods_protocol::{Coords, Event, Fit, ImageSrc, Mods, Node, PanZoom, PointerKind, Shape, Size};

use super::tree::Build;
use super::{WasmView, hsla, z, zoom};

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct View {
    x: f32,
    y: f32,
    zoom: f32,
}

#[derive(Default)]
pub(crate) struct CanvasState {
    /// Its shapes, taken out of the frame when it arrives (painting borrows them).
    pub shapes: Arc<Vec<Shape>>,
    w: f32,
    h: f32,
    view: Option<View>,
    moved: bool,
    reset: u64,
    fitted: String,
    down: Option<Down>,
    sent_view: String,
    /// Where it was last painted, in window pixels.
    origin: Rc<Cell<Point<Pixels>>>,
}

#[derive(Clone, Copy)]
struct Down {
    pan: bool,
    x: f32,
    y: f32,
}

/// Takes the canvases' shapes out of a new frame into their states.
pub(crate) fn take_shapes(v: &mut WasmView, ui: &mut Node) {
    let mut nodes = vec![ui];
    while let Some(n) = nodes.pop() {
        match n {
            Node::Canvas { id, shapes, .. } => {
                let c = v.canvases.entry(id.clone()).or_default();
                c.shapes = Arc::new(std::mem::take(shapes));
            }
            Node::Column { children, .. } | Node::Row { children, .. } => nodes.extend(children.iter_mut()),
            Node::Popup { child, .. } => nodes.push(child),
            Node::List { items, .. } => nodes.extend(items.iter_mut().filter_map(|it| it.child.as_deref_mut())),
            _ => {}
        }
    }
}

fn fit(c: &CanvasState, pz: &PanZoom) -> View {
    let Some((cw, ch)) = pz.content.filter(|_| c.w > 0.0 && c.h > 0.0) else { return View { x: 0.0, y: 0.0, zoom: 1.0 } };
    let zoom = (c.w / cw).min(c.h / ch).clamp(pz.min_zoom, pz.max_zoom);
    View { x: -(c.w / zoom - cw) / 2.0, y: -(c.h / zoom - ch) / 2.0, zoom }
}

fn zoom_at(c: &mut CanvasState, pz: &PanZoom, factor: f32, px_: f32, py_: f32) {
    let Some(v) = c.view else { return };
    let zoom = (v.zoom * factor).clamp(pz.min_zoom, pz.max_zoom);
    let (wx, wy) = (v.x + px_ / v.zoom, v.y + py_ / v.zoom);
    c.view = Some(View { x: wx - px_ / zoom, y: wy - py_ / zoom, zoom });
    c.moved = true;
}

/// Tells the module what a canvas with a view shows, when that changed.
fn send_view(v: &mut WasmView, id: &str) {
    let Some(c) = v.canvases.get_mut(id) else { return };
    let Some(view) = c.view.filter(|_| c.w > 0.0) else { return };
    let e = Event::View { id: id.to_owned(), x: view.x, y: view.y, width: c.w / view.zoom, height: c.h / view.zoom, zoom: view.zoom };
    let key = format!("{e:?}");
    if key == c.sent_view {
        return;
    }
    c.sent_view = key;
    v.send_events(vec![e], None);
}

/// Refits a view that the user hasn't moved (new size or content), or one the module reset.
fn settle(v: &mut WasmView, id: &str, pz: Option<&PanZoom>) {
    let Some(c) = v.canvases.get_mut(id) else { return };
    let Some(pz) = pz else {
        c.view = None;
        return;
    };
    let fitted = format!("{}x{}:{:?}", c.w, c.h, pz.content);
    if c.view.is_none() || pz.reset > c.reset || (!c.moved && c.fitted != fitted) {
        c.view = Some(fit(c, pz));
        c.reset = pz.reset;
        c.moved = false;
        c.fitted = fitted;
        if c.w > 0.0 {
            send_view(v, id);
        }
    }
}

fn mods(m: &gpui::Modifiers) -> Mods {
    Mods { ctrl: m.control, shift: m.shift, alt: m.alt }
}

pub(crate) fn canvas(v: &mut WasmView, b: &Build, n: &Node, in_row: bool, cx: &mut Context<WasmView>) -> AnyElement {
    let Node::Canvas { id, width, height, layout, coords, view: pz, sense, .. } = n else { return div().into_any_element() };
    let (id, unit, pz, sense) = (id.clone(), *coords == Coords::Unit, *pz, *sense);
    v.canvases.entry(id.clone()).or_default();
    settle(v, &id, pz.as_ref());
    let c = &v.canvases[&id];
    let (w, h) = (c.w, c.h);
    let view = c.view.unwrap_or(View { x: 0.0, y: 0.0, zoom: 1.0 });
    let origin = c.origin.clone();
    // The app's zoom: this canvas keeps everything in the protocol's pixels (window
    // pixels divided by it) and paints at it.
    let zm = zoom();
    let local = move |p: Point<Pixels>, o: Point<Pixels>| (f32::from(p.x - o.x) / zm, f32::from(p.y - o.y) / zm);

    let mut el = div().id(SharedString::from(format!("cv-{id}"))).relative().flex_shrink_0().overflow_hidden().h(z(*height));
    if layout.width == Size::Fill || (*width == 0.0 && unit) {
        el = if in_row { el.flex_1() } else { el.w_full() };
    } else {
        el = el.w(z(*width));
    }
    el = super::tree_sized(el, layout, in_row);

    // World → this element's pixels, and back (what events carry).
    let to_world = move |p: Point<Pixels>, o: Point<Pixels>, view: View, w: f32, h: f32| {
        let (lx, ly) = local(p, o);
        let (x, y) = (lx / view.zoom + view.x, ly / view.zoom + view.y);
        if unit { (x / w.max(1.0), y / h.max(1.0)) } else { (x, y) }
    };
    let pointer = {
        let id = id.clone();
        move |v: &mut WasmView, kind: PointerKind, p: Point<Pixels>, m: &gpui::Modifiers| {
            let Some(c) = v.canvases.get(&id) else { return };
            let (x, y) = to_world(p, c.origin.get(), c.view.unwrap_or(View { x: 0.0, y: 0.0, zoom: 1.0 }), c.w, c.h);
            v.send_events(vec![Event::Pointer { id: id.clone(), x, y, kind }], Some(mods(m)));
        }
    };

    let (cid, p) = (id.clone(), pointer.clone());
    el = el.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |v, e: &MouseDownEvent, window, cx| {
            let Some(c) = v.canvases.get_mut(&cid) else { return };
            let (x, y) = local(e.position, c.origin.get());
            if pz.is_some_and(|pz| pz.drag_pans) {
                c.down = Some(Down { pan: true, x, y });
            } else {
                c.down = Some(Down { pan: false, x, y });
                p(v, PointerKind::Down, e.position, &e.modifiers);
            }
            v.focus.focus(window, cx);
            cx.stop_propagation();
        }),
    );
    let cid = id.clone();
    el = el.on_mouse_down(
        MouseButton::Middle,
        cx.listener(move |v, e: &MouseDownEvent, _, cx| {
            if pz.is_none() {
                return;
            }
            let Some(c) = v.canvases.get_mut(&cid) else { return };
            let (x, y) = local(e.position, c.origin.get());
            c.down = Some(Down { pan: true, x, y });
            cx.stop_propagation();
        }),
    );
    let (cid, p) = (id.clone(), pointer.clone());
    el = el.on_mouse_move(cx.listener(move |v, e: &MouseMoveEvent, _, cx| {
        let Some(c) = v.canvases.get_mut(&cid) else { return };
        let (x, y) = local(e.position, c.origin.get());
        match (c.down, c.view) {
            (Some(d), Some(view)) if d.pan => {
                c.view = Some(View { x: view.x - (x - d.x) / view.zoom, y: view.y - (y - d.y) / view.zoom, ..view });
                c.down = Some(Down { x, y, ..d });
                c.moved = true;
                cx.notify();
            }
            (Some(_), _) if e.dragging() => p(v, PointerKind::Drag, e.position, &e.modifiers),
            (Some(_), _) => {}
            (None, _) if sense.hover => p(v, PointerKind::Move, e.position, &e.modifiers),
            _ => {}
        }
    }));
    for button in [MouseButton::Left, MouseButton::Middle] {
        let (cid, p) = (id.clone(), pointer.clone());
        el = el.on_mouse_up(
            button,
            cx.listener(move |v, e: &MouseUpEvent, _, _| {
                let Some(c) = v.canvases.get_mut(&cid) else { return };
                let Some(d) = c.down.take() else { return };
                if d.pan {
                    send_view(v, &cid);
                } else {
                    p(v, PointerKind::Up, e.position, &e.modifiers);
                }
            }),
        );
    }
    if sense.hover {
        let cid = id.clone();
        el = el.on_hover(cx.listener(move |v, hovered: &bool, _, _| {
            let kind = if *hovered { PointerKind::Enter } else { PointerKind::Leave };
            v.send_events(vec![Event::Pointer { id: cid.clone(), x: 0.0, y: 0.0, kind }], None);
        }));
    }
    if let Some(pz) = pz {
        let cid = id.clone();
        el = el.on_scroll_wheel(cx.listener(move |v, e: &ScrollWheelEvent, window, cx| {
            let d = e.delta.pixel_delta(window.line_height());
            let Some(c) = v.canvases.get_mut(&cid) else { return };
            if e.modifiers.control {
                let (x, y) = local(e.position, c.origin.get());
                zoom_at(c, &pz, (f32::from(d.y) * 0.003).exp(), x, y);
            } else if let Some(view) = c.view {
                let (dx, dy) = if e.modifiers.shift && d.x == px(0.0) { (d.y, px(0.0)) } else { (d.x, d.y) };
                let (dx, dy) = (f32::from(dx) / zm, f32::from(dy) / zm);
                c.view = Some(View { x: view.x - dx / view.zoom, y: view.y - dy / view.zoom, ..view });
                c.moved = true;
            }
            send_view(v, &cid);
            cx.stop_propagation();
            cx.notify();
        }));
        let cid = id.clone();
        el = el.on_click(cx.listener(move |v, e: &ClickEvent, _, cx| {
            if e.click_count() < 2 {
                return;
            }
            let Some(c) = v.canvases.get_mut(&cid) else { return };
            c.view = Some(fit(c, &pz));
            c.moved = false;
            send_view(v, &cid);
            cx.notify();
        }));
    } else if sense.wheel {
        let cid = id.clone();
        el = el.on_scroll_wheel(cx.listener(move |v, e: &ScrollWheelEvent, window, cx| {
            let d = e.delta.pixel_delta(window.line_height());
            let Some(c) = v.canvases.get(&cid) else { return };
            let (x, y) = to_world(e.position, c.origin.get(), c.view.unwrap_or(View { x: 0.0, y: 0.0, zoom: 1.0 }), c.w, c.h);
            let ev = if e.modifiers.control {
                Event::Zoom { id: cid.clone(), factor: (f32::from(d.y) * 0.003).exp(), x, y }
            } else {
                Event::Wheel { id: cid.clone(), dx: f32::from(d.x) / zm, dy: f32::from(d.y) / zm, x, y }
            };
            v.send_events(vec![ev], Some(mods(&e.modifiers)));
            cx.stop_propagation();
        }));
    }

    // Its size and place, from layout: a new size refits the view and tells the module.
    let weak = cx.entity().downgrade();
    let shapes = v.canvases[&id].shapes.clone();
    let mono = SharedString::from(b.mono.clone());
    let cid = id.clone();
    let painter = gpui::canvas(
        move |bounds, _, cx| {
            origin.set(bounds.origin);
            let (nw, nh) = (f32::from(bounds.size.width) / zm, f32::from(bounds.size.height) / zm);
            if (nw, nh) != (w, h) {
                let _ = weak.update(cx, |v, cx| {
                    if let Some(c) = v.canvases.get_mut(&cid) {
                        c.w = nw;
                        c.h = nh;
                    }
                    settle(v, &cid, pz.as_ref());
                    if pz.is_some() {
                        send_view(v, &cid);
                    }
                    cx.notify();
                });
            }
            bounds
        },
        move |_, bounds: Bounds<Pixels>, window, cx| {
            window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| paint(&shapes, bounds, view, unit, zm, &mono, window, cx));
        },
    )
    .absolute()
    .size_full();

    let pictures = pictures(b, &v.canvases[&id].shapes, view, unit, w, h, zm);
    el.child(painter).children(pictures).into_any_element()
}

/// Paints the shapes. Plain fills and lines in a row are batched by colour (one path
/// each), drawn in the order each first appeared; anything else ends the run.
#[allow(clippy::too_many_arguments)]
fn paint(shapes: &[Shape], bounds: Bounds<Pixels>, view: View, unit: bool, zm: f32, mono: &SharedString, window: &mut Window, cx: &mut App) {
    let (ox, oy) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    let (bw, bh) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    // World → window pixels: the view's zoom times the app's.
    let (cw, ch, z) = (bw / zm, bh / zm, view.zoom * zm);
    let sx = |n: f32| ((if unit { n * cw } else { n }) - view.x) * z + ox;
    let sy = |n: f32| ((if unit { n * ch } else { n }) - view.y) * z + oy;
    let sw = |n: f32| (if unit { n * cw } else { n }) * z;
    let sh = |n: f32| (if unit { n * ch } else { n }) * z;
    let visible = |x: f32, y: f32, w: f32, h: f32| x + w >= ox && y + h >= oy && x <= ox + bw && y <= oy + bh;
    let p = |x: f32, y: f32| point(px(x), px(y));

    // Batched strokes and fills: key → (colour, builder), in first-seen order.
    let mut order: Vec<(u64, Hsla)> = Vec::new();
    let mut paths: HashMap<u64, PathBuilder> = HashMap::new();
    let flush = |order: &mut Vec<(u64, Hsla)>, paths: &mut HashMap<u64, PathBuilder>, window: &mut Window| {
        for (key, color) in order.drain(..) {
            if let Some(path) = paths.remove(&key).and_then(|b| b.build().ok()) {
                window.paint_path(path, color);
            }
        }
    };
    let builder = |order: &mut Vec<(u64, Hsla)>, paths: &mut HashMap<u64, PathBuilder>, stroke: Option<f32>, color: u32| -> u64 {
        let key = match stroke {
            Some(w) => (u64::from(color) << 32) | u64::from(w.to_bits() | 1),
            None => u64::from(color) << 32,
        };
        paths.entry(key).or_insert_with(|| {
            order.push((key, hsla(color)));
            match stroke {
                Some(w) => PathBuilder::stroke(px((w * z).max(0.5))),
                None => PathBuilder::fill(),
            }
        });
        key
    };

    for s in shapes {
        match s {
            Shape::Rect { x, y, w, h, radius, fill: f, stroke } => {
                let (x, y, w, h) = (sx(*x), sy(*y), sw(*w), sh(*h));
                if !visible(x, y, w, h) {
                    continue;
                }
                if stroke.is_none() && *radius == 0.0 {
                    let key = builder(&mut order, &mut paths, None, *f);
                    if let Some(b) = paths.get_mut(&key) {
                        b.add_polygon(&[p(x, y), p(x + w, y), p(x + w, y + h), p(x, y + h)], true);
                    }
                    continue;
                }
                flush(&mut order, &mut paths, window);
                let mut q = fill(Bounds::new(p(x, y), size(px(w), px(h))), hsla(*f)).corner_radii(px(radius * z));
                if let Some((width, color)) = stroke {
                    q = q.border_widths(px(*width * zm)).border_color(hsla(*color));
                }
                window.paint_quad(q);
            }
            Shape::Circle { x, y, r, fill: f } => {
                let (r, x, y) = (r * z, sx(*x), sy(*y));
                if r < 0.2 || !visible(x - r, y - r, 2.0 * r, 2.0 * r) {
                    continue;
                }
                flush(&mut order, &mut paths, window);
                window.paint_quad(fill(Bounds::new(p(x - r, y - r), size(px(2.0 * r), px(2.0 * r))), hsla(*f)).corner_radii(px(r)));
            }
            Shape::Line { x1, y1, x2, y2, width, color } => {
                let key = builder(&mut order, &mut paths, Some(*width), *color);
                if let Some(b) = paths.get_mut(&key) {
                    b.move_to(p(sx(*x1), sy(*y1)));
                    b.line_to(p(sx(*x2), sy(*y2)));
                }
            }
            Shape::Path { points, closed, fill: f, stroke } => {
                if points.is_empty() {
                    continue;
                }
                let pts: Vec<Point<Pixels>> = points.iter().map(|(x, y)| p(sx(*x), sy(*y))).collect();
                if let Some(f) = f
                    && pts.len() > 2
                {
                    let key = builder(&mut order, &mut paths, None, *f);
                    if let Some(b) = paths.get_mut(&key) {
                        b.add_polygon(&pts, true);
                    }
                }
                if let Some((width, color)) = stroke
                    && pts.len() > 1
                {
                    let key = builder(&mut order, &mut paths, Some(*width), *color);
                    if let Some(b) = paths.get_mut(&key) {
                        b.move_to(pts[0]);
                        for q in &pts[1..] {
                            b.line_to(*q);
                        }
                        if *closed {
                            b.line_to(pts[0]);
                        }
                    }
                }
            }
            Shape::Text { x, y, text, size: s, color } => {
                let (s, x, y) = (s * z, sx(*x), sy(*y));
                if s < 2.0 || !visible(x, y, 0.6 * s * text.chars().count() as f32, s * 1.4) || text.contains('\n') {
                    continue;
                }
                flush(&mut order, &mut paths, window);
                let run = TextRun { len: text.len(), font: gpui::font(mono.clone()), color: hsla(*color), background_color: None, underline: None, strikethrough: None };
                let line = window.text_system().shape_line(SharedString::from(text.clone()), px(s), &[run], None);
                let _ = line.paint(p(x, y), px(s * 1.25), gpui::TextAlign::Left, None, window, cx);
            }
            Shape::Icon { x, y, size: s, name, color } => {
                let (s, x, y) = (s * z, sx(*x), sy(*y));
                if s < 2.0 || !visible(x - s / 2.0, y - s / 2.0, s, s) {
                    continue;
                }
                flush(&mut order, &mut paths, window);
                let color = if *color == 0 { window.text_style().color } else { hsla(*color) };
                let run = TextRun { len: name.len(), font: gpui::font(mono.clone()), color, background_color: None, underline: None, strikethrough: None };
                let fs = s * 0.5;
                let line = window.text_system().shape_line(SharedString::from(name.clone()), px(fs), &[run], None);
                let lw = f32::from(line.width);
                let _ = line.paint(p(x - lw / 2.0, y - fs * 0.6), px(fs * 1.2), gpui::TextAlign::Left, None, window, cx);
            }
            // Drawn as elements over the paint (`pictures`).
            Shape::Image { .. } => {}
        }
    }
    flush(&mut order, &mut paths, window);
}

/// The canvas's pictures, as image elements placed in its pixels.
#[allow(clippy::too_many_arguments)]
fn pictures(b: &Build, shapes: &[Shape], view: View, unit: bool, w: f32, h: f32, zm: f32) -> Vec<AnyElement> {
    let Some(dir) = b.images_dir.clone() else { return Vec::new() };
    let z = view.zoom * zm;
    let sx = |n: f32| ((if unit { n * w } else { n }) - view.x) * z;
    let sy = |n: f32| ((if unit { n * h } else { n }) - view.y) * z;
    let sw = |n: f32| (if unit { n * w } else { n }) * z;
    let sh = |n: f32| (if unit { n * h } else { n }) * z;
    let mut out = Vec::new();
    for s in shapes {
        let Shape::Image { x, y, w: iw, h: ih, src, fit, rounding, opacity } = s else { continue };
        let key = match src {
            ImageSrc::Handle(n) => format!("handle:{n}"),
            other => other.key(),
        };
        let Some(shown) = b.images.get(&key) else { continue };
        let (x, y, bw, bh) = (sx(*x), sy(*y), sw(*iw), sh(*ih));
        if x + bw < 0.0 || y + bh < 0.0 || x > w * zm || y > h * zm {
            continue;
        }
        let object_fit = match fit {
            Fit::Contain => ObjectFit::Contain,
            Fit::Cover => ObjectFit::Cover,
            Fit::Fill => ObjectFit::Fill,
            Fit::None => ObjectFit::None,
        };
        let mut pic = div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(bw))
            .h(px(bh))
            .overflow_hidden()
            .rounded(px(rounding * z))
            .child(img(dir.join(&shown.file)).size_full().object_fit(object_fit));
        if *opacity < 1.0 {
            pic = pic.opacity(*opacity);
        }
        out.push(pic.into_any_element());
    }
    out
}

