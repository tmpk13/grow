//! Land panel: the settlement map, the terrain that generates it and what the
//! view draws on top.

use web_sys::Element;

use crate::app::{App, Handle, Panel, Restart};
use crate::civ::config::FOLIAGE_MODES;
use crate::civ::terrain::{DepositKind, DEPOSIT_KINDS};
use crate::ui::{
    app_bool, app_button, app_color, app_num, app_restart_num, app_select, append, btn_row, clear,
    el, note, sampler_options, section, stat, NumOpts,
};

pub struct LandPanel {
    summary: Element,
    since: f64,
}

pub fn build(root: &Element, app: &mut App, h: &Handle) -> Box<dyn Panel> {
    let civ = &app.state.civ;

    let map = vec![
        civ_num(h, "Columns (x)", civ.world.cols as f64, NumOpts { min: 24.0, max: 512.0, step: 1.0 },
            Some("cells across the map"),
            |app, v| app.state.civ.world.cols = v as i32),
        civ_num(h, "Rows (depth)", civ.world.rows as f64, NumOpts { min: 12.0, max: 256.0, step: 1.0 }, None,
            |app, v| app.state.civ.world.rows = v as i32),
        civ_num(h, "Cell width (px)", civ.world.cell_px as f64, NumOpts { min: 4.0, max: 24.0, step: 1.0 },
            Some("everything drawn is sized from this"),
            |app, v| app.state.civ.world.cell_px = v as i32),
        civ_num(h, "Cell depth (px)", civ.world.depth_px as f64, NumOpts { min: 2.0, max: 24.0, step: 1.0 },
            Some("lower values tilt the ground toward the viewer"),
            |app, v| app.state.civ.world.depth_px = v as i32),
        civ_num(h, "Sky height (px)", civ.world.sky_px as f64, NumOpts { min: 20.0, max: 400.0, step: 2.0 }, None,
            |app, v| app.state.civ.world.sky_px = v as i32),
        app_num(h, "Seed", civ.seed as f64, NumOpts { min: 1.0, max: 999_999_999.0, step: 1.0 },
            Some("terrain, deposits, settlers and everything they do"),
            |app, v| { app.state.civ.seed = v as u32; app.request_save(); }),
        el("h4").text("Grow it instead").get(),
        note("Adds land to the right and along the bottom, so every column and row that is \
              already there keeps its number and nothing standing on one moves. The new ground \
              arrives with a wilderness on it, warmed for as long as a fresh map is; Wild growth \
              goes up with the area at the same time, because the land carries a count of plants \
              rather than a density, and without that the new ground would come out bare. The \
              town, its people and everything they know carry on."),
        grow_num(h, "Add columns", app.ui.grow_cols as f64, |ui, v| ui.grow_cols = v as i32),
        grow_num(h, "Add rows", app.ui.grow_rows as f64, |ui, v| ui.grow_rows = v as i32),
        btn_row(vec![app_button(h, "Grow the map", |app| {
            let (add_c, add_r) = (app.ui.grow_cols.max(0), app.ui.grow_rows.max(0));
            if add_c == 0 && add_r == 0 {
                app.set_note("nothing to add");
                return;
            }
            let was = (app.state.civ.world.cols, app.state.civ.world.rows);
            let (cols, rows) = ((was.0 + add_c).min(512), (was.1 + add_r).min(256));
            let ratio = (cols as f64 * rows as f64) / (was.0 as f64 * was.1 as f64);
            app.state.civ.world.cols = cols;
            app.state.civ.world.rows = rows;
            app.state.civ.terrain.wildness = (app.state.civ.terrain.wildness * ratio).min(6.0);
            // The running world is about to be exactly this, so nothing is
            // left waiting on Apply.
            app.mark_built(Restart::Civ);
            app.set_note("growing the wilderness...");
            app.pending_expand = Some((cols, rows));
        })]),
        el("h4").text("Or start over").get(),
        btn_row(vec![
            app_button(h, "New land", |app| {
                app.state.civ.seed = (js_sys::Math::random() * 1e9) as u32;
                app.civ_restart();
                app.rebuild_panel();
            }),
            app_button(h, "Rebuild this land", |app| app.civ_restart()),
        ]),
        note("Anything starred here is waiting on Apply: the map is not rebuilt while a slider is \
              being dragged. A large map costs memory for its pixel buffers and time for its \
              wilderness warmup, but not frame rate: only what the camera can see is ever drawn."),
    ];
    append(root, section("Map", map));

    let t = &app.state.civ.terrain;
    let terrain = vec![
        terrain_num(h, "Feature size", t.scale, 4.0, 48.0, 1.0,
            Some("cells per noise feature; larger means broader hills and lakes"),
            |t, v| t.scale = v),
        terrain_num(h, "Octaves", t.octaves as f64, 1.0, 6.0, 1.0, None, |t, v| t.octaves = v as i32),
        terrain_num(h, "Roughness", t.persistence, 0.15, 0.85, 0.05, None, |t, v| t.persistence = v),
        terrain_num(h, "Warp", t.warp, 0.0, 1.2, 0.05,
            Some("bends the coastlines out of the noise grid"), |t, v| t.warp = v),
        terrain_num(h, "Water level", t.water_level, 0.0, 0.7, 0.01, None, |t, v| t.water_level = v),
        terrain_num(h, "Shore width", t.sand_band, 0.0, 0.2, 0.01, None, |t, v| t.sand_band = v),
        terrain_num(h, "Rock level", t.rock_level, 0.3, 1.0, 0.01, None, |t, v| t.rock_level = v),
        terrain_num(h, "Moisture scale", t.moist_scale, 4.0, 60.0, 1.0, None, |t, v| t.moist_scale = v),
        terrain_num(h, "Fertility", t.fertility, 0.0, 1.5, 0.05,
            Some("how much the damp ground feeds a farm"), |t, v| t.fertility = v),
        terrain_num(h, "Wild growth", t.wildness, 0.2, 6.0, 0.1,
            Some("how lush the map is: scales seeding and how many plants the land carries"),
            |t, v| t.wildness = v),
        terrain_num(h, "Wilderness warmup (s)", t.warmup, 0.0, 3000.0, 30.0,
            Some("growth simulated before the settlers arrive"), |t, v| t.warmup = v),
    ];
    append(root, section("Terrain", terrain));

    let r = app.state.civ.terrain.rivers;
    append(root, section("Rivers", vec![
        note("Rivers are cut after the noise rather than sampled out of it: a spring in the high \
              ground, then downhill until it reaches standing water or the edge of the map. They \
              leave damp banks behind them, and they are the roads the boats use."),
        river_num(h, "Springs per 10000 cells", r.density, 0.0, 40.0, 0.5,
            Some("a larger map gets more rivers rather than longer ones"), |r, v| r.density = v),
        river_num(h, "Channel width at the mouth", r.width, 0.4, 8.0, 0.1,
            Some("in cells; the head is always one"), |r, v| r.width = v),
        river_num(h, "Meander", r.meander, 0.0, 3.0, 0.05,
            Some("how far the course is pushed off the fall line"), |r, v| r.meander = v),
        river_num(h, "Shortest river kept", r.min_length as f64, 2.0, 200.0, 1.0,
            Some("a trickle shorter than this is thrown away"), |r, v| r.min_length = v as i32),
        river_num(h, "Longest course traced", r.max_length as f64, 20.0, 2000.0, 10.0, None,
            |r, v| r.max_length = v as i32),
        river_num(h, "Bank fertility", r.bank_fertility, 0.0, 1.5, 0.05,
            Some("how much the damp ground either side feeds a farm"), |r, v| r.bank_fertility = v),
    ]));

    let mut deposit_fields = vec![note(
        "Stone and ore sit in the high rock, clay along the water. Every deposit holds a finite \
         amount, so a settlement that has emptied the ground near it has to reach further out.",
    )];
    for kind in DEPOSIT_KINDS {
        let cfg = app.state.civ.terrain.deposits.get(kind);
        let block = el("div")
            .class("class-block")
            .child(&el("h4").text(kind.id()).get())
            .child(&deposit_num(h, kind, "Clusters per 100 cells", cfg.density, 0.0, 5.0, 0.05,
                |d, v| d.density = v))
            .child(&deposit_num(h, kind, "Cluster cells (min)", cfg.cluster_min as f64, 1.0, 12.0, 1.0,
                |d, v| d.cluster_min = v as i32))
            .child(&deposit_num(h, kind, "Cluster cells (max)", cfg.cluster_max as f64, 1.0, 20.0, 1.0,
                |d, v| d.cluster_max = v as i32))
            .child(&deposit_num(h, kind, "Amount per cell (min)", cfg.amount_min as f64, 5.0, 900.0, 5.0,
                |d, v| d.amount_min = v as i32))
            .child(&deposit_num(h, kind, "Amount per cell (max)", cfg.amount_max as f64, 5.0, 2000.0, 5.0,
                |d, v| d.amount_max = v as i32))
            .get();
        deposit_fields.push(block);
    }
    append(root, section("Deposits", deposit_fields));

    let view = &app.state.civ.view;
    let mut view_fields = vec![
        app_bool(h, "Day and night", view.day_night,
            Some("tints the map with the hour and lights the windows"),
            |app, v| { app.state.civ.view.day_night = v; app.civ_repaint(); }),
        app_bool(h, "Footpaths", view.paths, Some("cells that get walked over wear into a path"),
            |app, v| { app.state.civ.view.paths = v; app.civ_repaint(); }),
        app_bool(h, "Deposits", view.deposits, None,
            |app, v| { app.state.civ.view.deposits = v; app.civ_repaint(); }),
        app_bool(h, "People", view.people, None,
            |app, v| { app.state.civ.view.people = v; app.civ_repaint(); }),
        app_bool(h, "Chimney smoke", view.smoke, None,
            |app, v| { app.state.civ.view.smoke = v; app.civ_repaint(); }),
        app_bool(h, "Wind in the trees", view.sway,
            Some("standing plants lean from the tips, each in its own time; runs on settlement \
                  time, so a paused world holds still"),
            |app, v| { app.state.civ.view.sway = v; app.civ_repaint(); }),
        app_num(h, "Sway lean (px)", view.sway_amp,
            NumOpts { min: 0.0, max: 6.0, step: 0.2 },
            Some("how far the crown of a full grown tree leans; smaller plants lean less"),
            |app, v| { app.state.civ.view.sway_amp = v; app.request_save(); }),
        app_num(h, "Sway rate (per s)", view.sway_speed,
            NumOpts { min: 0.05, max: 2.0, step: 0.05 },
            Some("full leans per simulated second, at speed one"),
            |app, v| { app.state.civ.view.sway_speed = v; app.request_save(); }),
        app_bool(h, "Clouds", view.clouds,
            Some("procedural, passing over the sky on settlement time; a paused world holds \
                  its weather still"),
            |app, v| { app.state.civ.view.clouds = v; app.request_save(); }),
        app_num(h, "Cloud cover", view.cloud_cover,
            NumOpts { min: 0.05, max: 1.0, step: 0.05 },
            Some("how much of the sky they take, wisps to overcast"),
            |app, v| { app.state.civ.view.cloud_cover = v; app.request_save(); }),
        app_num(h, "Cloud drift (px per s)", view.cloud_speed,
            NumOpts { min: 0.0, max: 12.0, step: 0.2 },
            Some("how fast they pass, in world pixels per simulated second"),
            |app, v| { app.state.civ.view.cloud_speed = v; app.request_save(); }),
        app_num(h, "Cloud edge wobble", view.cloud_wobble,
            NumOpts { min: 0.0, max: 1.0, step: 0.05 },
            Some("how strongly the edges churn as they pass; zero freezes the shapes and \
                  leaves only the drift"),
            |app, v| { app.state.civ.view.cloud_wobble = v; app.request_save(); }),
        app_bool(h, "Clouds past the map's edge", view.cloud_space,
            Some("the empty space around the map becomes the same sky: the gradient carries \
                  on and the clouds repeat across it"),
            |app, v| { app.state.civ.view.cloud_space = v; app.request_save(); }),
        app_bool(h, "Building labels", view.labels, None,
            |app, v| { app.state.civ.view.labels = v; app.civ_repaint(); }),
        app_bool(h, "Boats", view.boats, None,
            |app, v| { app.state.civ.view.boats = v; app.civ_repaint(); }),
        app_bool(h, "River current", view.current,
            Some("ripples along the flow, baked into the ground"),
            |app, v| { app.state.civ.view.current = v; app.civ_repaint(); }),
        app_num(h, "Fullscreen when idle (s)", view.idle_fullscreen,
            NumOpts { min: 0.0, max: 600.0, step: 5.0 },
            Some("with nobody touching anything for this long the map takes the whole window; \
                  a moved pointer, a key or a touch hands the menus back. Zero never does it"),
            |app, v| { app.state.civ.view.idle_fullscreen = v; app.request_save(); }),
        app_bool(h, "Draw only what is on screen", view.cull,
            Some("off is slower and only useful when something looks wrong at the edge"),
            |app, v| { app.state.civ.view.cull = v; app.civ_repaint(); }),
        app_num(h, "Detail threshold (zoom)", view.detail_zoom,
            NumOpts { min: 0.1, max: 4.0, step: 0.05 },
            Some("zoom below this and the drawing starts shedding detail: first the flourishes, \
                  then the sprites, then everything but the shapes"),
            |app, v| { app.state.civ.view.detail_zoom = v; app.civ_repaint(); }),
        app_color(h, "Shallow water", &view.water_top, None,
            |app, v| { app.state.civ.view.water_top = v.to_string(); app.civ_repaint(); }),
        app_color(h, "Deep water", &view.water_deep, None,
            |app, v| { app.state.civ.view.water_deep = v.to_string(); app.civ_repaint(); }),
        app_color(h, "Footpath", &view.path_color, None,
            |app, v| { app.state.civ.view.path_color = v.to_string(); app.civ_repaint(); }),
        app_select(h, "Soil texture", &app.state.civ.world.soil_sampler.clone(), &sampler_options(app), None,
            |app, v| { app.state.civ.world.soil_sampler = v.to_string(); app.civ_repaint(); }),
    ];
    view_fields.push(app_select(
        h,
        "Foliage over people",
        &app.state.civ.view.foliage.clone(),
        &FOLIAGE_MODES.iter().map(|(id, l)| (id.to_string(), l.to_string())).collect::<Vec<_>>(),
        Some("somebody walking behind a bush is behind it; the other two keep them findable"),
        |app, v| {
            app.state.civ.view.foliage = v.to_string();
            app.civ_repaint();
            // The amount only means anything for one of the three, so it comes
            // and goes with the choice.
            app.rebuild_panel();
        },
    ));
    if app.state.civ.view.foliage == "faded" {
        view_fields.push(app_num(
            h,
            "How much foliage is left",
            app.state.civ.view.foliage_alpha,
            NumOpts { min: 0.1, max: 1.0, step: 0.05 },
            Some("1 is solid, and anything below it lets the settler through"),
            |app, v| {
                app.state.civ.view.foliage_alpha = v;
                app.civ_repaint();
            },
        ));
    }
    append(root, section("View", view_fields));

    let summary = el("div").class("stat-grid").get();
    append(root, section("This land", vec![summary.clone()]));

    let mut panel = LandPanel { summary, since: 0.0 };
    panel.redraw(app);
    Box::new(panel)
}

/// How much bigger the map is about to get. Not a setting of the project and
/// not undoable: it is a number somebody is about to press a button with.
fn grow_num(
    h: &Handle,
    label: &str,
    value: f64,
    apply: fn(&mut crate::app::UiState, f64),
) -> Element {
    let h2 = h.clone();
    crate::ui::number_field(
        label,
        value,
        NumOpts { min: 0.0, max: 256.0, step: 4.0 },
        None,
        move |v| apply(&mut h2.borrow_mut().app.ui, v),
    )
}

/// A setting the map is generated from. Starred and left waiting rather than
/// rebuilding the world under whoever is dragging it.
fn civ_num(
    h: &Handle,
    label: &str,
    value: f64,
    opts: NumOpts,
    hint: Option<&str>,
    apply: fn(&mut App, f64),
) -> Element {
    app_restart_num(h, Restart::Civ, label, value, opts, hint, apply)
}

#[allow(clippy::too_many_arguments)]
fn terrain_num(
    h: &Handle,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    hint: Option<&str>,
    apply: fn(&mut crate::civ::terrain::TerrainConfig, f64),
) -> Element {
    app_restart_num(h, Restart::Civ, label, value, NumOpts { min, max, step }, hint, move |app, v| {
        apply(&mut app.state.civ.terrain, v);
    })
}

#[allow(clippy::too_many_arguments)]
fn river_num(
    h: &Handle,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    hint: Option<&str>,
    apply: fn(&mut crate::civ::terrain::RiverConfig, f64),
) -> Element {
    app_restart_num(h, Restart::Civ, label, value, NumOpts { min, max, step }, hint, move |app, v| {
        apply(&mut app.state.civ.terrain.rivers, v);
    })
}

#[allow(clippy::too_many_arguments)]
fn deposit_num(
    h: &Handle,
    kind: DepositKind,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    apply: fn(&mut crate::civ::terrain::DepositConfig, f64),
) -> Element {
    app_restart_num(h, Restart::Civ, label, value, NumOpts { min, max, step }, None, move |app, v| {
        apply(app.state.civ.terrain.deposits.get_mut(kind), v);
    })
}

impl Panel for LandPanel {
    fn redraw(&mut self, app: &mut App) {
        clear(&self.summary);
        let civ = match &app.settlement {
            Some(c) => c,
            None => return,
        };
        let cells = (civ.world().cols * civ.world().rows) as f64;
        let water = (civ.terrain.water_cells as f64 / cells * 100.0).round();
        let mut rows = vec![
            ("Name".to_string(), civ.name.clone()),
            ("Cells".to_string(), format!("{} x {}", civ.world().cols, civ.world().rows)),
            ("Pixels".to_string(), format!("{} x {}", civ.world().px_w, civ.world().px_h)),
            ("Water".to_string(), format!("{water}%")),
            ("Plants".to_string(), civ.plant_sim.plants.len().to_string()),
            ("Drawing".to_string(), civ.detail.label().to_string()),
            (
                "Towns".to_string(),
                civ.colonies
                    .iter()
                    .map(|c| format!("{} ({})", c.name, c.population))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        ];
        for river in &civ.terrain.rivers {
            rows.push((
                format!("the {}", river.name),
                format!(
                    "{} cells{}",
                    river.path.len(),
                    if river.reaches_sea { "" } else { ", peters out" }
                ),
            ));
        }
        if civ.terrain.rivers.is_empty() {
            rows.push(("Rivers".to_string(), "none on this land".to_string()));
        }
        for kind in DEPOSIT_KINDS {
            let d = civ.terrain.count_deposits(kind);
            rows.push((
                format!("{} left", kind.id()),
                format!("{} in {} spots", d.amount.round(), d.cells),
            ));
        }
        for (k, v) in rows {
            let _ = self.summary.append_child(&stat(&k, &v));
        }
    }

    fn tick(&mut self, app: &mut App, dt: f64) {
        self.since += dt;
        if self.since < 1.0 {
            return;
        }
        self.since = 0.0;
        self.redraw(app);
    }
}
