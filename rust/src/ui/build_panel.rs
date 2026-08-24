//! Build panel: the planner's weights, the catalog of what can be raised, and
//! the sites currently going up.

use web_sys::Element;

use crate::app::{App, Handle, Panel};
use crate::civ::buildings::{scaled_cost, BuildConfig, Job, Structure, BUILDINGS, CATEGORIES};
use crate::civ::resources::format_cost;
use crate::ui::{
    app_bool, app_num, append, bar, button, clear, clear_scope, colony_picker, el, note, section, stat, NumOpts,
    Scope,
};

pub struct BuildPanel {
    towns: Element,
    counters: Element,
    sites: Element,
    catalog: Element,
    handle: Handle,
    since: f64,
}

pub fn build(root: &Element, app: &mut App, h: &Handle) -> Box<dyn Panel> {
    let cfg = app.state.civ.build;

    append(root, section("Planner", vec![
        app_bool(h, "Plan buildings automatically", cfg.auto_build,
            Some("off leaves every building to the Build buttons below"),
            |app, v| { app.state.civ.build.auto_build = v; app.request_save(); }),
        build_num(h, "Sites at once", cfg.max_sites as f64, 1.0, 12.0, 1.0,
            Some("how many buildings may be under construction"), |c, v| c.max_sites = v as i32),
        build_num(h, "Spacing (cells)", cfg.spacing as f64, 0.0, 4.0, 1.0,
            Some("gap kept between buildings"), |c, v| c.spacing = v as i32),
        build_num(h, "Sprawl (cells)", cfg.sprawl as f64, 4.0, 80.0, 1.0,
            Some("how far from the center a site may be"), |c, v| c.sprawl = v as i32),
        build_num(h, "Cost scale", cfg.cost_scale, 0.1, 4.0, 0.1, None, |c, v| c.cost_scale = v),
        build_num(h, "Work scale", cfg.work_scale, 0.1, 4.0, 0.1, None, |c, v| c.work_scale = v),
        build_num(h, "Housing headroom", cfg.housing_slack as f64, 0.0, 20.0, 1.0,
            Some("empty beds kept ahead of the population"), |c, v| c.housing_slack = v as i32),
    ]));

    let mut favor = Vec::new();
    for cat in CATEGORIES {
        favor.push(crate::ui::app_num(
            h,
            &format!("{} weight", cat.label()),
            cfg.weights.get(cat),
            NumOpts { min: 0.0, max: 3.0, step: 0.1 },
            None,
            move |app, v| {
                *app.state.civ.build.weights.get_mut(cat) = v;
                app.request_save();
            },
        ));
    }
    for cat in CATEGORIES {
        let per = match cfg.per_type.get(cat) {
            Some(p) => p,
            None => continue,
        };
        favor.push(crate::ui::app_num(
            h,
            &format!("People per {} building", cat.label().to_lowercase()),
            per as f64,
            NumOpts { min: 1.0, max: 60.0, step: 1.0 },
            None,
            move |app, v| {
                if let Some(slot) = app.state.civ.build.per_type.get_mut(cat) {
                    *slot = v as i32;
                }
                app.request_save();
            },
        ));
    }
    append(root, section("What to favor", favor));

    append(root, section("Homes and towns", vec![
        note("A settler who has saved enough coin has their own house rebuilt one rung larger: \
              hut, house, manor, and finally a tower. The coin goes to the treasury, which is what \
              pays the laborers who carry the brick. Nobody plans a tower."),
        app_bool(h, "Settlers upgrade their own homes", cfg.home_upgrades, None,
            |app, v| { app.state.civ.build.home_upgrades = v; app.request_save(); }),
        build_num(h, "Upgrade price scale", cfg.upgrade_scale, 0.1, 6.0, 0.1,
            Some("multiplies the coin an owner has to put up"), |c, v| c.upgrade_scale = v),
        build_num(h, "Salvage from the old house", cfg.upgrade_salvage, 0.0, 1.0, 0.05,
            Some("share of the old materials that count toward the new"), |c, v| c.upgrade_salvage = v),
        build_num(h, "Homes rebuilt at once", cfg.max_home_rebuilds as f64, 0.0, 8.0, 1.0,
            Some("a rebuild takes its beds out of the housing stock, and only starts \
                  when the household has somewhere else to sleep"),
            |c, v| c.max_home_rebuilds = v as i32),
        app_bool(h, "Send out expeditions", cfg.expeditions,
            Some("a crowded, well stocked town founds another one"),
            |app, v| { app.state.civ.build.expeditions = v; app.request_save(); }),
        build_num(h, "Most colonies", cfg.max_colonies as f64, 1.0, 8.0, 1.0, None,
            |c, v| c.max_colonies = v as i32),
        build_num(h, "Population before sparing anyone", cfg.expedition_population as f64, 4.0, 200.0, 1.0,
            None, |c, v| c.expedition_population = v as i32),
        build_num(h, "Settlers in a party", cfg.expedition_party as f64, 1.0, 20.0, 1.0,
            Some("families follow them"), |c, v| c.expedition_party = v as i32),
        build_num(h, "Supplies carried out", cfg.expedition_supplies, 0.0, 400.0, 5.0,
            Some("of each founding resource"), |c, v| c.expedition_supplies = v),
        build_num(h, "Between attempts (s)", cfg.expedition_interval, 120.0, 20000.0, 60.0, None,
            |c, v| c.expedition_interval = v),
        build_num(h, "Cells between town centers", cfg.colony_spacing as f64, 8.0, 200.0, 1.0, None,
            |c, v| c.colony_spacing = v as i32),
    ]));

    append(root, section("Walls and gates", vec![
        note("A town that has learned to fortify rings itself: a rectangle around \
              everything it has built, with the gates cut where the paths are already \
              worn. Nothing is ever raised that would leave the town no way out, so the \
              ring closes down to its gates and stops there."),
        app_bool(h, "Towns wall themselves", cfg.walls, None,
            |app, v| { app.state.civ.build.walls = v; app.request_save(); }),
        build_num(h, "People before walling", cfg.wall_population as f64, 1.0, 200.0, 1.0,
            Some("a village that walls itself spends everything on the wall and then \
                  starves inside it"), |c, v| c.wall_population = v as i32),
        build_num(h, "Ring clearance (cells)", cfg.wall_margin as f64, 1.0, 20.0, 1.0,
            Some("gap kept between the outermost building and the wall"), |c, v| c.wall_margin = v as i32),
        build_num(h, "Ways through", cfg.wall_gates as f64, 1.0, 12.0, 1.0,
            Some("gates are drawn to the busiest cells of the ring and spread apart"),
            |c, v| c.wall_gates = v as i32),
        build_num(h, "Wall pieces at once", cfg.wall_sites as f64, 0.0, 6.0, 1.0,
            Some("counted apart from the sites above, or a ring would stop the town \
                  building anything else"), |c, v| c.wall_sites = v as i32),
    ]));

    append(root, section("Stalls", vec![
        note("A settler with coin to spare buys a counter of their own, stocks it out of \
              the town store at the town's price, and sells over it at a margin they \
              keep. It is the only thing in the settlement that moves coin from one \
              person to another without the treasury in the middle."),
        app_bool(h, "Settlers open stalls", cfg.stalls, None,
            |app, v| { app.state.civ.build.stalls = v; app.request_save(); }),
        build_num(h, "Stall price scale", cfg.stall_price_scale, 0.1, 6.0, 0.1,
            Some("multiplies the coin a keeper has to put up"), |c, v| c.stall_price_scale = v),
        build_num(h, "Keeper's margin", cfg.stall_margin, 0.0, 2.0, 0.05,
            Some("what a keeper adds to the town price, times how practised they are"),
            |c, v| c.stall_margin = v),
        build_num(h, "Customers per stall", cfg.stall_customers as f64, 1.0, 60.0, 1.0,
            Some("a counter takes its keeper out of every other trade"), |c, v| c.stall_customers = v as i32),
        build_num(h, "Most stalls in a town", cfg.stalls_per_town as f64, 0.0, 20.0, 1.0, None,
            |c, v| c.stalls_per_town = v as i32),
    ]));

    let counters = el("div").class("roster").get();
    append(root, section("Counters", vec![counters.clone()]));

    let towns = el("div").class("stat-grid").get();
    append(root, section("Towns", vec![towns.clone()]));

    let sites = el("div").class("roster").get();
    append(root, section("Under construction", vec![sites.clone()]));

    let catalog = el("div").class("catalog").get();
    let mut cat_body = Vec::new();
    if let Some(picker) = colony_picker(app, h) {
        cat_body.push(picker);
    }
    cat_body.push(note(
        "Build places a site for the selected town; the materials still have to be carried there \
         before anyone can raise it.",
    ));
    cat_body.push(catalog.clone());
    append(root, section("Catalog", cat_body));

    let mut panel = BuildPanel { towns, counters, sites, catalog, handle: h.clone(), since: 0.0 };
    panel.redraw(app);
    Box::new(panel)
}

#[allow(clippy::too_many_arguments)]
fn build_num(
    h: &Handle,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    hint: Option<&str>,
    apply: fn(&mut BuildConfig, f64),
) -> Element {
    app_num(h, label, value, NumOpts { min, max, step }, hint, move |app, v| {
        apply(&mut app.state.civ.build, v);
        app.request_save();
    })
}

impl Panel for BuildPanel {
    fn redraw(&mut self, app: &mut App) {
        // Every listener below is created fresh on each redraw, so the
        // previous set goes with the nodes it was attached to.
        clear_scope(Scope::List);
        clear(&self.towns);
        clear(&self.counters);
        clear(&self.sites);
        clear(&self.catalog);
        let civ = match &app.settlement {
            Some(c) => c,
            None => return,
        };
        let focus = match civ.focus_colony() {
            Some(c) => c,
            None => return,
        };
        let colony = focus.id;

        for c in &civ.colonies {
            let founded = if c.parent == 0 {
                "the first landing".to_string()
            } else {
                format!("day {} from {}", c.founded_day, civ.colony_name(c.parent))
            };
            let walled = civ
                .buildings
                .iter()
                .filter(|b| b.built && b.colony == c.id && b.def.structure.perimeter())
                .count();
            let line = match c.emptied_day {
                Some(day) => format!("empty since day {day}, {} buildings left standing",
                    civ.buildings.iter().filter(|b| b.colony == c.id && b.built).count()),
                None if walled > 0 => format!(
                    "{} people, {} beds, {walled} pieces of wall, {founded}",
                    c.population, c.housing
                ),
                None => format!("{} people, {} beds, {founded}", c.population, c.housing),
            };
            let _ = self.towns.append_child(&stat(&c.name, &line));
        }

        let mut counters = 0;
        for b in &civ.buildings {
            if !b.built || b.def.structure != Structure::Stall {
                continue;
            }
            counters += 1;
            let keeper = civ
                .people
                .get(b.owner)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "nobody".to_string());
            let wares: Vec<String> = b
                .def
                .sells
                .iter()
                .filter(|&&res| b.inv[res as usize] >= 1.0)
                .map(|&res| {
                    format!(
                        "{} {} at {:.1}",
                        b.inv[res as usize].floor(),
                        res.label().to_lowercase(),
                        civ.stall_price(&app.state, civ.building_index(b.id).unwrap_or(0), res)
                    )
                })
                .collect();
            let row = el("div")
                .class("roster-row")
                .child(&el("span").class("roster-name").text(&keeper).get())
                .child(&el("span").class("roster-job").text(&civ.colony_name(b.colony)).get())
                .child(
                    &el("span")
                        .class("roster-task")
                        .text(&if wares.is_empty() {
                            "an empty counter".to_string()
                        } else {
                            wares.join(", ")
                        })
                        .get(),
                )
                .get();
            let _ = self.counters.append_child(&row);
        }
        if counters == 0 {
            let _ = self
                .counters
                .append_child(&note("Nobody keeps a stall yet."));
        }

        let site_indices = civ.sites();
        for si in &site_indices {
            let site = &civ.buildings[*si];
            let missing: Vec<String> = site
                .cost
                .iter()
                .filter_map(|&(res, n)| {
                    let short = n - site.delivered[res as usize];
                    if short > 0.0 {
                        Some(format!("{} {}", short.ceil(), res.label().to_lowercase()))
                    } else {
                        None
                    }
                })
                .collect();
            let progress = site.work_done / site.work.max(1.0);
            let status = if missing.is_empty() {
                format!("raising {}%", (progress * 100.0).round())
            } else {
                format!("waiting on {}", missing.join(", "))
            };
            let what = if site.upgrading {
                format!("{} (rebuild)", site.def.label)
            } else {
                site.def.label.to_string()
            };
            let row = el("div")
                .class("roster-row")
                .child(&el("span").class("roster-name").text(&what).get())
                .child(
                    &el("span")
                        .class("roster-job")
                        .text(&civ.colony_name(site.colony))
                        .get(),
                )
                .child(&el("span").class("roster-task").text(&status).get())
                .child(&bar("build", progress))
                .get();
            let _ = self.sites.append_child(&row);
        }
        if site_indices.is_empty() {
            let _ = self.sites.append_child(&note("Nothing under construction."));
        }

        for cat in CATEGORIES {
            let defs: Vec<_> = BUILDINGS.iter().filter(|d| d.category == cat).collect();
            if defs.is_empty() {
                continue;
            }
            let block = el("div")
                .class("class-block")
                .child(&el("h4").text(cat.label()).get())
                .get();
            for def in defs {
                let unlocked = def.base || focus.unlocked.contains(def.id);
                let built = civ.count_built(colony, def.id);
                let cost = scaled_cost(def, &app.state.civ.build);
                let mut meta: Vec<String> = Vec::new();
                if def.housing > 0 {
                    meta.push(format!("houses {}", def.housing));
                }
                if def.storage > 0.0 {
                    meta.push(format!("holds {}", def.storage));
                }
                if def.slots > 0 {
                    meta.push(format!("{} workers", def.slots));
                }
                if def.rooms > 0 {
                    meta.push(format!("{} rooms for hire", def.rooms));
                }
                if let Some(next) = def.upgrade_to {
                    meta.push(format!("owner may rebuild as a {}", next.to_lowercase()));
                }
                if !def.planned {
                    meta.push(
                        match def.structure {
                            Structure::Wall | Structure::Gate => "raised on the town's ring",
                            Structure::Stall => "opened by a settler with the coin for it",
                            _ => "only ever raised by its owner",
                        }
                        .to_string(),
                    );
                }
                match &def.job {
                    Some(Job::Craft { input, output, .. }) => {
                        meta.push(format!("{} to {}", format_cost(input), format_cost(output)));
                    }
                    Some(job) => {
                        let out = job.produces();
                        if !out.is_empty() {
                            let names: Vec<&str> = out.iter().map(|&(r, _)| r.id()).collect();
                            meta.push(format!("gathers {}", names.join(", ")));
                        }
                    }
                    None => {}
                }

                let action: Element = if unlocked {
                    let h2 = self.handle.clone();
                    let id = def.id;
                    let label = def.label;
                    button("Build", Scope::List, move || {
                        let mut sh = h2.borrow_mut();
                        let placed = {
                            let App { settlement, state, .. } = &mut sh.app;
                            match settlement {
                                Some(civ) => {
                                    let ci = civ.focus.min(civ.colonies.len().saturating_sub(1));
                                    civ.queue_building(state, ci, id).is_some()
                                }
                                None => false,
                            }
                        };
                        if !placed {
                            sh.app.set_note(&format!("no room for a {label}"));
                        }
                        sh.app.redraw_panel = true;
                    })
                } else {
                    el("span").class("cat-lock").text("locked").get()
                };

                let row = el("div")
                    .class(if unlocked { "cat-row" } else { "cat-row locked" })
                    .child(
                        &el("div")
                            .class("cat-head")
                            .child(&el("span").class("cat-name").text(def.label).get())
                            .child(
                                &el("span")
                                    .class("cat-count")
                                    .text(&if built > 0 { format!("x{built}") } else { String::new() })
                                    .get(),
                            )
                            .child(&action)
                            .get(),
                    )
                    .child(&el("span").class("cat-cost").text(&format_cost(&cost)).get())
                    .maybe(if meta.is_empty() {
                        None
                    } else {
                        Some(el("span").class("cat-meta").text(&meta.join(" - ")).get())
                    })
                    .maybe(def.note.map(|n| el("span").class("cat-note").text(n).get()))
                    .get();
                let _ = block.append_child(&row);
            }
            let _ = self.catalog.append_child(&block);
        }
    }

    fn tick(&mut self, app: &mut App, dt: f64) {
        self.since += dt;
        if self.since < 0.6 {
            return;
        }
        self.since = 0.0;
        self.redraw(app);
    }
}

