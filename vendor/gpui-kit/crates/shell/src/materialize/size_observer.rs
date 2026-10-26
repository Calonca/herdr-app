use std::rc::{Rc, Weak};

use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Size, Window,
};

use crate::{ComponentDataValue, engine::ShellRuntime, spec::CallbackId};

pub(super) fn observe(
    element: AnyElement,
    identity: ElementId,
    callback: Option<CallbackId>,
    runtime: &Rc<ShellRuntime>,
) -> AnyElement {
    let Some(callback) = callback else {
        return element;
    };
    SizeObserver {
        element,
        identity,
        callback,
        runtime: Rc::downgrade(runtime),
    }
    .into_any_element()
}

// Reuse the child's layout node: an absolute canvas would exclude its borders.
struct SizeObserver {
    element: AnyElement,
    identity: ElementId,
    callback: CallbackId,
    runtime: Weak<ShellRuntime>,
}

impl IntoElement for SizeObserver {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for SizeObserver {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.element.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.element.prepaint(window, cx);
        let changed = window.with_id(self.identity.clone(), |window| {
            window.with_global_id("shell-size-change".into(), |id, window| {
                window.with_element_state(id, |previous: Option<Size<Pixels>>, _| {
                    (previous != Some(bounds.size), bounds.size)
                })
            })
        });
        if changed {
            let runtime = self.runtime.clone();
            let callback = self.callback;
            // Handlers may notify or start work only after layout/paint finishes.
            window.defer(cx, move |window, cx| {
                let Some(runtime) = runtime.upgrade() else {
                    return;
                };
                let size = ComponentDataValue::Object(vec![
                    (
                        "width".into(),
                        ComponentDataValue::Number(f32::from(bounds.size.width) as f64),
                    ),
                    (
                        "height".into(),
                        ComponentDataValue::Number(f32::from(bounds.size.height) as f64),
                    ),
                ]);
                if let Err(error) =
                    runtime.dispatch_component_event_data(callback, &[size], window, cx)
                {
                    tracing::error!("error in size-change handler: {error}");
                }
            });
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.element.paint(window, cx);
    }
}
