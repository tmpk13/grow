//! The chrome for driving a settler: the stick and the row of buttons.
//!
//! It lives over the map rather than in the panel, because it is played with
//! rather than set: what somebody steering a settler needs is under their thumb
//! at the bottom of the screen, and the panel is off to the side and may be
//! folded away entirely.
//!
//! The whole thing comes and goes with `Settlement::driven`, and is rebuilt
//! only when who is being driven changes. What changes every frame - the
//! labels that swap, and how far they are from starving - is written into the
//! nodes that are already there.

use wasm_bindgen::JsCast;
use web_sys::Element;

use crate::app::{App, Handle, Shell};
use crate::civ::control::ACTS;
use crate::ui::{button, by_id, el, on, Scope};

/// Which way each of the four keys pushes.
pub const DRIVE_KEYS: [(&str, &str, f64, f64); 4] = [
    ("w", "ArrowUp", 0.0, -1.0),
    ("s", "ArrowDown", 0.0, 1.0),
    ("a", "ArrowLeft", -1.0, 0.0),
    ("d", "ArrowRight", 1.0, 0.0),
];

/// The push the keys and the stick add up to, one at full pelt. The stick wins
/// where both are being used: a thumb on the glass is a deliberate act, and a
/// key that has stuck down would otherwise fight it forever.
pub fn push(app: &App) -> (f64, f64) {
    let (sx, sy) = app.ui.stick;
    if sx != 0.0 || sy != 0.0 {
        return (sx, sy);
    }
    let mut x = 0.0;
    let mut y = 0.0;
    for (i, (_, _, dx, dy)) in DRIVE_KEYS.iter().enumerate() {
        if app.ui.drive_keys[i] {
            x += dx;
            y += dy;
        }
    }
    (x, y)
}

/// Whether the keys mean steering rather than the shortcuts they usually are.
pub fn driving(app: &App) -> bool {
    app.settlement.as_ref().is_some_and(|sim| sim.driven != 0)
}

/// Puts the chrome up, takes it down, and keeps what it says current. Called
/// once a frame; everything in it is a cheap read except the rebuild, which
/// only happens when somebody else is taken over.
pub fn sync(sh: &mut Shell, h: &Handle) {
    let wrap = match by_id("stage-hud") {
        Some(n) => n,
        None => return,
    };
    let driven = sh.app.settlement.as_ref().map(|sim| sim.driven).unwrap_or(0);
    if driven == 0 {
        if !wrap.has_attribute("hidden") {
            crate::ui::clear_scope(Scope::Hud);
            crate::ui::clear(&wrap);
            let _ = wrap.set_attribute("hidden", "hidden");
        }
        return;
    }
    let built: u32 = wrap.get_attribute("data-driven").and_then(|v| v.parse().ok()).unwrap_or(0);
    if built != driven {
        build(&wrap, sh, h, driven);
    }
    label(&sh.app);
}

/// The one line that changes as they walk about: what the two buttons that
/// swap are saying, and how they are doing.
fn label(app: &App) {
    let sim = match app.settlement.as_ref() {
        Some(s) => s,
        None => return,
    };
    let p = match sim.people.get(sim.driven) {
        Some(p) => p,
        None => return,
    };
    for act in ACTS {
        if let Some(node) = by_id(&format!("drive-{}", act.key())) {
            let want = act.label(p.carrying(), p.indoors());
            if node.text_content().unwrap_or_default() != want {
                node.set_text_content(Some(want));
            }
        }
    }
    if let Some(node) = by_id("drive-who") {
        let load = match p.carry.res {
            Some(res) if p.carry.n >= 0.5 => format!(", {:.0} {}", p.carry.n, res.label()),
            _ => String::new(),
        };
        let text = format!(
            "{} - {:.0}% fed, {:.0}% rested{load}",
            p.name,
            (1.0 - p.hunger) * 100.0,
            p.energy * 100.0
        );
        if node.text_content().unwrap_or_default() != text {
            node.set_text_content(Some(&text));
        }
    }
}

fn build(wrap: &Element, sh: &mut Shell, h: &Handle, driven: u32) {
    crate::ui::clear_scope(Scope::Hud);
    crate::ui::clear(wrap);
    let _ = wrap.set_attribute("data-driven", &driven.to_string());
    let _ = wrap.remove_attribute("hidden");

    if sh.app.state.civ.experiments.control.joystick {
        let _ = wrap.append_child(&stick(h));
    }

    let mut row = el("div").class("hud-acts").get();
    for act in ACTS {
        let h2 = h.clone();
        let press = button(act.label(false, false), Scope::Hud, move || {
            let mut sh = h2.borrow_mut();
            let sh = &mut *sh;
            let note = match sh.app.settlement.as_mut() {
                Some(sim) => crate::civ::control::act(sim, &sh.app.state, act),
                None => return,
            };
            sh.app.set_note(&note);
            sh.app.civ_stepped = true;
        });
        let _ = press.set_attribute("id", &format!("drive-{}", act.key()));
        let _ = press.set_attribute("title", &format!("or the {} key", act.key()));
        let _ = row.append_child(&press);
    }
    let h2 = h.clone();
    let go = crate::ui::danger_button("Let go", Scope::Hud, move || {
        let mut sh = h2.borrow_mut();
        let name = sh
            .app
            .settlement
            .as_ref()
            .and_then(|sim| sim.people.get(sim.driven))
            .map(|p| p.name.clone())
            .unwrap_or_default();
        if let Some(sim) = sh.app.settlement.as_mut() {
            crate::civ::control::let_go(sim);
        }
        sh.app.ui.drive_keys = [false; 4];
        sh.app.ui.stick = (0.0, 0.0);
        sh.app.set_note(&format!("{name} is the town's again"));
    });
    let _ = row.append_child(&go);
    row = el("div")
        .class("hud-row")
        .child(&el("span").class("hud-who").attr("id", "drive-who").get())
        .child(&row)
        .get();
    let _ = wrap.append_child(&row);
}

/// The stick: a ring with a knob in it that follows a thumb, and springs back
/// to the middle when it is let go. It reports where it is pushed as a fraction
/// of its own radius, so a small push is a slow walk.
fn stick(h: &Handle) -> Element {
    let knob = el("span").class("stick-knob").get();
    let pad = el("div")
        .class("stick")
        .attr("id", "drive-stick")
        .attr("role", "application")
        .attr("aria-label", "steer the settler")
        .child(&knob)
        .get();

    for event in ["pointerdown", "pointermove"] {
        let h2 = h.clone();
        let pad2 = pad.clone();
        let knob2 = knob.clone();
        on(pad.unchecked_ref(), event, Scope::Hud, move |e: web_sys::Event| {
            let pe = match e.dyn_ref::<web_sys::PointerEvent>() {
                Some(pe) => pe,
                None => return,
            };
            // A move with nothing held down is a thumb passing over, not a
            // push.
            if pe.buttons() == 0 {
                return;
            }
            e.prevent_default();
            e.stop_propagation();
            if event == "pointerdown" {
                let _ = pad2
                    .clone()
                    .dyn_into::<web_sys::Element>()
                    .map(|el| el.set_pointer_capture(pe.pointer_id()));
            }
            let rect = pad2.get_bounding_client_rect();
            let r = (rect.width().min(rect.height()) / 2.0).max(1.0);
            let dx = (pe.client_x() as f64 - (rect.left() + rect.width() / 2.0)) / r;
            let dy = (pe.client_y() as f64 - (rect.top() + rect.height() / 2.0)) / r;
            let len = dx.hypot(dy);
            let (dx, dy) = if len > 1.0 { (dx / len, dy / len) } else { (dx, dy) };
            h2.borrow_mut().app.ui.stick = (dx, dy);
            let _ = knob2.set_attribute(
                "style",
                &format!("transform: translate({:.1}%, {:.1}%)", dx * 100.0, dy * 100.0),
            );
        });
    }
    for event in ["pointerup", "pointercancel", "pointerleave"] {
        let h2 = h.clone();
        let knob2 = knob.clone();
        on(pad.unchecked_ref(), event, Scope::Hud, move |e: web_sys::Event| {
            e.stop_propagation();
            h2.borrow_mut().app.ui.stick = (0.0, 0.0);
            let _ = knob2.set_attribute("style", "transform: translate(0, 0)");
        });
    }
    pad
}
