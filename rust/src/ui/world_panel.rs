//! World panel: grid size, per size class ceilings and simulation settings.

use web_sys::Element;

use crate::app::{App, Handle, Panel, Restart, StaticPanel};
use crate::species::{SizeClass, SIZE_CLASSES};
use crate::ui::{
    app_bool, app_button, app_color, app_danger_button, app_num, app_restart_num, app_select,
    append, btn_row, el, note, sampler_options, section, NumOpts,
};

/// A setting the area is built from. Starred and left waiting rather than
/// rebuilding the world under whoever is dragging it.
fn lab_num(
    h: &Handle,
    label: &str,
    value: f64,
    opts: NumOpts,
    hint: Option<&str>,
    apply: fn(&mut App, f64),
) -> Element {
    app_restart_num(h, Restart::Lab, label, value, opts, hint, apply)
}

pub fn build(root: &Element, app: &mut App, h: &Handle) -> Box<dyn Panel> {
    let w = &app.state.world;

    let grid = vec![
        lab_num(h, "Columns (x)", w.cols as f64, NumOpts { min: 8.0, max: 400.0, step: 1.0 },
            Some("cells across the area"),
            |app, v| app.state.world.cols = v as i32),
        lab_num(h, "Rows (depth)", w.rows as f64, NumOpts { min: 2.0, max: 200.0, step: 1.0 },
            Some("cells from the far edge to the near edge"),
            |app, v| app.state.world.rows = v as i32),
        lab_num(h, "Cell width (px)", w.cell_px as f64, NumOpts { min: 2.0, max: 32.0, step: 1.0 }, None,
            |app, v| app.state.world.cell_px = v as i32),
        lab_num(h, "Cell depth (px)", w.depth_px as f64, NumOpts { min: 1.0, max: 32.0, step: 1.0 },
            Some("screen height of one row; below the cell width it foreshortens the ground"),
            |app, v| app.state.world.depth_px = v as i32),
        lab_num(h, "Sky height (px)", w.sky_px as f64, NumOpts { min: 0.0, max: 600.0, step: 2.0 },
            Some("room above the far row for tall plants"),
            |app, v| app.state.world.sky_px = v as i32),
        lab_num(h, "Distance haze", w.depth_fade, NumOpts { min: 0.0, max: 0.5, step: 0.01 },
            Some("tone lift applied to far rows, in ramp steps"),
            |app, v| app.state.world.depth_fade = v),
        app_bool(h, "Ground shadows", w.shadows, None,
            |app, v| { app.state.world.shadows = v; app.repaint_background(); }),
        app_color(h, "Sky top", &w.sky_top, None,
            |app, v| { app.state.world.sky_top = v.to_string(); app.repaint_background(); }),
        app_color(h, "Sky horizon", &w.sky_bottom, None,
            |app, v| { app.state.world.sky_bottom = v.to_string(); app.repaint_background(); }),
        app_select(h, "Soil texture", &w.soil_sampler.clone(), &sampler_options(app), None,
            |app, v| { app.state.world.soil_sampler = v.to_string(); app.repaint_background(); }),
        note("Grid and cell size are starred until Apply: the area is not rebuilt under a slider \
              while it is being dragged."),
    ];
    append(root, section("Area grid", grid));

    let mut class_fields = vec![note(
        "One item per cell per class, so a ground cover and a tree can share a cell but two trees \
         cannot. A species can never exceed its class ceiling. Plants already growing keep the \
         limits they started with.",
    )];
    for class in SIZE_CLASSES {
        let limits = app.state.class_limits.get(class);
        let block = el("div")
            .class("class-block")
            .child(&el("h4").text(&format!("{} (layer {})", class.label(), class.layer())).get())
            .child(&class_num(h, class, "Max footprint radius (cells)", limits.max_radius_cells as f64,
                0.0, 30.0, Some("ceiling on the perimeter a plant may claim"),
                |l, v| l.max_radius_cells = v as i32))
            .child(&class_num(h, class, "Max height (px)", limits.max_height_px, 4.0, 400.0, None,
                |l, v| l.max_height_px = v))
            .child(&class_num(h, class, "Min spacing (cells)", limits.min_spacing as f64, 0.0, 30.0,
                Some("gap enforced between two items of this class"),
                |l, v| l.min_spacing = v as i32))
            .child(&class_num(h, class, "Max instances", limits.max_instances as f64, 0.0, 800.0, None,
                |l, v| l.max_instances = v as i32))
            .get();
        class_fields.push(block);
    }
    append(root, section("Size class limits", class_fields));

    let sim = vec![
        app_num(h, "Seed", app.state.seed as f64, NumOpts { min: 1.0, max: 999_999_999.0, step: 1.0 },
            Some("applied on restart"),
            |app, v| { app.state.seed = v as u32; app.request_save(); }),
        app_num(h, "Ticks per second", app.state.sim.tick_hz, NumOpts { min: 1.0, max: 120.0, step: 1.0 }, None,
            |app, v| { app.state.sim.tick_hz = v.round(); app.request_save(); }),
        app_num(h, "Redraws per frame", app.state.sim.raster_budget as f64,
            NumOpts { min: 1.0, max: 80.0, step: 1.0 },
            Some("plants rasterized per frame; lower keeps the view responsive"),
            |app, v| { app.state.sim.raster_budget = v as usize; app.request_save(); }),
        btn_row(vec![
            app_button(h, "Restart", |app| app.restart()),
            app_button(h, "New seed and restart", |app| {
                app.state.seed = (js_sys::Math::random() * 1e9) as u32;
                app.restart();
                app.rebuild_panel();
            }),
            app_danger_button(h, "Clear plants", |app| app.sim.remove_all()),
        ]),
    ];
    append(root, section("Simulation", sim));

    Box::new(StaticPanel)
}

#[allow(clippy::too_many_arguments)]
fn class_num(
    h: &Handle,
    class: SizeClass,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    hint: Option<&str>,
    apply: fn(&mut crate::species::ClassLimit, f64),
) -> Element {
    app_num(h, label, value, NumOpts { min, max, step: 1.0 }, hint, move |app, v| {
        apply(app.state.class_limits.get_mut(class), v);
        app.species_changed();
    })
}
