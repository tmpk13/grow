//! Build panel: the planner's weights, the catalog of what can be raised, and
//! the sites currently going up.

use web_sys::Element;

use crate::app::{App, Handle, Panel};
use crate::civ::buildings::{scaled_cost, BuildConfig, Job, Structure, BUILDINGS, CATEGORIES};
use crate::civ::resources::{format_cost, missing_from, Stock, RES_IDS};
use crate::ui::{
    app_bool, app_num, append, bar, button, clear, clear_scope, colony_picker, danger_button, el,
    note, section, select_field, stat, NumOpts, Scope,
};

pub struct BuildPanel {
    towns: Element,
    counters: Element,
    sites: Element,
    catalog: Element,
    /// The card the Look inside switch fills, and the section around it,
    /// which is hidden whole while nothing is being looked at.
    inside: Element,
    inside_wrap: Element,
    handle: Handle,
    since: f64,
}

pub fn build(root: &Element, app: &mut App, h: &Handle) -> Box<dyn Panel> {
    let cfg = app.state.civ.build;

    // What the Look inside press found, above everything: it is the one part
    // of the panel somebody just pressed the map to see.
    let inside = el("div").class("person-card").get();
    let inside_wrap = section("Looking inside", vec![inside.clone()]);
    let _ = inside_wrap.set_attribute("hidden", "hidden");
    append(root, inside_wrap.clone());

    append(root, place_section(app, h));

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
        note("A person who has saved enough coin has their own house rebuilt one rung larger: \
              hut, house, manor, and finally a tower. The coin goes to the treasury, which pays \
              the laborers who carry the brick. Nobody plans a tower."),
        app_bool(h, "People upgrade their own homes", cfg.home_upgrades, None,
            |app, v| { app.state.civ.build.home_upgrades = v; app.request_save(); }),
        build_num(h, "Upgrade price scale", cfg.upgrade_scale, 0.1, 6.0, 0.1,
            Some("multiplies the coin an owner has to put up"), |c, v| c.upgrade_scale = v),
        build_num(h, "Salvage from the old house", cfg.upgrade_salvage, 0.0, 1.0, 0.05,
            Some("share of the old materials that count toward the new"), |c, v| c.upgrade_salvage = v),
        build_num(h, "Homes rebuilt at once", cfg.max_home_rebuilds as f64, 0.0, 8.0, 1.0,
            Some("a rebuild takes its beds out of the housing stock, and starts only once the \
                  household has somewhere else to sleep"),
            |c, v| c.max_home_rebuilds = v as i32),
        app_bool(h, "Send out expeditions", cfg.expeditions,
            Some("a crowded, well stocked town founds another one"),
            |app, v| { app.state.civ.build.expeditions = v; app.request_save(); }),
        build_num(h, "Most colonies", cfg.max_colonies as f64, 1.0, 8.0, 1.0, None,
            |c, v| c.max_colonies = v as i32),
        build_num(h, "Population before sparing anyone", cfg.expedition_population as f64, 4.0, 200.0, 1.0,
            None, |c, v| c.expedition_population = v as i32),
        build_num(h, "People in a party", cfg.expedition_party as f64, 1.0, 20.0, 1.0,
            Some("families follow them"), |c, v| c.expedition_party = v as i32),
        build_num(h, "Supplies carried out", cfg.expedition_supplies, 0.0, 400.0, 5.0,
            Some("of each founding resource"), |c, v| c.expedition_supplies = v),
        build_num(h, "Between attempts (s)", cfg.expedition_interval, 120.0, 20000.0, 60.0, None,
            |c, v| c.expedition_interval = v),
        build_num(h, "Cells between town centers", cfg.colony_spacing as f64, 8.0, 200.0, 1.0, None,
            |c, v| c.colony_spacing = v as i32),
        note("Somebody stranded a long way from their own town gives up on walking back and \
              founds one where they stand. They bring nothing but what they know: no stores and \
              no storehouse, because nobody planned this. Anyone else out there joins it."),
        app_bool(h, "Strays found towns of their own", cfg.strays_settle, None,
            |app, v| { app.state.civ.build.strays_settle = v; app.request_save(); }),
        build_num(h, "Far from home (cells)", cfg.stray_distance, 10.0, 300.0, 1.0,
            Some("from their own town's center"), |c, v| c.stray_distance = v),
        build_num(h, "Out there before giving up (s)", cfg.stray_wait, 1.0, 600.0, 1.0,
            Some("settlement seconds; a person walks about two and a half cells in one, so \
                  this is short by design"),
            |c, v| c.stray_wait = v),
    ]));

    append(root, section("Pulling things down", vec![
        note("Look inside anything the town has put up and it can be condemned: people take it \
              apart at the rate they build it, leaving most of the materials on the ground. \
              Nobody lives or works in it while it comes down, so the planner starts on its \
              replacement at once. Letting it stand again costs the work already spent on taking \
              it apart. A site not yet raised is called off instead, and whatever was delivered \
              to it is left where it stood."),
        build_num(h, "Work to pull down", cfg.pull_down_share, 0.05, 2.0, 0.05,
            Some("against what it took to put up"), |c, v| c.pull_down_share = v),
        build_num(h, "Salvage from pulling down", cfg.pull_down_salvage, 0.0, 1.0, 0.05,
            Some("share of the materials that come back, more than falls out of a collapse"),
            |c, v| c.pull_down_salvage = v),
    ]));

    append(root, section("Camp fires", vec![
        note("A camp fire is the one thing on the map that takes itself away again. Somebody \
              out after dark in no light at all, further gone than the price of a lamp post, \
              stops walking, gathers what is lying around and lights a fire where they stand. \
              It costs the town nothing, throws a small light while it lasts, then burns out; \
              nothing is salvaged, because it burned. A fire lights its own cell, so the fear \
              below has to sit above the one that buys a lamp post, or no street is ever lit."),
        app_bool(h, "People light fires in the dark", cfg.camp_fires, None,
            |app, v| { app.state.civ.build.camp_fires = v; app.request_save(); }),
        build_num(h, "Frightened enough to light one", cfg.camp_fire_fear, 0.0, 1.0, 0.05,
            Some("keep above the fear that buys a lamp post, or no street is ever lit"),
            |c, v| c.camp_fire_fear = v),
        build_num(h, "How long one burns", cfg.camp_fire_burn, 0.1, 6.0, 0.1,
            Some("against the catalog's burn time, which is five settlement minutes"),
            |c, v| c.camp_fire_burn = v),
        build_num(h, "Fires per town at once", cfg.camp_fires_at_once as f64, 0.0, 20.0, 1.0,
            Some("a bad night should read as a scatter of lights, not a wilderness alight"),
            |c, v| c.camp_fires_at_once = v as i32),
        note("Whoever lights one sits down at it, and anybody else out in the dark walks over \
              and takes a place. Sitting there settles the dark far faster than standing under \
              a lamp, which is what makes the walk worth taking, and it is how the people at a \
              fire come to know each other."),
        build_num(h, "Places at a fire", cfg.camp_fire_seats as f64, 0.0, 8.0, 1.0,
            Some("the cells around it, so a fire in a corner has fewer than one in the open"),
            |c, v| c.camp_fire_seats = v as i32),
        build_num(h, "Frightened enough to walk to one", cfg.camp_fire_gather, 0.0, 1.0, 0.05,
            Some("keep below the fear that lights one: joining a fire is cheaper and should \
                  come first. Anybody with no bed goes whatever their fear"),
            |c, v| c.camp_fire_gather = v),
        build_num(h, "How far one is worth walking", cfg.camp_fire_reach, 1.0, 80.0, 1.0,
            Some("in cells, measured from wherever the night caught them"),
            |c, v| c.camp_fire_reach = v),
        build_num(h, "Warmth of one", cfg.camp_fire_warmth, 1.0, 20.0, 0.5,
            Some("how much faster the dark eases at a fire than under ordinary light; at one \
                  a fire is only a light"),
            |c, v| c.camp_fire_warmth = v),
    ]));

    append(root, section("Walls and gates", vec![
        note("A town that has learned to fortify rings itself: a rectangle around everything \
              it has built, with gates cut where the paths are already worn. Nothing is raised \
              that would leave the town no way out, so the ring closes to its gates and stops."),
        app_bool(h, "Towns wall themselves", cfg.walls, None,
            |app, v| { app.state.civ.build.walls = v; app.request_save(); }),
        build_num(h, "People before walling", cfg.wall_population as f64, 1.0, 200.0, 1.0,
            Some("a village that walls itself spends everything on the wall and starves \
                  inside it"), |c, v| c.wall_population = v as i32),
        build_num(h, "Ring clearance (cells)", cfg.wall_margin as f64, 1.0, 20.0, 1.0,
            Some("gap kept between the outermost building and the wall"), |c, v| c.wall_margin = v as i32),
        build_num(h, "Ways through", cfg.wall_gates as f64, 1.0, 12.0, 1.0,
            Some("gates are drawn to the busiest cells of the ring and spread apart"),
            |c, v| c.wall_gates = v as i32),
        build_num(h, "Wall pieces at once", cfg.wall_sites as f64, 0.0, 6.0, 1.0,
            Some("counted apart from the sites above, or a ring would stop the town building \
                  anything else"), |c, v| c.wall_sites = v as i32),
    ]));

    append(root, section("Stalls", vec![
        note("A person with coin to spare buys a counter of their own, stocks it from the town \
              store at the town's price, and sells over it at a margin they keep. It is the \
              only thing in the settlement that moves coin between two people without the \
              treasury in the middle."),
        app_bool(h, "People open stalls", cfg.stalls, None,
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
    append(root, crate::ui::sprite_drop::made_section(app, h));

    let mut panel = BuildPanel {
        towns,
        counters,
        sites,
        catalog,
        inside,
        inside_wrap,
        handle: h.clone(),
        since: 0.0,
    };
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

/// The placing menu: what the next press on the map puts down. Only the choice
/// lives here; the press itself is the stage's, the same as every other switch
/// over the map.
fn place_section(app: &App, h: &Handle) -> Element {
    let hand = app.ui.hand.clone();
    let mut rows = vec![note(
        "With Place on above the map, every press puts one of these down where it lands: a \
         building for the town chosen below, a plant of any species in the project, a load on \
         the ground, or scenery in the sky behind the map. Anything placed by hand behaves as \
         if the town had placed it, and is built, harvested and hauled by the same rules.",
    )];
    let kinds: Vec<(String, String)> = crate::civ::place::KINDS
        .iter()
        .map(|k| (k.key().to_string(), k.label().to_string()))
        .collect();
    rows.push(select_field(
        "Put down",
        hand.kind.key(),
        &kinds,
        None,
        {
            let h2 = h.clone();
            move |v| {
                let mut sh = h2.borrow_mut();
                sh.app.ui.hand.kind = crate::civ::place::Kind::from_key(&v);
                sh.app.rebuild_panel = true;
            }
        },
    ));
    match hand.kind {
        crate::civ::place::Kind::Building => {
            let options: Vec<(String, String)> = crate::civ::buildings::BUILDINGS
                .iter()
                .map(|d| (d.id.to_string(), format!("{} - {}", d.category.label(), d.label)))
                .collect();
            rows.push(select_field("Which", &hand.building, &options, None, {
                let h2 = h.clone();
                move |v| {
                    h2.borrow_mut().app.ui.hand.building = v;
                }
            }));
            rows.push(app_bool(
                h,
                "Put it up finished",
                hand.finished,
                Some("off lays a site for the town to carry materials to and raise"),
                |app, v| app.ui.hand.finished = v,
            ));
            if let Some(picker) = colony_picker(app, h) {
                rows.push(picker);
            }
        }
        crate::civ::place::Kind::Plant => {
            let options: Vec<(String, String)> = app
                .state
                .species
                .iter()
                .map(|s| (s.id.clone(), s.name.clone()))
                .collect();
            let chosen = if app.state.species.iter().any(|s| s.id == hand.species) {
                hand.species.clone()
            } else {
                app.state.species.first().map(|s| s.id.clone()).unwrap_or_default()
            };
            rows.push(select_field("Which", &chosen, &options, None, {
                let h2 = h.clone();
                move |v| {
                    h2.borrow_mut().app.ui.hand.species = v;
                }
            }));
            rows.push(note(
                "One plant, grown from nothing the way the wilderness grows one: it needs room \
                 for its own kind and will not take in water or on ground somebody has built on.",
            ));
        }
        crate::civ::place::Kind::Scenery => {
            let shapes: Vec<(String, String)> = crate::civ::scenery::SHAPES
                .iter()
                .map(|s| (s.key().to_string(), s.label().to_string()))
                .collect();
            rows.push(select_field("Which", hand.scene.shape.key(), &shapes, None, {
                let h2 = h.clone();
                move |v| {
                    h2.borrow_mut().app.ui.hand.scene.shape =
                        crate::civ::scenery::Shape::from_key(&v);
                }
            }));
            rows.push(app_num(h, "Width (cells)", hand.scene.width,
                NumOpts { min: 2.0, max: 200.0, step: 1.0 }, None,
                |app, v| app.ui.hand.scene.width = v));
            rows.push(app_num(h, "Height (cells)", hand.scene.height,
                NumOpts { min: 0.5, max: 60.0, step: 0.5 },
                Some("or drag it up and down once it is up"),
                |app, v| app.ui.hand.scene.height = v));
            rows.push(app_num(h, "How far off", hand.scene.distance,
                NumOpts { min: 0.0, max: 1.0, step: 0.05 },
                Some("hazes it into the sky and puts it behind anything nearer"),
                |app, v| app.ui.hand.scene.distance = v));
            rows.push(app_num(h, "Snow line", hand.scene.snow,
                NumOpts { min: 0.0, max: 1.0, step: 0.05 },
                Some("as a share of its height; 1 is no snow at all"),
                |app, v| app.ui.hand.scene.snow = v));
            rows.push(note(
                "Press the sky to put one up, then drag it: sideways moves it, up and down \
                 resizes it. Pressing one already standing takes hold of it instead. What is \
                 standing, and what it is made of, is on the Land panel.",
            ));
        }
        crate::civ::place::Kind::Load => {
            let options: Vec<(String, String)> = RES_IDS
                .iter()
                .map(|r| (r.id().to_string(), r.label().to_string()))
                .collect();
            rows.push(select_field("Which", hand.res.id(), &options, None, {
                let h2 = h.clone();
                move |v| {
                    let res = RES_IDS.iter().find(|r| r.id() == v).copied();
                    if let Some(res) = res {
                        h2.borrow_mut().app.ui.hand.res = res;
                    }
                }
            }));
            rows.push(app_num(
                h,
                "How much",
                hand.amount,
                NumOpts { min: 1.0, max: 200.0, step: 1.0 },
                None,
                |app, v| app.ui.hand.amount = v,
            ));
        }
    }
    section("Place by hand", rows)
}

impl BuildPanel {
    /// What the Look inside press found: the building, its state, and everyone
    /// and everything under its roof right now. Hidden whole while nothing is
    /// being looked at, and cleared by its own Done button.
    fn draw_inside(&self, app: &App) {
        clear(&self.inside);
        let civ = match &app.settlement {
            Some(c) => c,
            None => {
                let _ = self.inside_wrap.set_attribute("hidden", "hidden");
                return;
            }
        };
        let b = match app.ui.inspected.and_then(|id| civ.buildings.iter().find(|b| b.id == id)) {
            Some(b) => b,
            // Gone, or dismissed: a looked-at building that was pulled down
            // takes its card with it.
            None => {
                let _ = self.inside_wrap.set_attribute("hidden", "hidden");
                return;
            }
        };
        // Unfolded when it first appears; a fold after that is left alone,
        // this redraws twice a second and would otherwise fight it.
        if self.inside_wrap.has_attribute("hidden") {
            let _ = self.inside_wrap.set_attribute("open", "open");
        }
        let _ = self.inside_wrap.remove_attribute("hidden");

        let names = |ids: &[u32]| -> String {
            let list: Vec<String> = ids
                .iter()
                .filter_map(|&id| civ.people.get(id))
                .map(|p| p.name.clone())
                .collect();
            if list.is_empty() { "nobody".to_string() } else { list.join(", ") }
        };

        let head = if b.name.is_some() {
            format!("{} - {}", b.label(), b.def.label)
        } else {
            b.label()
        };
        let _ = self.inside.append_child(&el("h4").text(&head).get());

        let state = if !b.built {
            let pct = (b.work_done / b.work.max(1.0) * 100.0).clamp(0.0, 100.0);
            if b.upgrading {
                format!("being rebuilt one rung larger, {pct:.0}% raised")
            } else {
                format!("under construction, {pct:.0}% raised")
            }
        } else if b.condemned {
            format!("condemned: {:.0}% pulled down", b.decay * 100.0)
        } else if b.decay > 0.0 {
            format!("standing, but falling in: {:.0}% gone", b.decay * 100.0)
        } else {
            "standing".to_string()
        };
        let _ = self.inside.append_child(&stat("State", &state));
        if !b.built {
            let missing = missing_from(&b.delivered, &b.cost);
            if !missing.is_empty() {
                let _ = self.inside.append_child(&stat("Still needs", &format_cost(&missing)));
            }
        }

        let _ = self.inside.append_child(&stat("Town", &civ.colony_name(b.colony)));
        let owner = match civ.people.get(b.owner) {
            Some(p) => p.name.clone(),
            None => "the town".to_string(),
        };
        let _ = self.inside.append_child(&stat("Owned by", &owner));

        // Who is under the roof this moment, which is the one thing the map
        // itself cannot show: indoors is where it loses sight of people.
        let indoors: Vec<u32> = civ
            .people
            .iter()
            .filter(|p| p.inside == b.id)
            .map(|p| p.id)
            .collect();
        let _ = self.inside.append_child(&stat("Inside now", &names(&indoors)));

        if b.def.housing > 0 {
            let beds = format!("{} of {} beds", b.residents.len(), b.def.housing);
            let _ = self.inside.append_child(&stat("Household", &beds));
            let _ = self.inside.append_child(&stat("Residents", &names(&b.residents)));
        }
        if b.def.rooms > 0 {
            let rooms = format!("{} of {} rooms taken", b.guests.len(), b.def.rooms);
            let _ = self.inside.append_child(&stat("Tonight", &rooms));
            if !b.guests.is_empty() {
                let _ = self.inside.append_child(&stat("Guests", &names(&b.guests)));
            }
        }
        if b.def.slots > 0 || !b.workers.is_empty() {
            let crew = format!("{} of {}: {}", b.workers.len(), b.def.slots, names(&b.workers));
            let _ = self.inside.append_child(&stat("Workers", &crew));
        }
        if b.def.fields > 0 {
            let _ = self
                .inside
                .append_child(&stat("Fields", &format!("{:.0}% watered", b.water * 100.0)));
        }
        if let Some(holds) = stock_line(&b.inv) {
            let _ = self.inside.append_child(&stat("Holds", &holds));
        }
        if let Some(made) = stock_line(&b.out) {
            let _ = self.inside.append_child(&stat("On the bench", &made));
        }

        // Pulling it down: an order to the town's own people rather than
        // something that happens on the press, so the button says what it will
        // set in motion and the work is theirs.
        let (id, built, condemned) = (b.id, b.built, b.condemned);
        let h3 = self.handle.clone();
        let label = match (built, condemned) {
            (false, _) => "Call it off",
            (true, false) => "Pull it down",
            (true, true) => "Let it stand",
        };
        let press: Element = if condemned {
            button(label, Scope::List, move || {
                let mut sh = h3.borrow_mut();
                if let Some(sim) = sh.app.settlement.as_mut() {
                    sim.condemn(id, false);
                }
                sh.app.redraw_panel = true;
            })
        } else {
            danger_button(label, Scope::List, move || {
                let mut sh = h3.borrow_mut();
                if let Some(sim) = sh.app.settlement.as_mut() {
                    sim.condemn(id, true);
                }
                sh.app.redraw_panel = true;
            })
        };
        let _ = self.inside.append_child(&press);

        let h2 = self.handle.clone();
        let _ = self.inside.append_child(&button("Done looking", Scope::List, move || {
            let mut sh = h2.borrow_mut();
            sh.app.ui.inspected = None;
            sh.app.redraw_panel = true;
        }));
    }
}

/// The non-empty lines of a stock, or nothing: an empty bench says nothing
/// rather than listing twelve zeros.
fn stock_line(stock: &Stock) -> Option<String> {
    let mut parts = Vec::new();
    for &r in RES_IDS.iter() {
        let n = stock[r as usize];
        if n >= 0.5 {
            parts.push(format!("{} {}", n.round(), r.label()));
        }
    }
    if parts.is_empty() { None } else { Some(parts.join(", ")) }
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
        self.draw_inside(app);
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

        // Farms, and how wet their fields are: the one thing about a building
        // that changes on its own and that nothing else on the panel says.
        for b in &civ.buildings {
            if !b.built || !matches!(b.def.job, Some(crate::civ::buildings::Job::Farm { .. })) {
                continue;
            }
            let bi = match civ.building_index(b.id) {
                Some(i) => i,
                None => continue,
            };
            let soak = civ.farm_soak(&app.state, bi);
            let source = if soak > 0.6 {
                "on damp ground"
            } else if soak > 0.0 {
                "part way to water"
            } else {
                "carried to by hand"
            };
            let _ = self.towns.append_child(&stat(
                &format!("{} in {}", b.def.label, civ.colony_name(b.colony)),
                &format!(
                    "fields {:.0}% watered, {source}, bringing in {:.0}% of the yield",
                    b.water * 100.0,
                    civ.farm_water_factor(&app.state, bi) * 100.0
                ),
            ));
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
                            Structure::Stall => "opened by a person with the coin for it",
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

