//! Technology panel: what the settlement knows, what it is working on and the
//! rates behind research.

use web_sys::Element;

use crate::app::{App, Handle, Panel};
use crate::civ::buildings::building_by_id;
use crate::civ::tech::{tech_cost, TechConfig, TechDef, MOD_KEYS, TECHS};
use crate::ui::{
    app_bool, app_num, append, bar, button, chip_head, clear, clear_scope, colony_picker, el, note,
    section, stat, NumOpts, Scope,
};

pub struct TechPanel {
    current: Element,
    mods: Element,
    lore: Element,
    tree: Element,
    handle: Handle,
    since: f64,
}

pub fn build(root: &Element, app: &mut App, h: &Handle) -> Box<dyn Panel> {
    let cfg = app.state.civ.tech;
    append(root, section("Research", vec![
        tech_num(h, "Cost scale", cfg.cost_scale, 0.1, 5.0, 0.1, Some("multiplies every tech cost"), |c, v| c.cost_scale = v),
        tech_num(h, "Points per scholar per second", cfg.research_per_scholar, 0.0, 4.0, 0.05, None, |c, v| c.research_per_scholar = v),
        tech_num(h, "Insight per person per second", cfg.insight_per_person, 0.0, 0.1, 0.001,
            Some("what a settlement works out without a school"), |c, v| c.insight_per_person = v),
        tech_num(h, "Need bias", cfg.need_bias, 0.0, 3.0, 0.1,
            Some("how strongly research chases what the settlement is short of"), |c, v| c.need_bias = v),
        app_bool(h, "Choose research automatically", cfg.auto_research, None, |app, v| {
            app.state.civ.tech.auto_research = v;
            app.request_save();
        }),
    ]));

    let current = el("div").class("stat-grid").get();
    let mods = el("div").class("chips").get();
    let mut progress = Vec::new();
    if let Some(picker) = colony_picker(app, h) {
        progress.push(picker);
        progress.push(note("Each town researches on its own. A colony starts with whatever its \
                            founders knew when they left."));
    }
    progress.push(current.clone());
    progress.push(mods.clone());
    let lore = el("div").get();
    progress.push(lore.clone());
    append(root, section("Progress", progress));

    let tree = el("div").class("tech-tree").get();
    append(root, section("Tree", vec![
        note("Pick one to make it the target; the settlement researches it next."),
        tree.clone(),
    ]));

    let mut panel = TechPanel { current, mods, lore, tree, handle: h.clone(), since: 0.0 };
    panel.redraw(app);
    Box::new(panel)
}

#[allow(clippy::too_many_arguments)]
fn tech_num(
    h: &Handle,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    hint: Option<&str>,
    apply: fn(&mut TechConfig, f64),
) -> Element {
    app_num(h, label, value, NumOpts { min, max, step }, hint, move |app, v| {
        apply(&mut app.state.civ.tech, v);
        app.request_save();
    })
}

impl Panel for TechPanel {
    fn redraw(&mut self, app: &mut App) {
        // Every listener below is created fresh on each redraw, so the
        // previous set goes with the nodes it was attached to.
        clear_scope(Scope::List);
        clear(&self.current);
        clear(&self.mods);
        clear(&self.lore);
        clear(&self.tree);
        let cfg = app.state.civ.tech;
        let civ = match &app.settlement {
            Some(c) => c,
            None => return,
        };
        let colony = match civ.focus_colony() {
            Some(c) => c,
            None => return,
        };
        let tech = &colony.tech;
        let mods = colony.mods;
        let target = tech.target.as_deref().and_then(crate::civ::tech::tech_by_id);
        let rows = [
            ("Town".to_string(), colony.name.clone()),
            ("Known".to_string(), format!("{} of {}", tech.known.len(), TECHS.len())),
            ("Points".to_string(), format!("{}", tech.points.round())),
            ("Spent".to_string(), format!("{}", tech.spent.round())),
            (
                "Target".to_string(),
                match target {
                    Some(t) => t.label.to_string(),
                    None if cfg.auto_research => "automatic".to_string(),
                    None => "none".to_string(),
                },
            ),
        ];
        for (k, v) in rows {
            let _ = self.current.append_child(&stat(&k, &v));
        }
        let mut any_mod = false;
        for key in MOD_KEYS {
            let value = mods.get(key);
            if (value - 1.0).abs() < 0.001 {
                continue;
            }
            if !any_mod {
                let _ = self.mods.append_child(&chip_head("What the research changed"));
                any_mod = true;
            }
            let chip = el("span")
                .class("chip")
                .text(&format!("{} x{:.2}", key.label(), value))
                .get();
            let _ = self.mods.append_child(&chip);
        }

        // What the pointer has taught the map. Not research and not owned by a
        // town, so it sits below the technologies rather than among them, and
        // it is only there at all once something has been cut by hand.
        let taught = civ.lore.known();
        if !taught.is_empty() {
            let chips = el("div").class("chips").get();
            for (id, interest) in taught {
                let name = app.state.find_species(id).map(|s| s.name.as_str()).unwrap_or(id);
                let chip = el("span")
                    .class("chip")
                    .text(&format!("{name} {}%", (interest * 100.0).round()))
                    .get();
                let _ = chips.append_child(&chip);
            }
            let block = el("div")
                .class("class-block")
                .child(&el("h4").text("Learned by hand").get())
                .child(&chips)
                .child(&note("Gatherers take these sooner and walk further for them."))
                .get();
            let _ = self.lore.append_child(&block);
        }

        let known: Vec<&'static TechDef> =
            TECHS.iter().filter(|t| tech.is_known(t.id)).collect();
        let groups: [(&str, Vec<&'static TechDef>, &str); 3] = [
            ("Known", known, "known"),
            ("Available", tech.available(), "open"),
            ("Locked", tech.locked(), "locked"),
        ];
        for (label, list, class) in groups {
            if list.is_empty() {
                continue;
            }
            let block = el("div")
                .class("class-block")
                .child(&el("h4").text(label).get())
                .get();
            for def in list {
                let cost = tech_cost(def, &cfg);
                let unlocks: Vec<&str> = def
                    .unlocks
                    .iter()
                    .map(|id| building_by_id(id).map(|b| b.label).unwrap_or(id))
                    .collect();
                let effects: Vec<String> = def
                    .effects
                    .iter()
                    .map(|&(k, v)| format!("{} +{}%", k.label(), (v * 100.0).round()))
                    .collect();
                let is_target = tech.target.as_deref() == Some(def.id);
                let action: Option<Element> = if class == "known" {
                    None
                } else {
                    let h2 = self.handle.clone();
                    let id = def.id;
                    Some(button(
                        if is_target { "Target" } else { "Research" },
                        Scope::List,
                        move || {
                            let mut sh = h2.borrow_mut();
                            if let Some(civ) = &mut sh.app.settlement {
                                let focus = civ.focus.min(civ.colonies.len().saturating_sub(1));
                                if let Some(colony) = civ.colonies.get_mut(focus) {
                                    colony.tech.target = if colony.tech.target.as_deref()
                                        == Some(id)
                                    {
                                        None
                                    } else {
                                        Some(id.to_string())
                                    };
                                }
                            }
                            sh.app.redraw_panel = true;
                        },
                    ))
                };
                let row_class = if is_target {
                    format!("tech-row {class} target")
                } else {
                    format!("tech-row {class}")
                };
                let row = el("div")
                    .class(&row_class)
                    .child(
                        &el("div")
                            .class("cat-head")
                            .child(&el("span").class("cat-name").text(def.label).get())
                            .child(
                                &el("span")
                                    .class("cat-count")
                                    .text(&if class == "known" {
                                        String::new()
                                    } else {
                                        format!("{cost} pts")
                                    })
                                    .get(),
                            )
                            .maybe(action)
                            .get(),
                    )
                    .child(&el("span").class("cat-note").text(def.note).get())
                    .maybe(if unlocks.is_empty() {
                        None
                    } else {
                        Some(el("span").class("cat-meta").text(&format!("unlocks {}", unlocks.join(", "))).get())
                    })
                    .maybe(if effects.is_empty() {
                        None
                    } else {
                        Some(el("span").class("cat-meta").text(&effects.join(", ")).get())
                    })
                    .maybe(if class == "locked" && !def.requires.is_empty() {
                        Some(
                            el("span")
                                .class("cat-lock")
                                .text(&format!("needs {}", def.requires.join(", ")))
                                .get(),
                        )
                    } else {
                        None
                    })
                    .maybe(if class == "open" {
                        Some(bar("research", (tech.points / cost).min(1.0)))
                    } else {
                        None
                    })
                    .get();
                let _ = block.append_child(&row);
            }
            let _ = self.tree.append_child(&block);
        }
    }

    fn tick(&mut self, app: &mut App, dt: f64) {
        self.since += dt;
        if self.since < 0.8 {
            return;
        }
        self.since = 0.0;
        self.redraw(app);
    }
}
