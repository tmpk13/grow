//! Experimental panel: the things that are not finished, behind one switch.
//!
//! The switch at the top is the whole contract. With it off nothing below it
//! runs, nothing below it is asked a question, and a settlement is exactly the
//! settlement it would have been; turning it off again puts the world back the
//! way it ran. That is what makes it safe to leave something half thought out
//! in here.

use web_sys::Element;

use crate::app::{App, Handle, Panel};
use crate::civ::balloons::BalloonConfig;
use crate::ui::{app_bool, app_num, append, clear, el, note, section, stat, NumOpts};

pub struct ExperimentalPanel {
    aloft: Element,
    since: f64,
}

pub fn build(root: &Element, app: &mut App, h: &Handle) -> Box<dyn Panel> {
    let on = app.state.civ.experiments.on;
    append(
        root,
        section(
            "Experiments",
            vec![
                app_bool(
                    h,
                    "Try the unfinished things",
                    on,
                    Some("off by default, which is the settlement as it has always run"),
                    |app, v| {
                        app.state.civ.experiments.on = v;
                        app.request_save();
                        // Everything below this switch comes and goes with it,
                        // and so does the Take over control over the map.
                        app.rebuild_panel();
                        app.rebuild_toolbar = true;
                    },
                ),
                note(
                    "Nothing in here is settled. It can be turned on and off while a town runs, \
                     and with it off none of it is asked anything: no balloon is built, nothing \
                     is spent on one, and research runs at the rate it always did.",
                ),
            ],
        ),
    );

    if on {
        let c = app.state.civ.experiments.control;
        append(
            root,
            section(
                "Take over a person",
                vec![
                    note(
                        "With this on, a Take over switch joins the row above the map. Press a \
                         person with it and they are yours: the arrow keys or W A S D walk them, \
                         a stick on the map does the same with a thumb, and the buttons under it \
                         cut what is in front of them, pick a load up or put it down, step \
                         through a doorway, and eat what they have. They plan nothing for \
                         themselves until they are let go, and the rest of being a person still \
                         happens: they age, they tire, and they starve if nobody feeds them.",
                    ),
                    app_bool(h, "Let a person be taken over", c.on, None, |app, v| {
                        app.state.civ.experiments.control.on = v;
                        if !v {
                            app.ui.take_over = false;
                            if let Some(sim) = app.settlement.as_mut() {
                                crate::civ::control::let_go(sim);
                            }
                        }
                        app.request_save();
                        // The switch above the map comes and goes with this.
                        app.rebuild_toolbar = true;
                    }),
                    app_bool(h, "Show the stick", c.joystick,
                        Some("the keys work either way; the stick is for a screen with no \
                              keyboard"),
                        |app, v| {
                            app.state.civ.experiments.control.joystick = v;
                            app.request_save();
                        }),
                    control_num(h, "Walking pace", c.speed, 0.1, 4.0, 0.05,
                        Some("against a person's own"), |c, v| c.speed = v),
                    control_num(h, "Reach (cells)", c.reach, 0.5, 8.0, 0.1,
                        Some("how far a hand goes for something to cut, pick up or step into"),
                        |c, v| c.reach = v),
                ],
            ),
        );
    }

    let aloft = el("div").class("stat-grid").get();
    if on {
        let b = app.state.civ.experiments.balloons;
        append(
            root,
            section(
                "Hot air balloons",
                vec![
                    note(
                        "A town with a school and cloth to spare sews a canopy, burns charcoal \
                         under it and sends it up. What the scholars see from up there is worth \
                         more than another day at the bench, so research runs faster while one \
                         is in the air. No school, no balloon.",
                    ),
                    app_bool(h, "Send them up", b.on, None, |app, v| {
                        app.state.civ.experiments.balloons.on = v;
                        app.request_save();
                    }),
                    balloon_num(h, "Research while aloft", b.research_gain, 0.0, 4.0, 0.05,
                        Some("what one canopy over the town adds, as a fraction"),
                        |c, v| c.research_gain = v),
                    balloon_num(h, "Canopies at once", b.per_town as f64, 1.0, 6.0, 1.0,
                        Some("per town"), |c, v| c.per_town = v as i32),
                    balloon_num(h, "Between flights (s)", b.interval, 30.0, 3600.0, 10.0,
                        Some("settlement seconds a town waits before trying again"),
                        |c, v| c.interval = v),
                    balloon_num(h, "A flight lasts (s)", b.flight, 20.0, 1200.0, 10.0, None,
                        |c, v| c.flight = v),
                    balloon_num(h, "Ceiling (cells)", b.ceiling, 2.0, 60.0, 1.0,
                        Some("how high it gets over the ground it started from"),
                        |c, v| c.ceiling = v),
                    balloon_num(h, "Wind (cells per s)", b.drift, 0.0, 4.0, 0.05,
                        Some("how fast it is carried while it is up"), |c, v| c.drift = v),
                    balloon_num(h, "Cloth for a canopy", b.cloth, 0.0, 60.0, 1.0, None,
                        |c, v| c.cloth = v),
                    balloon_num(h, "Charcoal for the burner", b.fuel, 0.0, 60.0, 1.0, None,
                        |c, v| c.fuel = v),
                ],
            ),
        );
        append(root, section("In the air", vec![aloft.clone()]));
    }

    let mut panel = ExperimentalPanel { aloft, since: 0.0 };
    panel.redraw(app);
    Box::new(panel)
}

impl Panel for ExperimentalPanel {
    fn tick(&mut self, app: &mut App, dt: f64) {
        self.since += dt;
        if self.since < 0.5 {
            return;
        }
        self.since = 0.0;
        self.redraw(app);
    }

    fn redraw(&mut self, app: &mut App) {
        clear(&self.aloft);
        if !app.state.civ.experiments.on {
            return;
        }
        let civ = match &app.settlement {
            Some(civ) => civ,
            None => return,
        };
        let ceiling = app.state.civ.experiments.balloons.ceiling;
        if civ.balloons.is_empty() {
            let _ = self.aloft.append_child(&stat("aloft", "nothing"));
            return;
        }
        for balloon in &civ.balloons {
            let town = civ
                .colony_index(balloon.colony)
                .map(|ci| civ.colonies[ci].name.clone())
                .unwrap_or_default();
            let left = (balloon.flight - balloon.flown).max(0.0);
            let _ = self.aloft.append_child(&stat(
                &town,
                &format!("{:.0} cells up, {left:.0}s left", balloon.height(ceiling)),
            ));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn control_num(
    h: &Handle,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    hint: Option<&str>,
    apply: fn(&mut crate::civ::control::ControlConfig, f64),
) -> Element {
    app_num(h, label, value, NumOpts { min, max, step }, hint, move |app, v| {
        apply(&mut app.state.civ.experiments.control, v);
        app.request_save();
    })
}

#[allow(clippy::too_many_arguments)]
fn balloon_num(
    h: &Handle,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    hint: Option<&str>,
    apply: fn(&mut BalloonConfig, f64),
) -> Element {
    app_num(h, label, value, NumOpts { min, max, step }, hint, move |app, v| {
        apply(&mut app.state.civ.experiments.balloons, v);
        app.request_save();
    })
}
