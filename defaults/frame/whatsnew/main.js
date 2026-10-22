// The frame's "What's new": a window over the live app, shown once after an
// update that brings one (the app opens it when shared "frame.whatsnew" isn't
// its key yet; people who just got the welcome never see it) and with
// `app.whats_new`. Props from the app: { key }. Closing it marks it seen.
// Only its own close ends it: the app keeps its frame windows through a
// layout switch. A new release replaces this page's content.
// This is the app's own mod: rewritten from the binary on every start.

import { View, div, svg, image } from "gpui-kit";
import { v_flex, h_flex, Scrollbar } from "gpui-base";
import { run, theme, props, closeOverlay, setShared, shared, keys } from "herdr";

const W = (name) => `mods/workspace-chrome/${name}.svg`;
const REMOVED = ["zen", "t4", "ade"];

// The key bound to a command, as people press it: "Ctrl+Shift+P".
function bound(action) {
  const k = keys().find((b) => b.section === "global" && b.action === action);
  return k ? k.keys.split("-").map((p) => (p === "cmd" ? "\u2318" : p.charAt(0).toUpperCase() + p.slice(1))).join("+") : null;
}

export default class WhatsNew extends View {
  init(_props, cx) {
    this.handle = cx.focus_handle();
    this.handle.focus();
    cx.timer.after(600, () => this.handle.focus());
  }

  done() {
    setShared("frame.whatsnew", props()?.key ?? true);
    closeOverlay(props().overlay);
  }

  para(t, text) {
    return div().w_full().text_size(13).line_height("20px").text_color(t.muted).child(text);
  }

  button(t, id, label, primary, onClick) {
    return h_flex()
      .id(id)
      .h(30)
      .px(12)
      .items_center()
      .rounded(8)
      .cursor_pointer()
      .text_size(12.5)
      .font_medium()
      .when(primary, (el) => el.bg(t.solid).text_color(t.solid_foreground).hover((s) => s.opacity(0.88)))
      .when(!primary, (el) => el.border_1().border_color(t.input).text_color(t.foreground).hover((s) => s.bg(t.hover)))
      .on_mouse_down("left", (_e, cx) => cx.stop_propagation())
      .on_click(onClick)
      .child(label);
  }

  // The Herdr layout drawn small, in the theme's colors.
  sketch(t) {
    const row = () => div().flex().flex_row();
    const col = () => div().flex().flex_col();
    const line = (w, c = t.dim) => div().h(3).w(w).flex_none().rounded(2).bg(c);
    const space = (c, w, on) => row().h(12).px(4).gap(4).items_center().rounded(3).when(on, (el) => el.bg(t.selected)).child(div().size(6).rounded(2).bg(c)).child(line(w, t.muted));
    const card = (lines) =>
      col()
        .flex_1()
        .rounded(5)
        .border_1()
        .border_color(t.border)
        .bg(t.background)
        .child(row().h(9).px(4).items_center().border_b(1).border_color(t.border).child(line(18)))
        .child(col().gap(3).p(5).children(lines.map((w) => line(w, t.dim))));
    return row()
      .flex_none()
      .w(250)
      .h(150)
      .rounded(10)
      .overflow_hidden()
      .border_1()
      .border_color(t.border)
      .bg(t.canvas ?? t.background)
      .child(
        col()
          .w(74)
          .p(5)
          .gap(2)
          .bg(t.chrome ?? t.surface)
          .border_r(1)
          .border_color(t.border)
          .child(line(26, t.dim))
          .child(div().h(4))
          .child(space("#22a99a", 30, true))
          .child(row().h(10).pl(12).items_center().child(line(24, t.dim)))
          .child(space("#e3814f", 26))
          .child(space("#5b8def", 34))
          .child(div().h(6))
          .child(line(20, t.dim))
          .child(row().h(10).gap(3).items_center().child(div().size(5).rounded_full().bg(t.working)).child(line(32, t.muted))),
      )
      .child(
        col()
          .flex_1()
          .child(
            row()
              .h(16)
              .px(5)
              .gap(3)
              .items_center()
              .border_b(1)
              .border_color(t.border)
              .child(div().w(22).h(9).rounded(3).bg(t.raised).border_1().border_color(t.border))
              .child(div().w(18).h(9).rounded(3))
              .child(div().w(18).h(9).rounded(3)),
          )
          .child(row().flex_1().p(5).gap(5).child(card([40, 28, 34, 20])).child(card([30, 36]))),
      );
  }

  card(t, icon, title, body, action) {
    return v_flex()
      .flex_1()
      .min_w(0)
      .gap(8)
      .p(14)
      .rounded(12)
      .border_1()
      .border_color(t.border)
      .bg(t.background)
      .child(
        h_flex()
          .gap(10)
          .items_center()
          .child(icon)
          .child(div().flex_1().min_w(0).text_size(14).font_semibold().text_color(t.foreground).child(title)),
      )
      .child(this.para(t, body))
      .child(div().flex_1())
      .when(Boolean(action), (el) => el.child(h_flex().child(action)));
  }

  glyph(t, name) {
    return div()
      .flex_none()
      .w(30)
      .h(30)
      .rounded(8)
      .bg(t.raised)
      .border_1()
      .border_color(t.border)
      .flex()
      .items_center()
      .justify_center()
      .child(svg(W(name)).size(15).text_color(t.accent));
  }

  body(t) {
    const pal = bound("app.palette");
    const old = REMOVED.includes(shared("layout"));
    return v_flex()
      .gap(14)
      .child(
        h_flex()
          .gap(20)
          .items_center()
          .child(
            v_flex()
              .flex_1()
              .min_w(0)
              .gap(8)
              .child(div().text_size(20).font_semibold().text_color(t.foreground).child("The Herdr layout is the new default"))
              .child(this.para(t, "Spaces on the left, tabs on top: the Herdr you know. Your agents and the helpers they start sit under your spaces, every pane is a card, and Files and Changes panels open from the tab bar."))
              .when(old, (el) => el.child(h_flex().pt(4).child(this.button(t, "wn-main", "Switch to Herdr", true, () => run("layout.switch", { name: "main" }))))),
          )
          .child(this.sketch(t)),
      )
      .child(
        div()
          .p(12)
          .rounded(10)
          .bg(t.raised)
          .child(this.para(t, "Your terminals kept running through the update. Your spaces and tabs start fresh in this version: they come back grouped as before and named after their folders, so rename any you like.")),
      )
      .child(
        h_flex()
          .gap(12)
          .items_stretch()
          .child(
            this.card(
              t,
              this.glyph(t, "server"),
              "Your other machines",
              "Add a server or a Mac mini from your ssh config, in one click. Its spaces show in the same sidebar, and its agents keep working while your laptop sleeps.",
              this.button(t, "wn-machine", "Add a machine", false, () => run("mod.open", { name: "machine-add" })),
            ),
          )
          .child(
            this.card(
              t,
              this.glyph(t, "git-branch"),
              "Spaces and worktrees, the Herdr way",
              "New spaces are named after the repo they're in and new tabs are numbered, until you rename them. The sidebar shows each branch, unpushed commits and uncommitted changes. Right-click a space for New and Open worktree.",
              null,
            ),
          ),
      )
      .child(
        h_flex()
          .gap(16)
          .p(14)
          .items_center()
          .rounded(12)
          .border_1()
          .border_color(t.border)
          .bg(t.background)
          .child(image("layouts/win98.svg").w(192).h(120).rounded(8).flex_none())
          .child(
            v_flex()
              .flex_1()
              .min_w(0)
              .gap(8)
              .child(div().text_size(14).font_semibold().text_color(t.foreground).child("New: Herdr 98"))
              .child(this.para(t, `A whole desktop in one layout: a glimpse of how far you can take Herdr. Every part of it is a file you or your agent can change. Try it from Settings \u203a Look${pal ? ` or the command palette (${pal})` : ""}.`)),
          ),
      )
      .child(
        h_flex()
          .gap(12)
          .items_stretch()
          .child(
            this.card(
              t,
              this.glyph(t, "monitor"),
              "Light and dark",
              "Themes follow your system now, or pick one. The window also opens at the size you left it.",
              this.button(t, "wn-look", "Open Settings", false, () => run("app.settings", { section: "look" })),
            ),
          )
          .child(
            this.card(
              t,
              this.glyph(t, "x"),
              "No longer built in: Zen, T4 and ADE",
              "If you used one, switch to Herdr in Settings \u203a Look, or ask your agent to rebuild it from your layout file.",
              null,
            ),
          ),
      );
  }

  render() {
    const t = theme();
    return v_flex()
      .id("whatsnew")
      .track_focus(this.handle)
      .size_full()
      .bg(t.popover)
      .on_key_down((e, cx) => {
        if (e.key === "escape" || e.key === "enter") {
          this.done();
          cx.stop_propagation();
        }
      })
      .child(
        h_flex()
          .flex_none()
          .h(52)
          // Only the header drags the window, and only it shows the grab hand.
          .cursor_grab()
          .pl(20)
          .pr(12)
          .gap(10)
          .items_center()
          .border_b(1)
          .border_color(t.border)
          .child(image("frame/welcome/herdr.png").w(24).h(24).rounded(6).flex_none())
          .child(div().text_size(14).font_semibold().text_color(t.foreground).child("What's new in Herdr"))
          .child(div().flex_1())
          .child(
            div()
              .id("wn-close")
              .w(26)
              .h(26)
              .rounded(7)
              .flex()
              .items_center()
              .justify_center()
              .text_color(t.muted)
              .cursor_pointer()
              .hover((s) => s.bg(t.hover).text_color(t.foreground))
              .on_mouse_down("left", (_e, cx) => cx.stop_propagation())
              .on_click(() => this.done())
              .child(svg(W("x")).size(14)),
          ),
      )
      .child(
        div()
          .relative()
          .flex_1()
          .min_h(0)
          .on_mouse_down("left", (_e, cx) => cx.stop_propagation())
          .child(
            div()
              .id("wn-body")
              .size_full()
              .overflow_y_scroll()
              .child(div().px(22).pt(20).pb(20).child(this.body(t))),
          )
          .child(Scrollbar.vertical("wn-body").mode("always").absolute().inset_0()),
      )
      .child(
        h_flex()
          .flex_none()
          .px(16)
          .py(12)
          .gap(8)
          .items_center()
          .border_t(1)
          .border_color(t.border)
          .on_mouse_down("left", (_e, cx) => cx.stop_propagation())
          .child(div().pl(6).flex_1().text_size(12).text_color(t.dim).child("Something off or missing? Settings \u203a Send feedback."))
          .child(this.button(t, "wn-done", "Got it", true, () => this.done())),
      );
  }
}
