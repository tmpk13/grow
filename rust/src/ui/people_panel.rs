//! People panel: the parameters that decide how settlers move, work, eat and
//! age, and the register of everyone who has lived here.
//!
//! The roster is a view onto the settler database rather than a summary of it:
//! pick a name and the panel shows that person's parentage, household, trades,
//! purse and the log of what has happened to them.

use web_sys::Element;

use crate::app::{App, Handle, Panel};
use crate::civ::config::WorkConfig;
use crate::civ::people::{PeopleConfig, Profession, PROFESSIONS};
use crate::civ::resources::Res;
use crate::civ::social::SocialConfig;
use crate::civ::settlement::{standing, Settlement};
use crate::ui::sprite_drop::sprites_section;
use crate::ui::{
    app_bool, app_num, append, bar, clear, clear_scope, colony_picker, el, note, section, stat, NumOpts, Scope,
};

/// How the register is ordered, which is also what it is for: the same list
/// sorted by wealth and sorted by age answers two different questions.
///
/// The order travels through the panel trait as a small integer, because that
/// is the widest thing a generic `dyn Panel` can be told without every other
/// panel having to know what a sort order is.
const SORT_AGE: u8 = 0;
const SORT_WEALTH: u8 = 1;
const SORT_STANDING: u8 = 2;
const SORT_NAME: u8 = 3;

const SORTS: [(u8, &str); 4] = [
    (SORT_AGE, "Age"),
    (SORT_WEALTH, "Coin"),
    (SORT_STANDING, "Standing"),
    (SORT_NAME, "Name"),
];

pub struct PeoplePanel {
    counts: Element,
    controls: Element,
    roster: Element,
    detail: Element,
    graves: Element,
    handle: Handle,
    sort: u8,
    /// Person id the detail card is showing, or 0 for nobody.
    selected: u32,
    show_dead: bool,
    since: f64,
}

const START_SUPPLIES: [Res; 4] = [Res::Wood, Res::Food, Res::Fiber, Res::Stone];

pub fn build(root: &Element, app: &mut App, h: &Handle) -> Box<dyn Panel> {
    let mut founding = vec![app_num(
        h,
        "Settlers",
        app.state.civ.start.population as f64,
        NumOpts { min: 1.0, max: 40.0, step: 1.0 },
        Some("applied on the next restart"),
        |app, v| {
            app.state.civ.start.population = v as i32;
            app.request_save();
        },
    )];
    for res in START_SUPPLIES {
        let value = app.state.civ.start.supplies[res as usize];
        founding.push(app_num(
            h,
            &format!("{} carried in", res.label()),
            value,
            NumOpts { min: 0.0, max: 400.0, step: 1.0 },
            None,
            move |app, v| {
                app.state.civ.start.supplies[res as usize] = v.round();
                app.request_save();
            },
        ));
    }
    founding.push(app_bool(
        h,
        "Arrive with a storehouse",
        app.state.civ.start.storehouse,
        Some("off means the first thing they do is build one"),
        |app, v| {
            app.state.civ.start.storehouse = v;
            app.request_save();
        },
    ));
    append(root, section("Founding party", founding));

    // The register first: it is what the panel is for, and the parameters
    // below are what it is explained by.
    let counts = el("div").class("chips").get();
    let controls = el("div").class("chips").get();
    let roster = el("div").class("roster").get();
    let detail = el("div").class("person-card").get();
    let graves = el("div").class("event-log").get();
    let mut register = Vec::new();
    if let Some(picker) = colony_picker(app, h) {
        register.push(picker);
    }
    register.push(counts.clone());
    register.push(controls.clone());
    register.push(detail.clone());
    register.push(roster.clone());
    append(root, section("Register", register));
    append(root, section("Obituaries", vec![graves.clone()]));

    append(root, sprites_section(app, h));

    let p = app.state.civ.people;
    append(root, section("Body and day", vec![
        people_num(h, "Day length (s)", p.day_length, 20.0, 600.0, 5.0, Some("simulated seconds in one day"), |c, v| c.day_length = v),
        people_num(h, "Work starts", p.work_start, 0.0, 0.5, 0.01, Some("fraction of the day"), |c, v| c.work_start = v),
        people_num(h, "Work ends", p.work_end, 0.5, 1.0, 0.01, None, |c, v| c.work_end = v),
        people_num(h, "Walking speed", p.walk_speed, 0.3, 10.0, 0.1, Some("cells per second"), |c, v| c.walk_speed = v),
        people_num(h, "Path speed bonus", p.road_speed_bonus, 0.0, 1.5, 0.05, Some("how much a worn path helps"), |c, v| c.road_speed_bonus = v),
        people_num(h, "Swimming speed", p.swim_speed, 0.05, 1.0, 0.05,
            Some("against walking speed, once somebody is in the water"), |c, v| c.swim_speed = v),
        people_num(h, "Cost of a swim", p.swim_cost, 1.0, 40.0, 0.5,
            Some("how much dearer a step into water is than one onto ground: high enough and \
                  a river is only ever walked round"), |c, v| c.swim_cost = v),
        people_num(h, "Carry capacity", p.carry_capacity, 1.0, 80.0, 1.0, Some("one load; the rest is left where it fell"), |c, v| c.carry_capacity = v),
        people_num(h, "Work rate", p.work_rate, 0.1, 4.0, 0.1, Some("global multiplier on every kind of work"), |c, v| c.work_rate = v),
        people_num(h, "Laborer share", p.laborer_share, 0.0, 0.9, 0.05, Some("adults kept out of workplaces to haul and build"), |c, v| c.laborer_share = v),
    ]));

    append(root, section("Needs", vec![
        people_num(h, "Hunger per second", p.hunger_rate, 0.001, 0.1, 0.001, None, |c, v| c.hunger_rate = v),
        people_num(h, "Eats at", p.eat_at, 0.1, 0.95, 0.05, Some("hunger level that sends someone to the store"), |c, v| c.eat_at = v),
        people_num(h, "Meal size", p.meal_size, 0.5, 10.0, 0.5, Some("food units per meal"), |c, v| c.meal_size = v),
        people_num(h, "Tires per second", p.tire_rate, 0.001, 0.05, 0.001, None, |c, v| c.tire_rate = v),
        people_num(h, "Rests per second", p.sleep_rate, 0.02, 1.0, 0.02, Some("a bed under a roof is worth a third again"), |c, v| c.sleep_rate = v),
        people_num(h, "Starvation damage", p.starve_damage, 0.001, 0.1, 0.001, None, |c, v| c.starve_damage = v),
        people_num(h, "Healing per second", p.heal_rate, 0.0, 0.1, 0.002, None, |c, v| c.heal_rate = v),
    ]));

    append(root, section("Life", vec![
        people_num(h, "Years per day", p.years_per_day, 0.05, 3.0, 0.05, Some("how fast people age"), |c, v| c.years_per_day = v),
        people_num(h, "Adult at (years)", p.adult_age, 4.0, 30.0, 1.0, None, |c, v| c.adult_age = v),
        people_num(h, "Marries from (years)", p.marry_age, 10.0, 60.0, 1.0, Some("only couples have children"), |c, v| c.marry_age = v),
        people_num(h, "Fertile until (years)", p.fertile_until, 20.0, 80.0, 1.0, None, |c, v| c.fertile_until = v),
        people_num(h, "Births per couple per day", p.birth_rate, 0.0, 1.0, 0.01, Some("thinned by food in store and by housing"), |c, v| c.birth_rate = v),
        people_num(h, "Lifespan low", p.lifespan_min as f64, 20.0, 120.0, 1.0, None, |c, v| c.lifespan_min = v as i32),
        people_num(h, "Lifespan high", p.lifespan_max as f64, 20.0, 140.0, 1.0, None, |c, v| c.lifespan_max = v as i32),
        people_num(h, "Sickness per day", p.sickness_rate, 0.0, 0.2, 0.002, Some("a well nearby cuts this, and so does being hardy"), |c, v| c.sickness_rate = v),
        app_num(h, "Dead kept on file", app.state.civ.people_archive as f64,
            NumOpts { min: 0.0, max: 5000.0, step: 50.0 },
            Some("the register holds a slot for everyone ever born; this is where it stops growing"),
            |app, v| { app.state.civ.people_archive = v as usize; app.request_save(); }),
    ]));

    let s = app.state.civ.social;
    append(root, section("Company", vec![
        note("Everyone a settler has stood near for long enough keeps a slot in their \
              memory. What the two of them make of each other follows from how alike \
              they are, plus a draw that belongs to the pair, and it decides who they \
              marry, whose counter they buy from, and how content they are."),
        app_bool(h, "Settlers keep track of each other", s.enabled, None,
            |app, v| { app.state.civ.social.enabled = v; app.request_save(); }),
        social_num(h, "Meeting pass (s)", s.interval, 0.25, 20.0, 0.25,
            Some("simulated seconds between passes over who is standing near whom"), |c, v| c.interval = v),
        social_num(h, "Notices within (cells)", s.radius, 1.0, 12.0, 0.5, None, |c, v| c.radius = v),
        social_num(h, "People remembered", s.memory as f64, 4.0, 80.0, 1.0,
            Some("the faintest bonds are forgotten first; family never is"), |c, v| c.memory = v as usize),
        social_num(h, "Warmth per meeting", s.warmth, 0.005, 0.5, 0.005,
            Some("how far one meeting moves a bond toward what the two make of each other"),
            |c, v| c.warmth = v),
        social_num(h, "Friendship at", s.friend_at, 0.1, 1.0, 0.05,
            Some("and a feud at its negative"), |c, v| c.friend_at = v),
        social_num(h, "Worth of good company", s.company, 0.0, 0.6, 0.02,
            Some("how much friends nearby lift contentment"), |c, v| c.company = v),
        social_num(h, "Courtship weight", s.courtship, 0.0, 6.0, 0.1,
            Some("how much affinity decides between two matches of a like age; it has \
                  no say across a generation"), |c, v| c.courtship = v),
        social_num(h, "Meetings per pass", s.max_meetings as f64, 1.0, 24.0, 1.0,
            Some("what bounds the cost of a crowd"), |c, v| c.max_meetings = v as usize),
    ]));

    append(root, section("Money and lodging", vec![
        note("Wages are only paid once a town has a market. What a settler keeps of one is what \
              eventually buys a bigger house; the rest is spent back into the town the same day."),
        people_num(h, "Kept from a wage", p.savings_share, 0.0, 1.0, 0.05,
            Some("the rest returns to the treasury"), |c, v| c.savings_share = v),
        people_num(h, "A night at an inn", p.inn_price, 0.0, 40.0, 0.5,
            Some("what somebody with no roof pays for a bed and a meal"), |c, v| c.inn_price = v),
    ]));

    let w = app.state.civ.work;
    append(root, section("Work rates", vec![
        work_num(h, "Harvest rate", w.harvest_rate, 0.2, 12.0, 0.1, Some("plant mass cut per second"), |c, v| c.harvest_rate = v),
        work_num(h, "Mining rate", w.mine_rate, 0.1, 12.0, 0.1, None, |c, v| c.mine_rate = v),
        work_num(h, "Building rate", w.build_rate, 0.1, 12.0, 0.1, None, |c, v| c.build_rate = v),
        work_num(h, "Crafting rate", w.craft_rate, 0.1, 12.0, 0.1, None, |c, v| c.craft_rate = v),
        work_num(h, "Farming rate", w.farm_rate, 0.05, 4.0, 0.05, Some("multiplied by the fertility under the fields"), |c, v| c.farm_rate = v),
        work_num(h, "Smallest plant worth cutting", w.min_harvest_mass, 0.5, 20.0, 0.5, None, |c, v| c.min_harvest_mass = v),
        work_num(h, "Cleared ground yield", w.clear_yield, 0.0, 1.0, 0.05, Some("share of a plant recovered when a building is raised over it"), |c, v| c.clear_yield = v),
        work_num(h, "Dropped load life (days)", w.pile_life, 0.2, 30.0, 0.2, None, |c, v| c.pile_life = v),
        work_num(h, "Replanning interval (s)", w.plan_interval, 0.1, 10.0, 0.1, None, |c, v| c.plan_interval = v),
        work_num(h, "Plant index rebuild (s)", w.plant_index_interval, 0.2, 20.0, 0.2,
            Some("how often the coarse map of what is growing where is refreshed"), |c, v| c.plant_index_interval = v),
    ]));

    let mut panel = PeoplePanel {
        counts,
        controls,
        roster,
        detail,
        graves,
        handle: h.clone(),
        sort: SORT_AGE,
        selected: 0,
        show_dead: false,
        since: 0.0,
    };
    panel.redraw(app);
    Box::new(panel)
}

#[allow(clippy::too_many_arguments)]
fn people_num(
    h: &Handle,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    hint: Option<&str>,
    apply: fn(&mut PeopleConfig, f64),
) -> Element {
    crate::ui::app_num(h, label, value, NumOpts { min, max, step }, hint, move |app, v| {
        apply(&mut app.state.civ.people, v);
        app.request_save();
    })
}

#[allow(clippy::too_many_arguments)]
fn social_num(
    h: &Handle,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    hint: Option<&str>,
    apply: fn(&mut SocialConfig, f64),
) -> Element {
    crate::ui::app_num(h, label, value, NumOpts { min, max, step }, hint, move |app, v| {
        apply(&mut app.state.civ.social, v);
        app.request_save();
    })
}

#[allow(clippy::too_many_arguments)]
fn work_num(
    h: &Handle,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    hint: Option<&str>,
    apply: fn(&mut WorkConfig, f64),
) -> Element {
    crate::ui::app_num(h, label, value, NumOpts { min, max, step }, hint, move |app, v| {
        apply(&mut app.state.civ.work, v);
        app.request_save();
    })
}

/// What a settler is doing right now, in one phrase.
fn doing(civ: &Settlement, pi: usize) -> String {
    let p = &civ.people[pi];
    if p.aboard != 0 {
        let boat = civ.boats.iter().find(|b| b.id == p.aboard);
        return match boat {
            Some(b) => format!("aboard the {} ({})", b.name, b.state.label()),
            None => "at sea".to_string(),
        };
    }
    if p.sleeping {
        return if p.indoors() { "asleep indoors".into() } else { "asleep outside".into() };
    }
    let task = p
        .task
        .as_ref()
        .map(|t| t.label().to_string())
        .unwrap_or_else(|| "idle".to_string());
    if p.indoors() {
        let what = civ
            .building_index(p.inside)
            .map(|bi| civ.buildings[bi].label())
            .unwrap_or_else(|| "indoors".to_string());
        return format!("{task}, inside {what}");
    }
    task
}

impl PeoplePanel {
    fn draw_detail(&self, civ: &Settlement, app: &App) {
        let pi = match civ.people.index_of(self.selected) {
            Some(pi) => pi,
            None => {
                let _ = self
                    .detail
                    .append_child(&note("Pick a name below to open their record."));
                return;
            }
        };
        let p = &civ.people[pi];
        let name_of = |id: u32| {
            civ.people
                .get(id)
                .map(|q| q.name.clone())
                .unwrap_or_else(|| "unknown".to_string())
        };
        let home = civ
            .building_index(p.home)
            .map(|bi| civ.buildings[bi].label())
            .unwrap_or_else(|| "nowhere".to_string());
        let owns = civ
            .building_index(p.owns)
            .map(|bi| format!("{} (deed)", civ.buildings[bi].label()))
            .unwrap_or_else(|| "nothing".to_string());
        let work = civ
            .building_index(p.work)
            .map(|bi| civ.buildings[bi].label())
            .unwrap_or_else(|| "no fixed workplace".to_string());
        let children: Vec<String> = p.children.iter().map(|&id| name_of(id)).collect();
        let age = if p.alive {
            format!("{}", p.age.floor())
        } else {
            format!("{} at death", p.age.floor())
        };

        let head = el("div")
            .class("cat-head")
            .child(&el("span").class("cat-name").text(&p.name).get())
            .child(
                &el("span")
                    .class("cat-count")
                    .text(&format!("{} {}", p.profession.label(), age))
                    .get(),
            )
            .get();
        let _ = self.detail.append_child(&head);

        let grid = el("div").class("stat-grid").get();
        let mut rows: Vec<(String, String)> = vec![
            ("Town".into(), civ.colony_name(p.colony)),
            // Founders are dated before the landing, which is where day zero is.
            (
                "Born".into(),
                if p.born < 0 {
                    format!("{} years before {} was founded", -p.born, civ.colony_name(p.born_in))
                } else {
                    format!("day {} in {}", p.born, civ.colony_name(p.born_in))
                },
            ),
            ("Doing".into(), if p.alive { doing(civ, pi) } else { format!("died of {}", p.cause.unwrap_or("old age")) }),
            ("Home".into(), home),
            ("Owns".into(), owns),
            ("Works".into(), work),
            ("Purse".into(), format!("{} coin (peak {})", p.coin.round(), p.peak_coin.round())),
            ("Standing".into(), format!("{:.2}", standing(civ, pi))),
        ];
        if p.spouse != 0 {
            rows.push(("Married to".into(), name_of(p.spouse)));
        }
        if p.mother != 0 || p.father != 0 {
            rows.push((
                "Parents".into(),
                format!("{} and {}", name_of(p.mother), name_of(p.father)),
            ));
        }
        if !children.is_empty() {
            rows.push(("Children".into(), children.join(", ")));
        }
        if p.stall != 0 {
            let what = civ
                .building_index(p.stall)
                .map(|bi| civ.buildings[bi].label())
                .unwrap_or_else(|| "a stall".to_string());
            rows.push(("Keeps".into(), what));
        }
        if !p.bonds.is_empty() {
            let count = |n: usize, one: &str, many: &str| {
                if n == 1 {
                    format!("1 {one}")
                } else {
                    format!("{n} {many}")
                }
            };
            rows.push((
                "Knows".into(),
                format!(
                    "{}, {}, {}",
                    count(p.bonds.len(), "person", "people"),
                    count(p.friends as usize, "friend", "friends"),
                    count(p.rivals as usize, "rival", "rivals")
                ),
            ));
        }
        if p.literacy > 0.01 {
            rows.push(("Lettered".into(), format!("{:.0}%", p.literacy * 100.0)));
        }
        for (k, v) in rows {
            let _ = grid.append_child(&stat(&k, &v));
        }
        let _ = self.detail.append_child(&grid);

        let bars = el("div").class("person-bars").get();
        for (label, value) in [
            ("health", p.health),
            ("fed", 1.0 - p.hunger),
            ("rested", p.energy),
            ("content", p.happiness),
        ] {
            let _ = bars.append_child(
                &el("div")
                    .class("field")
                    .child(&el("span").class("field-label").text(label).get())
                    .child(&bar(label, value))
                    .get(),
            );
        }
        let _ = self.detail.append_child(&bars);

        let traits = el("div").class("chips").get();
        for (label, value) in p.traits.rows() {
            let chip = el("span")
                .class("chip")
                .text(&format!("{label} {:.0}", value * 100.0))
                .get();
            let _ = traits.append_child(&chip);
        }
        let _ = self.detail.append_child(&traits);

        let skills = el("div").class("chips").get();
        let mut best: Vec<(Profession, f64)> = PROFESSIONS
            .iter()
            .map(|&prof| (prof, p.skill_in(prof)))
            .filter(|&(_, v)| v > 1.02)
            .collect();
        best.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (prof, value) in best.into_iter().take(5) {
            let chip = el("span")
                .class("chip")
                .text(&format!("{} x{:.2}", prof.label().to_lowercase(), value))
                .get();
            let _ = skills.append_child(&chip);
        }
        let _ = self.detail.append_child(&skills);

        // Who they know, strongest feeling first either way: a feud is as much
        // a fact about somebody as a friendship is.
        let friend_at = app.state.civ.social.friend_at;
        let mut bonds: Vec<_> = p.bonds.iter().collect();
        bonds.sort_by(|a, b| {
            b.affinity
                .abs()
                .partial_cmp(&a.affinity.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if !bonds.is_empty() {
            let list = el("div").class("bonds").get();
            for bond in bonds.into_iter().take(8) {
                let tie = bond.tie(p.spouse, friend_at);
                let warmth = (bond.affinity.abs() as f64).min(1.0);
                let kind = if bond.affinity < 0.0 { "rival" } else { "friend" };
                let row = el("div")
                    .class("bond")
                    .child(&el("span").class("bond-name").text(&name_of(bond.who)).get())
                    .child(
                        &el("span")
                            .class(&format!("bond-tie {}", tie.label()))
                            .text(tie.label())
                            .get(),
                    )
                    .child(&bar(kind, warmth))
                    .get();
                let _ = list.append_child(&row);
            }
            let _ = self.detail.append_child(&list);
        }

        let log = el("div").class("event-log").get();
        for e in p.events.iter().rev() {
            let line = el("div")
                .class("event")
                .text(&format!("day {}  {}", e.day, e.text))
                .get();
            let _ = log.append_child(&line);
        }
        let _ = self.detail.append_child(&log);
    }
}

impl Panel for PeoplePanel {
    fn redraw(&mut self, app: &mut App) {
        // Every listener below is created fresh on each redraw, so the
        // previous set goes with the nodes it was attached to.
        clear_scope(Scope::List);
        clear(&self.counts);
        clear(&self.controls);
        clear(&self.roster);
        clear(&self.detail);
        clear(&self.graves);
        let civ = match &app.settlement {
            Some(c) => c,
            None => return,
        };
        let colony = civ.focus_colony().map(|c| c.id).unwrap_or(0);
        let stats = civ.stats(&app.state);
        for prof in PROFESSIONS {
            let n = civ
                .people
                .iter()
                .filter(|p| p.colony == colony && p.profession == prof)
                .count();
            if n == 0 {
                continue;
            }
            let chip = el("span").class("chip").text(&format!("{} {}", prof.label(), n)).get();
            let _ = self.counts.append_child(&chip);
        }
        let _ = self.counts.append_child(
            &el("span")
                .class("chip dim")
                .text(&format!("on file {}", civ.people.slots()))
                .get(),
        );

        for (sort, label) in SORTS {
            let h2 = self.handle.clone();
            let class = if self.sort == sort { "chip active" } else { "chip" };
            let chip = el("button")
                .class(class)
                .attr("type", "button")
                .text(label)
                .on("click", Scope::List, move |_| {
                    let mut sh = h2.borrow_mut();
                    if let Some(panel) = sh.panel.as_mut() {
                        panel.set_sort(sort);
                    }
                    sh.app.redraw_panel = true;
                })
                .get();
            let _ = self.controls.append_child(&chip);
        }
        {
            let h2 = self.handle.clone();
            let class = if self.show_dead { "chip active" } else { "chip" };
            let chip = el("button")
                .class(class)
                .attr("type", "button")
                .text("Include the dead")
                .on("click", Scope::List, move |_| {
                    let mut sh = h2.borrow_mut();
                    if let Some(panel) = sh.panel.as_mut() {
                        panel.toggle_dead();
                    }
                    sh.app.redraw_panel = true;
                })
                .get();
            let _ = self.controls.append_child(&chip);
        }

        self.draw_detail(civ, app);

        let mut people: Vec<usize> = if self.show_dead {
            (0..civ.people.slots()).collect()
        } else {
            civ.people.live_indices()
        };
        people.retain(|&pi| civ.people[pi].colony == colony);
        let sort = self.sort;
        people.sort_by(|&a, &b| {
            let (x, y) = (&civ.people[a], &civ.people[b]);
            let order = match sort {
                SORT_WEALTH => y.coin.partial_cmp(&x.coin),
                SORT_STANDING => standing(civ, b).partial_cmp(&standing(civ, a)),
                SORT_NAME => return x.name.cmp(&y.name),
                _ => y.age.partial_cmp(&x.age),
            };
            order.unwrap_or(std::cmp::Ordering::Equal)
        });

        for pi in people.into_iter().take(60) {
            let p = &civ.people[pi];
            let carry = match (p.carry.res, p.carrying()) {
                (Some(res), true) => format!(" {} {}", p.carry.n.round(), res.id()),
                _ => String::new(),
            };
            let task = if p.alive {
                format!("{}{carry}", doing(civ, pi))
            } else {
                format!("died day {} of {}", p.died, p.cause.unwrap_or("old age"))
            };
            let h2 = self.handle.clone();
            let id = p.id;
            let selected = self.selected == id;
            let class = if !p.alive {
                "roster-row dead"
            } else if selected {
                "roster-row active"
            } else {
                "roster-row"
            };
            let row = el("div")
                .class(class)
                .child(&{
                    let h3 = h2.clone();
                    el("button")
                        .class("roster-name link")
                        .attr("type", "button")
                        .text(&p.name)
                        .on("click", Scope::List, move |_| {
                            let mut sh = h3.borrow_mut();
                            if let Some(panel) = sh.panel.as_mut() {
                                panel.select(id);
                            }
                            sh.app.redraw_panel = true;
                        })
                        .get()
                })
                .child(
                    &el("span")
                        .class("roster-job")
                        .text(&format!("{} {}", p.profession.label(), p.age.floor()))
                        .get(),
                )
                .child(&el("span").class("roster-task").text(&task).get())
                .child(&el("span").class("roster-coin").text(&format!("{}c", p.coin.round())).get())
                .child(&bar("hunger", 1.0 - p.hunger))
                .child(&bar("health", p.health))
                .get();
            let _ = self.roster.append_child(&row);
        }

        for o in civ.dead.iter().rev().take(12) {
            let line = el("div")
                .class("event")
                .text(&format!(
                    "day {}  {} of {} died of {} at {}",
                    o.day,
                    o.name,
                    civ.colony_name(o.colony),
                    o.cause,
                    o.age
                ))
                .get();
            let _ = self.graves.append_child(&line);
        }
        let _ = stats;
    }

    fn tick(&mut self, app: &mut App, dt: f64) {
        self.since += dt;
        if self.since < 0.5 {
            return;
        }
        self.since = 0.0;
        self.redraw(app);
    }

    fn select(&mut self, id: u32) {
        self.selected = if self.selected == id { 0 } else { id };
    }

    fn set_sort(&mut self, sort: u8) {
        self.sort = sort;
    }

    fn toggle_dead(&mut self) {
        self.show_dead = !self.show_dead;
    }
}
