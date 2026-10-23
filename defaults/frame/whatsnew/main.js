// The frame's "What's new": a window over the live app, shown once after an
// update that brings one (the app opens it when shared "frame.whatsnew" isn't
// its key yet; people who just got the welcome never see it) and with
// `app.whats_new`. Props from the app: { key }. Closing it marks it seen.
// Only its own close ends it: the app keeps its frame windows through a
// layout switch. A new release replaces this page's content.
// This is the app's own mod: rewritten from the binary on every start.

import { View, div, svg, image } from "gpui-kit";
import { v_flex, h_flex, Scrollbar } from "gpui-base";
import { theme, props, closeOverlay, setShared } from "herdr";

const W = (name) => `mods/workspace-chrome/${name}.svg`;

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


  // The agents sidebar drawn small, in the theme's colors: a pinned agent on
  // top, then the priority sections.
  sketch(t) {
    const section = (label) => h_flex().h(20).px(8).items_center().text_size(10).font_medium().text_color(t.dim).child(label);
    const dot = (c) => div().size(6).mx(2).flex_none().rounded_full().bg(c);
    const agent = (name, where, status, opts = {}) =>
      h_flex()
        .h(22)
        .px(8)
        .gap(5)
        .items_center()
        .rounded(5)
        .when(Boolean(opts.on), (el) => el.bg(t.selected))
        .child(h_flex().flex_1().min_w(0).text_size(11).child(div().text_color(t.foreground).child(name)).child(div().truncate().text_color(t.dim).child(`\u00a0\u00b7 ${where}`)))
        .when(Boolean(opts.pin), (el) => el.child(svg(W("pin")).size(10).flex_none().text_color(t.muted)))
        .child(status);
    const needs = div().flex_none().px(4).h(13).rounded(4).bg(t.blocked + "26").text_size(8.5).font_semibold().text_color(t.blocked).flex().items_center().child("Needs you");
    const spin = () => svg(W("spinner")).size(10).flex_none().text_color(t.working);
    return v_flex()
      .flex_none()
      .w(230)
      .p(6)
      .rounded(10)
      .overflow_hidden()
      .border_1()
      .border_color(t.border)
      .bg(t.chrome ?? t.surface)
      .child(
        h_flex()
          .h(22)
          .px(8)
          .items_center()
          .justify_between()
          .text_size(10.5)
          .font_medium()
          .text_color(t.dim)
          .child("Agents")
          .child(svg(W("clock")).size(11).text_color(t.dim)),
      )
      .child(section("Pinned"))
      .child(agent("claude", "api", spin(), { pin: true, on: true }))
      .child(section("Needs you"))
      .child(agent("pi", "web", needs))
      .child(section("Done"))
      .child(agent("codex", "docs", dot(t.done ?? t.accent)))
      .child(section("Working"))
      .child(agent("pi", "herdr", spin()))
      .child(agent("claude", "infra", spin()));
  }

  card(t, icon, title, body) {
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
      .child(this.para(t, body));
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

  fix(t, text) {
    return h_flex()
      .gap(8)
      .items_start()
      .child(svg(W("check")).size(13).mt(3).flex_none().text_color(t.accent))
      .child(this.para(t, text));
  }

  body(t) {
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
              .child(div().text_size(20).font_semibold().text_color(t.foreground).child("See the agents that need you first"))
              .child(this.para(t, "The agents sidebar has a second view. Click the button by its title and pick Priority: agents waiting on you on top, then the ones that just finished, then the ones still working. Grouped by space is still there.")),
          )
          .child(this.sketch(t)),
      )
      .child(
        h_flex()
          .gap(12)
          .items_stretch()
          .child(
            this.card(
              t,
              this.glyph(t, "pin"),
              "Pin the ones you're watching",
              "Hover an agent and click the pin. Pinned agents stay on top in both views until they end.",
            ),
          )
          .child(
            this.card(
              t,
              this.glyph(t, "chevron-down"),
              "Fold what you don't need",
              "Click a space or section header to fold it. Folded, it shows how many agents are inside.",
            ),
          ),
      )
      .child(
        this.card(
          t,
          this.glyph(t, "server"),
          "Your agents' sandboxes stay out of sight",
          "Machines an agent starts for itself (hc machine add --temp) no longer show up in your spaces or machine lists.",
        ),
      )
      .child(
        v_flex()
          .gap(6)
          .p(14)
          .rounded(12)
          .bg(t.raised)
          .child(div().pb(2).text_size(14).font_semibold().text_color(t.foreground).child("Fixed"))
          .child(this.fix(t, "The agents list keeps its place when you scroll, instead of jumping back to the top."))
          .child(this.fix(t, "An agent you're in moves to its new section right away."))
          .child(this.fix(t, "A pane closes when its terminal finishes, instead of showing \"has finished\"."))
          .child(this.fix(t, "On macOS, the red close button hides Herdr instead of leaving it without a window."))
          .child(this.fix(t, "Mods reload cleanly when you save them, and tell you when a save didn't load.")),
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
