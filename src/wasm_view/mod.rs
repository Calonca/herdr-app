//! WasmView: a WASM mod drawn natively. A WASM mod's JavaScript bridge finds (or starts)
//! the wasm-mods host and hands the pane to this view, which talks to the host itself (its
//! wire, one JSON line each way) and turns the module's tree into GPUI elements: no
//! JavaScript on the way from the module to the screen.
//!
//!   herdr.wasmView?.()               // this view's version; absent on an app without it
//!   import("herdr-components") → { WasmViewState, WasmView }
//!   this.state = WasmViewState(JSON.stringify({module, port, token, arg?, open?, images?,
//!     pane?, folder?}))              // in a live call, once
//!   render: new WasmView(this.state)
//!   this.state.lost()                // {port, why} once the host went away, else null
//!   this.state.connect(port, token)  // the host the bridge found next
//!   this.state.focus()               // the keyboard to the view (version 2): what a
//!                                    // focusable mod does on nextCommand()'s "focus"
//!
//! Finding and starting hosts stays the bridge's: it knows the protocols and the
//! installed host versions. The state owns the connection: it closes when the mod closes
//! or reloads.

mod canvas;
mod tree;

use tree::sized as tree_sized;

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, FocusHandle, KeyDownEvent, Pixels, ScrollStrategy, Subscription, Task, UniformListScrollHandle, Window, div};
use gpui_base::input::{InputEvent, InputState};
use gpui_shell::{
    ArgumentDescriptor, ArgumentSchema, COMPONENT_REGISTRY_API_VERSION, ComponentArgument, ComponentDataValue, ComponentDescriptor,
    ComponentMaterializer, ComponentPayload, ComponentRegistry, ConstructorDescriptor, FrozenComponentRegistry, MaterializeRequest,
    StateDescriptor, StateMethodDescriptor,
};
use serde::Deserialize;
use serde_json::{Value, json};
use wasm_mods_protocol::wire::{In, Shown};
use wasm_mods_protocol::{Action, Event, Mods, Move, Node};

/// What the script imports the components from.
pub const MODULE: &str = "herdr-components";

/// This view's version, as `herdr.wasmView()` says it: what a bridge may rely on
/// (1: `WasmViewState(props)`, `lost()`, `connect(port, token)`; 2: `focus()`).
pub const VERSION: u32 = 2;

/// A designed length at the app's zoom.
pub(crate) fn z(v: f32) -> Pixels {
    crate::settings::z(v)
}

/// The app's zoom: what the protocol's lengths are multiplied by on screen.
pub(crate) fn zoom() -> f32 {
    crate::settings::zoom()
}

/// The app's components: `WasmViewState` and `WasmView`.
pub fn components() -> FrozenComponentRegistry {
    let mut r = ComponentRegistry::new(COMPONENT_REGISTRY_API_VERSION, MODULE).expect("component registry");
    r.register_state(
        StateDescriptor::new("WasmViewState", "WasmViewState", vec![ArgumentDescriptor::new("props", ArgumentSchema::String)], |args, window, cx| {
            let [ComponentArgument::String(json)] = args else { return Err("WasmViewState(props: string)".into()) };
            let props: Props = serde_json::from_str(json).map_err(|e| format!("WasmViewState props: {e}"))?;
            if !wasm_mods_protocol::wire::module_name_ok(&props.module) {
                return Err(format!("WasmViewState: bad module name {:?}", props.module));
            }
            let view = cx.new(|cx| WasmView::new(props, window, cx));
            Ok(Box::new(view))
        })
        .with_methods(vec![
            StateMethodDescriptor::new::<Entity<WasmView>>("lost", "(): { port: number, why: string } | null", |entity, _, _, cx| {
                Ok(match &entity.read(cx).lost {
                    Some((port, why)) => ComponentDataValue::Object(vec![
                        ("port".into(), ComponentDataValue::Number(f64::from(*port))),
                        ("why".into(), ComponentDataValue::String(why.clone())),
                    ]),
                    None => ComponentDataValue::Null,
                })
            })
            .with_readonly(true),
            StateMethodDescriptor::new::<Entity<WasmView>>("connect", "(port: number, token: string): void", |entity, args, window, cx| {
                let [ComponentDataValue::Number(port), ComponentDataValue::String(token)] = args else {
                    anyhow::bail!("connect(port: number, token: string)")
                };
                let port = u16::try_from(*port as i64).map_err(|_| anyhow::anyhow!("connect: bad port {port}"))?;
                let token = token.clone();
                entity.update(cx, |v, cx| {
                    v.port = port;
                    v.token = token;
                    v.lost = None;
                    v.error = None;
                    v.connect(window, cx);
                    cx.notify();
                });
                Ok(ComponentDataValue::Null)
            }),
            // `pane.go` onto a focusable mod sends it "focus" (nextCommand): its bridge
            // gives the keyboard to the view, whose keys then move its lists and fields.
            StateMethodDescriptor::new::<Entity<WasmView>>("focus", "(): void", |entity, _, window, cx| {
                entity.update(cx, |v, cx| {
                    v.focus.focus(window, cx);
                    cx.notify();
                });
                Ok(ComponentDataValue::Null)
            }),
        ])
        .with_documentation("A WASM mod's connection and what it shows; pass it to `new WasmView(state)`."),
    )
    .expect("WasmViewState");
    r.register(
        ComponentDescriptor::new("WasmView", Arc::new(Materializer))
            .with_constructors(vec![ConstructorDescriptor::new(
                "WasmView",
                vec![ArgumentDescriptor::new("state", ArgumentSchema::Entity("WasmViewState"))],
                |args| Ok(ComponentPayload::new(args[0].clone())),
            )])
            .with_methods(Vec::new()),
    )
    .expect("WasmView");
    r.freeze().expect("components")
}

struct Materializer;

impl ComponentMaterializer for Materializer {
    fn materialize(&self, request: MaterializeRequest<'_>) -> anyhow::Result<AnyElement> {
        let arg = request.payload().downcast_ref::<ComponentArgument>().ok_or_else(|| anyhow::anyhow!("WasmView(state)"))?.clone();
        let view = request.with_state::<Entity<WasmView>, _>(&arg, Clone::clone)?;
        request.finish(div().size_full().child(view))
    }
}

/// What the script passes.
#[derive(Deserialize)]
struct Props {
    module: String,
    port: u16,
    token: String,
    #[serde(default)]
    arg: Option<String>,
    #[serde(default)]
    open: Option<String>,
    #[serde(default)]
    images: Option<String>,
    #[serde(default)]
    pane: Option<String>,
    #[serde(default)]
    folder: Option<String>,
}

/// What the host says that this view reads (the rest of the wire is skipped).
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum HostMsg {
    Ready {
        /// "frame" or "widget" (a dev host's bar widget, which this view doesn't draw).
        #[serde(default)]
        kind: Option<String>,
    },
    Frame {
        ui: Node,
        #[serde(default)]
        actions: Vec<Hint>,
        #[serde(default)]
        ack: u64,
        #[serde(default)]
        images: BTreeMap<String, Shown>,
    },
    Focus {
        id: String,
    },
    OpenUrl {
        url: String,
    },
    Close,
    Open {
        module: String,
        arg: String,
    },
    Error {
        message: String,
    },
}

/// An action the frame offers now: drawn as a hint under it; `key` is a key of its own.
#[derive(Clone, Deserialize)]
pub(crate) struct Hint {
    pub action: Action,
    pub label: String,
    #[serde(default)]
    pub key: Option<String>,
}

/// The last frame.
pub(crate) struct Frame {
    ui: Node,
    actions: Vec<Hint>,
    images: BTreeMap<String, Shown>,
}

pub(crate) struct Field {
    state: Entity<InputState>,
    revision: u64,
    focused: bool,
    masked: bool,
    _subscription: Subscription,
}

/// A list's selection here: it moves at once on a click or key, before the module answers.
pub(crate) struct ListSel {
    selected: Option<usize>,
    count: usize,
    local: bool,
}

/// A virtual list's scroll here, and what it last told the module.
pub(crate) struct VirtualList {
    pub handle: UniformListScrollHandle,
    /// The rows last said to show (first, count).
    pub sent: Option<(usize, usize)>,
    /// The module's last `scroll_to` revision done.
    pub revision: Option<u64>,
}

pub struct WasmView {
    props: Props,
    port: u16,
    token: String,
    out: Option<std::sync::mpsc::Sender<String>>,
    status: Option<String>,
    error: Option<String>,
    /// The host went away: its port and why, until the bridge connects another.
    lost: Option<(u16, String)>,
    frame: Option<Frame>,
    /// The room it has, in the protocol's pixels (the app's zoom taken out).
    size: Option<(f32, f32)>,
    sent_theme: String,
    sent_window: String,
    seq: u64,
    /// Sent and not answered yet: seq → when.
    sent: BTreeMap<u64, Instant>,
    /// Moves and views while an answer is due: only the latest of each, sent with the next batch.
    held: Vec<(String, Event)>,
    held_mods: Mods,
    pub(crate) fields: HashMap<String, Field>,
    pub(crate) lists: HashMap<String, ListSel>,
    pub(crate) virtual_lists: HashMap<String, VirtualList>,
    pub(crate) key_list: Option<String>,
    /// The frame's page (a list with `pages`): PageUp/PageDown and Home/End scroll it.
    page_list: Option<String>,
    field_focused: bool,
    pub(crate) canvases: HashMap<String, canvas::CanvasState>,
    pub(crate) open_popup: Option<String>,
    pub(crate) focus: FocusHandle,
    closed: bool,
    _reader: Option<Task<()>>,
}

impl Drop for WasmView {
    fn drop(&mut self) {
        self.closed = true;
    }
}

enum Incoming {
    Message(Box<HostMsg>),
    Gone(String),
}

impl WasmView {
    fn new(props: Props, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut v = WasmView {
            port: props.port,
            token: props.token.clone(),
            props,
            out: None,
            status: Some("connecting…".into()),
            error: None,
            lost: None,
            frame: None,
            size: None,
            sent_theme: String::new(),
            sent_window: String::new(),
            seq: 0,
            sent: BTreeMap::new(),
            held: Vec::new(),
            held_mods: Mods::default(),
            fields: HashMap::new(),
            lists: HashMap::new(),
            virtual_lists: HashMap::new(),
            key_list: None,
            page_list: None,
            field_focused: false,
            canvases: HashMap::new(),
            open_popup: None,
            focus: cx.focus_handle(),
            closed: false,
            _reader: None,
        };
        v.connect(window, cx);
        v
    }

    /// Opens the connection (Hello first) and starts reading.
    fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let hello = json!({ "hello": {
            "token": self.token, "module": self.props.module, "arg": self.props.arg,
            "images": self.props.images, "open": self.props.open, "native_canvas": true,
        } });
        let (tx, rx) = async_channel::unbounded::<Incoming>();
        let (out_tx, out_rx) = std::sync::mpsc::channel::<String>();
        let port = self.port;
        // Connecting, writing and reading (with the JSON parse) all happen off the UI thread.
        std::thread::spawn(move || {
            let stream = match TcpStream::connect(("127.0.0.1", port)) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send_blocking(Incoming::Gone(format!("can't reach wasm-mods-host on port {port}: {e}")));
                    return;
                }
            };
            let _ = stream.set_nodelay(true);
            let Ok(mut writer) = stream.try_clone() else { return };
            std::thread::spawn(move || {
                for line in out_rx {
                    if writer.write_all(line.as_bytes()).and_then(|_| writer.write_all(b"\n")).is_err() {
                        break;
                    }
                }
                let _ = writer.shutdown(std::net::Shutdown::Both);
            });
            let mut reader = std::io::BufReader::with_capacity(1 << 16, stream);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(e) => {
                        let _ = tx.send_blocking(Incoming::Gone(e.to_string()));
                        return;
                    }
                }
                match serde_json::from_str::<HostMsg>(&line) {
                    Ok(m) => {
                        if tx.send_blocking(Incoming::Message(Box::new(m))).is_err() {
                            return;
                        }
                    }
                    Err(e) => tracing::debug!("wasm view: a line it doesn't read: {e}"),
                }
            }
            let _ = tx.send_blocking(Incoming::Gone("the host closed the connection".into()));
        });
        self.out = Some(out_tx);
        self.sent_theme.clear();
        self.sent_window.clear();
        self.sent.clear();
        self.held.clear();
        if let Some(out) = &self.out {
            let _ = out.send(hello.to_string());
        }
        self.send_theme();
        self.send_window();
        if let Some((width, height)) = self.size {
            self.send(&In::Size { width, height });
        }
        self._reader = Some(cx.spawn_in(window, async move |this, cx| {
            while let Ok(first) = rx.recv().await {
                // Everything that's here already, then one redraw.
                let mut batch = vec![first];
                while let Ok(m) = rx.try_recv() {
                    batch.push(m);
                }
                let gone = this
                    .update_in(cx, |v, window, cx| {
                        let mut gone = None;
                        for m in batch {
                            match m {
                                Incoming::Message(m) => v.receive(*m, window, cx),
                                Incoming::Gone(why) => gone = Some(why),
                            }
                        }
                        cx.notify();
                        gone
                    })
                    .ok()
                    .flatten();
                if let Some(why) = gone {
                    let _ = this.update(cx, |v, cx| v.lost(why, cx));
                    return;
                }
            }
        }));
    }

    /// The host went away: the bridge hears it (`lost()`), finds the next host and
    /// connects this view to it (`connect`). Meanwhile the last frame stays.
    fn lost(&mut self, why: String, cx: &mut Context<Self>) {
        self.out = None;
        self._reader = None;
        if self.closed {
            return;
        }
        // The host closed it after saying why (the module didn't load): the reason stays;
        // another try would only fail the same way.
        if self.error.is_some() {
            self.status = None;
            cx.notify();
            return;
        }
        tracing::info!("wasm view {}: {why}; waiting for the next host", self.props.module);
        self.status = Some("reconnecting…".into());
        self.lost = Some((self.port, why));
        cx.notify();
    }

    fn send(&self, msg: &In) {
        if let (Some(out), Ok(line)) = (&self.out, serde_json::to_string(msg)) {
            let _ = out.send(line);
        }
    }

    /// Events in the protocol's form; moves and views are held while an answer is due.
    pub(crate) fn send_events(&mut self, events: Vec<Event>, mods: Option<Mods>) {
        let waiting = self.sent.values().next().is_some_and(|at| at.elapsed() < Duration::from_secs(1));
        if !events.is_empty() && waiting && events.iter().all(|e| latest_only(e).is_some()) {
            for e in events {
                let key = latest_only(&e).unwrap_or_default();
                self.held.retain(|(k, _)| *k != key);
                self.held.push((key, e));
            }
            if let Some(m) = mods {
                self.held_mods = m;
            }
            return;
        }
        let mut all: Vec<Event> = std::mem::take(&mut self.held).into_iter().map(|(_, e)| e).collect();
        all.extend(events);
        if all.is_empty() || self.out.is_none() {
            return;
        }
        let mods = mods.unwrap_or(self.held_mods);
        self.seq += 1;
        self.sent.insert(self.seq, Instant::now());
        let events = all.iter().filter_map(|e| serde_json::to_value(e).ok()).collect();
        self.send(&In::Events { seq: self.seq, events, mods });
    }

    fn send_theme(&mut self) {
        let theme = protocol_theme();
        let key = theme.to_string();
        if key == self.sent_theme || self.out.is_none() {
            return;
        }
        if let Ok(t) = serde_json::from_value(theme) {
            self.sent_theme = key;
            self.send(&In::Theme(t));
        }
    }

    fn send_window(&mut self) {
        let w = self.window_context();
        let key = w.to_string();
        if key == self.sent_window || self.out.is_none() {
            return;
        }
        if let Ok(w) = serde_json::from_value(w) {
            self.sent_window = key;
            self.send(&In::Window(w));
        }
    }

    /// Where its tab works, as the protocol's WindowContext: the folder it was opened
    /// for (props: a diff opened from the git graph), else the tab's first terminal's,
    /// else its space's; null when there's none.
    fn window_context(&self) -> Value {
        crate::mods::with_snapshot(|s| {
            let panes = s.panes.as_array().cloned().unwrap_or_default();
            let mine = self.props.pane.as_ref().and_then(|p| panes.iter().find(|x| x["pane"].as_str() == Some(p.as_str())));
            let tab = mine.map(|p| p["tab"].clone()).or_else(|| s.focused.get("tab").cloned()).unwrap_or(Value::Null);
            let space = s.spaces.as_array().and_then(|a| {
                a.iter().find(|sp| sp["tabs"].as_array().is_some_and(|ts| ts.iter().any(|t| t["id"] == tab))).cloned()
            });
            let machine = space.as_ref().map_or(Value::Null, |sp| sp.get("machine").cloned().unwrap_or(Value::Null));
            if let Some(folder) = &self.props.folder {
                return json!({ "folder": folder, "branch": null, "machine": machine });
            }
            let term = panes.iter().find(|p| p["tab"] == tab && p.get("terminal").is_some_and(|t| !t.is_null()) && p["cwd"].is_string());
            let folder = match (term, &space) {
                (Some(p), _) => p["cwd"].clone(),
                (None, Some(sp)) => sp.get("folder").filter(|f| f.as_str().is_some_and(|f| !f.is_empty())).cloned().unwrap_or_else(|| json!("~")),
                (None, None) => return Value::Null,
            };
            let branch = term.and_then(|p| p.get("branch")).or_else(|| space.as_ref().and_then(|sp| sp.get("branch"))).filter(|b| b.is_string()).cloned();
            json!({ "folder": folder, "branch": branch, "machine": machine })
        })
    }

    fn receive(&mut self, msg: HostMsg, window: &mut Window, cx: &mut Context<Self>) {
        match msg {
            HostMsg::Ready { kind } => {
                self.status = None;
                if kind.as_deref() == Some("widget") {
                    self.error = Some("a bar widget: this view draws only frames".into());
                }
            }
            HostMsg::Frame { mut ui, actions, ack, images } => {
                self.answered(ack);
                self.error = None;
                self.status = None;
                self.adopt(&ui, window, cx);
                canvas::take_shapes(self, &mut ui);
                self.frame = Some(Frame { ui, actions, images });
            }
            HostMsg::Focus { id } => {
                if let Some(f) = self.fields.get(&id) {
                    let handle = gpui::Focusable::focus_handle(f.state.read(cx), cx);
                    handle.focus(window, cx);
                }
            }
            HostMsg::OpenUrl { url } => cx.open_url(&url),
            HostMsg::Close => {
                if let Some(pane) = &self.props.pane {
                    crate::mods::request(crate::mods::Request::Run { name: "pane.close".into(), args: json!({ "pane": pane }) });
                }
            }
            HostMsg::Open { module, arg } => {
                let space = crate::mods::with_snapshot(|s| s.focused.get("space").cloned()).unwrap_or(Value::Null);
                let folder = self.window_context().get("folder").cloned().unwrap_or(Value::Null);
                // Its own mod folder if it has one (wasm-diff), else the core mod runs it.
                let own = format!("wasm-{}", module.replace('_', "-"));
                let args = if crate::mods::app_dir().join("mods").join(&own).join("mod.json").is_file() {
                    json!({ "space": space, "mod": own, "props": { "arg": arg, "folder": folder } })
                } else {
                    json!({ "space": space, "mod": "wasm-mods", "props": { "module": module, "arg": arg, "folder": folder } })
                };
                crate::mods::request(crate::mods::Request::Run { name: "tab.new".into(), args });
            }
            HostMsg::Error { message } => self.error = Some(message),
        }
    }

    /// An answer covers every batch sent up to its ack; held moves go out now.
    fn answered(&mut self, ack: u64) {
        self.sent.retain(|seq, _| *seq > ack);
        if self.sent.is_empty() && !self.held.is_empty() {
            self.send_events(Vec::new(), None);
        }
    }

    /// Text fields and lists in the new tree: states for new fields, new text when the
    /// module changed a revision, scroll handles for virtual lists.
    fn adopt(&mut self, ui: &Node, window: &mut Window, cx: &mut Context<Self>) {
        let mut seen = Vec::new();
        let mut listed = Vec::new();
        let mut first_key_list = None;
        let mut page_list = None;
        let mut nodes = vec![ui];
        while let Some(n) = nodes.pop() {
            match n {
                Node::TextField { id, value, revision, password, placeholder, .. } => {
                    seen.push(id.clone());
                    if let Some(f) = self.fields.get_mut(id) {
                        // Up: the module set the text. Down: a new session (a reconnect)
                        // whose module counts from the start again; edits must carry its
                        // revision or it drops them all.
                        if *revision != f.revision {
                            f.revision = *revision;
                            let value = value.clone();
                            f.state.update(cx, |s, cx| s.set_value(value, window, cx));
                        }
                    } else {
                        let (placeholder, value, field) = (placeholder.clone(), value.clone(), id.clone());
                        let state = cx.new(|cx| {
                            let mut s = InputState::new(window, cx).placeholder(placeholder);
                            s.set_value(value, window, cx);
                            s
                        });
                        let sub = cx.subscribe_in(&state, window, move |v: &mut WasmView, state, e: &InputEvent, _window, cx| {
                            let id = field.clone();
                            match e {
                                InputEvent::Change => {
                                    let value = state.read(cx).value().to_string();
                                    let revision = v.fields.get(&id).map_or(0, |f| f.revision);
                                    v.send_events(vec![Event::Edited { id, value, revision }], None);
                                }
                                InputEvent::PressEnter { .. } => v.send_events(vec![Event::Submitted { id }], None),
                                InputEvent::Focus | InputEvent::Blur => {
                                    let focused = matches!(e, InputEvent::Focus);
                                    if let Some(f) = v.fields.get_mut(&id) {
                                        f.focused = focused;
                                    }
                                    v.field_focused = focused;
                                    v.send_events(vec![Event::Focus { id, focused }], None);
                                    cx.notify();
                                }
                                _ => {}
                            }
                        });
                        self.fields.insert(id.clone(), Field { state, revision: *revision, focused: false, masked: false, _subscription: sub });
                    }
                    if let Some(f) = self.fields.get_mut(id)
                        && f.masked != *password
                    {
                        let masked = *password;
                        f.masked = masked;
                        f.state.update(cx, |s, cx| s.set_masked(masked, window, cx));
                    }
                }
                Node::List { id, items, selected, keyboard, window: w, pages, .. } => {
                    listed.push(id.clone());
                    // A selection made here and not answered yet stays (no flicker back).
                    let pending = !self.sent.is_empty() && self.lists.get(id).is_some_and(|l| l.local);
                    let mine = self.lists.get(id).and_then(|l| l.selected);
                    let count = w.as_ref().map_or(items.len(), |w| w.total as usize);
                    self.lists.insert(id.clone(), ListSel { selected: if pending { mine } else { *selected }, count, local: pending });
                    if *keyboard && first_key_list.is_none() {
                        first_key_list = Some(id.clone());
                    }
                    if w.is_some() {
                        self.virtual_lists.entry(id.clone()).or_insert_with(|| VirtualList {
                            handle: UniformListScrollHandle::new(),
                            sent: None,
                            revision: None,
                        });
                        if *pages && page_list.is_none() {
                            page_list = Some(id.clone());
                        }
                    }
                    nodes.extend(items.iter().filter_map(|it| it.child.as_deref()));
                }
                Node::Column { children, .. } | Node::Row { children, .. } => nodes.extend(children.iter().rev()),
                Node::Popup { child, .. } => nodes.push(child),
                _ => {}
            }
        }
        self.fields.retain(|id, _| seen.contains(id));
        if self.fields.is_empty() {
            self.field_focused = false;
        }
        // A list gone from the frame no longer takes keys; a virtual one keeps its scroll
        // (back, it is where it was).
        self.lists.retain(|id, _| listed.contains(id));
        self.page_list = page_list;
        if self.key_list.as_ref().is_none_or(|k| !self.lists.contains_key(k)) {
            self.key_list = first_key_list;
        }
    }

    /// Scrolls the frame's page list a page up or down, or to its start or end; false if
    /// the frame has none. The module hears where it is (VisibleRange).
    fn page_by(&mut self, mv: Move) -> bool {
        let Some(id) = &self.page_list else { return false };
        let Some(vl) = self.virtual_lists.get(id) else { return false };
        let total = self.lists.get(id).map_or(0, |l| l.count);
        if total == 0 {
            return true;
        }
        let (first, count) = vl.sent.unwrap_or((0, 0));
        // The range counts rows partly showing at both ends: a page keeps a row or so.
        let page = count.saturating_sub(3).max(1);
        let mid = first + count / 2;
        match mv {
            Move::PageUp => vl.handle.scroll_to_item(mid.saturating_sub(page), ScrollStrategy::Center),
            Move::PageDown => vl.handle.scroll_to_item((mid + page).min(total - 1), ScrollStrategy::Center),
            Move::Home => vl.handle.scroll_to_item(0, ScrollStrategy::Top),
            _ => vl.handle.scroll_to_item(total - 1, ScrollStrategy::Bottom),
        }
        true
    }

    /// Whether a text field has the keyboard: only fields take it inside the view.
    fn in_field(&self, window: &Window) -> bool {
        self.field_focused || (!self.fields.is_empty() && !self.focus.is_focused(window))
    }

    /// Keys: actions, never raw keys (as the JavaScript bridge does).
    fn key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = e.keystroke.key.as_str();
        let in_field = self.in_field(window);
        if in_field && !matches!(key, "escape" | "tab") {
            return;
        }
        let m = &e.keystroke.modifiers;
        if m.control || m.alt || m.platform {
            return;
        }
        let mv = match key {
            "up" => Some(Move::Up),
            "down" => Some(Move::Down),
            "pageup" => Some(Move::PageUp),
            "pagedown" => Some(Move::PageDown),
            "home" => Some(Move::Home),
            "end" => Some(Move::End),
            "left" => Some(Move::Left),
            "right" => Some(Move::Right),
            "j" if !in_field => Some(Move::Down),
            "k" if !in_field => Some(Move::Up),
            _ => None,
        };
        // A key the module offers by name ({action: {Key: name}, key: "s"}): that action.
        let named = (!in_field && !m.shift)
            .then(|| self.frame.as_ref()?.actions.iter().find(|h| matches!(h.action, Action::Key(_)) && h.key.as_deref() == Some(key)).map(|h| h.action.clone()))
            .flatten();
        let list = self.key_list.clone().filter(|k| self.lists.get(k).is_some_and(|l| l.count > 0));
        let mut events = Vec::new();
        if let Some(action) = named {
            events.push(Event::Action(action));
        } else if matches!(mv, Some(Move::PageUp | Move::PageDown | Move::Home | Move::End)) && self.page_by(mv.unwrap()) {
            cx.notify();
        } else if let (Some(mv), Some(id)) = (mv, &list)
            && !matches!(mv, Move::Left | Move::Right)
        {
            let l = self.lists.get_mut(id).unwrap();
            let cur = l.selected.map_or(-1, |s| s as i64);
            let step: i64 = match mv {
                Move::Up => -1,
                Move::Down => 1,
                Move::PageUp => -10,
                Move::PageDown => 10,
                Move::Home => -1_000_000_000,
                _ => 1_000_000_000,
            };
            let next = (cur + step).clamp(0, l.count as i64 - 1) as usize;
            l.selected = Some(next);
            l.local = true;
            // A virtual list scrolls to it (the others are scrolled by their container).
            if let Some(vl) = self.virtual_lists.get(id) {
                vl.handle.scroll_to_item(next, ScrollStrategy::Nearest);
            }
            events.push(Event::Selected { id: id.clone(), index: next });
            cx.notify();
        } else if let Some(mv) = mv {
            events.push(Event::Action(Action::Move(mv)));
        } else if key == "enter"
            && let Some(id) = &list
            && let Some(index) = self.lists[id].selected
        {
            events.push(Event::Activated { id: id.clone(), index });
        } else if key == "escape" {
            events.push(match &self.open_popup {
                Some(id) => Event::Dismissed { id: id.clone() },
                None => Event::Action(Action::Back),
            });
            if in_field {
                self.focus.focus(window, cx);
            }
        } else if key == "enter" {
            events.push(Event::Action(Action::Open));
        } else if key == "space" {
            events.push(Event::Action(Action::Primary));
        } else if key == "tab" && !in_field {
            events.push(Event::Action(Action::Secondary));
        }
        if !events.is_empty() {
            self.send_events(events, None);
            cx.stop_propagation();
        }
    }
}

/// For an event of which only the latest counts (a pointer move or drag, a view): what
/// it replaces is keyed by this.
fn latest_only(e: &Event) -> Option<String> {
    match e {
        Event::Pointer { id, kind: k @ (wasm_mods_protocol::PointerKind::Move | wasm_mods_protocol::PointerKind::Drag), .. } => Some(format!("p:{id}:{k:?}")),
        Event::View { id, .. } => Some(format!("v:{id}")),
        Event::VisibleRange { id, .. } => Some(format!("r:{id}")),
        _ => None,
    }
}

/// "#rrggbb[aa]" → 0xRRGGBBAA.
fn rgba(hex: &str) -> u32 {
    let h = hex.trim_start_matches('#');
    let full = if h.len() == 6 { format!("{h}ff") } else { h.chars().take(8).collect() };
    u32::from_str_radix(&full, 16).unwrap_or(0xff00ffff)
}

fn theme_rgba(name: &str) -> u32 {
    crate::settings::theme().get(name).map_or(0xff00ffff, |v| rgba(v))
}

/// The app's theme as the protocol's.
fn protocol_theme() -> Value {
    let s = crate::settings::get();
    json!({
        "name": s.theme, "space": theme_rgba("background"), "surface": theme_rgba("surface"),
        "text": theme_rgba("foreground"), "subtext": theme_rgba("muted"), "muted": theme_rgba("dim"),
        "accent": theme_rgba("accent"), "border": theme_rgba("border"),
        "ansi": (0..16).map(|i| theme_rgba(&format!("ansi{i}"))).collect::<Vec<_>>(),
        "fonts": { "ui": "", "mono": s.font_family, "system_ui": "", "ui_choices": [] },
    })
}

impl Render for WasmView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.send_theme();
        self.send_window();
        let t = Colors::now();
        let root = div()
            .id("wasm-view")
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|v, e: &KeyDownEvent, window, cx| v.key(e, window, cx)))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|v, _, window, cx| {
                if !v.field_focused {
                    v.focus.focus(window, cx);
                }
            }))
            .child(size_probe(cx.entity().downgrade()))
            .text_color(t.fg)
            .text_size(z(13.0));
        if let Some(err) = &self.error {
            return root
                .p(z(14.0))
                .gap(z(6.0))
                .child(div().text_size(z(14.0)).font_weight(gpui::FontWeight::SEMIBOLD).child(self.props.module.clone()))
                .child(div().text_color(t.danger).child(err.clone()))
                .into_any_element();
        }
        let Some(frame) = self.frame.take() else {
            return root
                .p(z(14.0))
                .child(div().text_color(t.dim).child(self.status.clone().unwrap_or_else(|| "loading…".into())))
                .into_any_element();
        };
        self.open_popup = None;
        let mut b = tree::Build {
            popups: Vec::new(),
            images: &frame.images,
            t: &t,
            mono: crate::settings::get().font_family.clone(),
            images_dir: self.props.images.as_ref().map(std::path::PathBuf::from),
        };
        let body = tree::node(self, &mut b, &frame.ui, tree::Ctx::default(), window, cx);
        let popups = std::mem::take(&mut b.popups);
        let hints = (!frame.actions.is_empty()).then(|| tree::hints(&frame.actions, &t, cx));
        // The frame scrolls, unless a virtual list in it does (what's wider is cut).
        let content = div().id("wasm-view-scroll").flex().flex_col().flex_1().min_h_0().p(z(14.0)).child(body);
        let content = if tree::holds_virtual(&frame.ui) { content.overflow_hidden() } else { content.overflow_y_scroll() };
        self.frame = Some(frame);
        let reconnecting = self.lost.is_some().then(|| {
            div().absolute().top(z(6.0)).right(z(10.0)).px(z(6.0)).rounded(z(4.0)).bg(t.raised).text_size(z(11.0)).text_color(t.dim).child("reconnecting…")
        });
        root.relative().bg(t.bg).child(content).children(hints).children(popups).children(reconnecting).into_any_element()
    }
}

/// Tells the view its size when it changes (In::Size), in the protocol's pixels.
fn size_probe(view: gpui::WeakEntity<WasmView>) -> impl IntoElement {
    gpui::canvas(
        move |bounds, _, cx| {
            let zm = zoom();
            let size = (f32::from(bounds.size.width) / zm, f32::from(bounds.size.height) / zm);
            let _ = view.update(cx, |v, _| {
                if v.size != Some(size) {
                    v.size = Some(size);
                    v.send(&In::Size { width: size.0, height: size.1 });
                }
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

/// The theme's colours, read once per render.
pub(crate) struct Colors {
    pub bg: gpui::Hsla,
    pub fg: gpui::Hsla,
    pub muted: gpui::Hsla,
    pub dim: gpui::Hsla,
    pub accent: gpui::Hsla,
    pub accent_fg: gpui::Hsla,
    pub border: gpui::Hsla,
    pub surface: gpui::Hsla,
    pub popover: gpui::Hsla,
    pub raised: gpui::Hsla,
    pub input: gpui::Hsla,
    pub selected: gpui::Hsla,
    pub hover: gpui::Hsla,
    pub solid: gpui::Hsla,
    pub solid_fg: gpui::Hsla,
    pub danger: gpui::Hsla,
    pub ansi: [gpui::Hsla; 8],
}

impl Colors {
    fn now() -> Self {
        let c = crate::settings::color;
        Colors {
            bg: c("background"),
            fg: c("foreground"),
            muted: c("muted"),
            dim: c("dim"),
            accent: c("accent"),
            accent_fg: c("accent_foreground"),
            border: c("border"),
            surface: c("surface"),
            popover: c("popover"),
            raised: c("raised"),
            input: c("input"),
            selected: c("selected"),
            hover: c("hover"),
            solid: c("solid"),
            solid_fg: c("solid_foreground"),
            danger: c("ansi1"),
            ansi: std::array::from_fn(|i| c(&format!("ansi{i}"))),
        }
    }
}

/// 0xRRGGBBAA as GPUI paint.
pub(crate) fn hsla(c: u32) -> gpui::Hsla {
    gpui::Rgba { r: (c >> 24) as f32 / 255.0, g: ((c >> 16) & 0xff) as f32 / 255.0, b: ((c >> 8) & 0xff) as f32 / 255.0, a: (c & 0xff) as f32 / 255.0 }.into()
}
