//! Species panel: the parameter form (generated from SPECIES_SCHEMA) plus an
//! isolated growth preview for the selected species.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlCanvasElement};

use crate::app::{App, Handle, Panel};
use crate::render::draw_plant_preview;
use crate::sim::Preview;
use crate::species::{
    effective_limits, FieldKind, Species, SIZE_CLASSES, SPECIES_SCHEMA,
};
use crate::ui::{
    app_bool, app_button, app_danger_button, app_num, app_range, app_select, app_text, append,
    btn_row, button, el, note, sampler_options, section, NumOpts, Scope,
};

pub struct SpeciesPanel {
    species_id: String,
    canvas: HtmlCanvasElement,
    info: Element,
    preview: Rc<RefCell<Preview>>,
    speed: Rc<Cell<f64>>,
    paused: Rc<Cell<bool>>,
}

pub fn build(root: &Element, app: &mut App, h: &Handle) -> Box<dyn Panel> {
    if app.state.species.iter().all(|s| s.id != app.ui.selected_species) {
        app.ui.selected_species = app.state.species.first().map(|s| s.id.clone()).unwrap_or_default();
    }

    let chips = el("div").class("chips").get();
    for sp in &app.state.species {
        let h2 = h.clone();
        let id = sp.id.clone();
        let mut class = String::from("chip");
        if sp.id == app.ui.selected_species {
            class.push_str(" active");
        }
        if !sp.enabled {
            class.push_str(" off");
        }
        let chip = el("button")
            .class(&class)
            .attr("type", "button")
            .text(&sp.name)
            .on("click", Scope::Panel, move |_| {
                let mut sh = h2.borrow_mut();
                sh.app.ui.selected_species = id.clone();
                sh.app.rebuild_panel();
            })
            .get();
        let _ = chips.append_child(&chip);
    }

    let actions = btn_row(vec![
        app_button(h, "Add", |app| {
            let id = app.uid("sp");
            let name = format!("Species {}", app.state.species.len() + 1);
            let mut sp = Species::new(&id, &name);
            sp.id = id.clone();
            app.state.species.push(sp);
            app.ui.selected_species = id;
            app.species_changed();
            app.rebuild_panel();
        }),
        app_button(h, "Duplicate", |app| {
            let selected = app.ui.selected_species.clone();
            let copy = match app.state.find_species(&selected) {
                Some(sp) => {
                    let mut copy = sp.clone();
                    copy.id = String::new();
                    copy.name = format!("{} copy", sp.name);
                    copy
                }
                None => return,
            };
            let id = app.uid("sp");
            let mut copy = copy;
            copy.id = id.clone();
            app.state.species.push(copy);
            app.ui.selected_species = id;
            app.species_changed();
            app.rebuild_panel();
        }),
        app_danger_button(h, "Remove", |app| {
            if app.state.species.len() <= 1 {
                return;
            }
            let selected = app.ui.selected_species.clone();
            if let Some(i) = app.state.species_index(&selected) {
                app.state.species.remove(i);
                let next = i.saturating_sub(1);
                app.ui.selected_species = app.state.species[next].id.clone();
                app.species_changed();
                app.rebuild_panel();
            }
        }),
    ]);
    append(root, section("Species", vec![chips, actions]));

    let species = match app.state.find_species(&app.ui.selected_species).cloned() {
        Some(sp) => sp,
        None => return Box::new(crate::app::StaticPanel),
    };

    // ---- preview ---------------------------------------------------------
    let canvas = el("canvas")
        .class("species-preview")
        .get()
        .dyn_into::<HtmlCanvasElement>()
        .unwrap();
    let info = el("p").class("note").get();
    let preview = Rc::new(RefCell::new(Preview::new(&app.state, &species, 1234)));
    let speed = Rc::new(Cell::new(4.0));
    let paused = Rc::new(Cell::new(false));

    let regrow = {
        let h2 = h.clone();
        let prev = preview.clone();
        let canvas2 = canvas.clone();
        let id = species.id.clone();
        button("Regrow", Scope::Panel, move || {
            let mut sh = h2.borrow_mut();
            let app = &mut sh.app;
            let sp = match app.state.find_species(&id).cloned() {
                Some(sp) => sp,
                None => return,
            };
            let seed = (js_sys::Math::random() * 1e9) as u32;
            let mut fresh = Preview::new(&app.state, &sp, seed);
            let App { state, env, scratch, .. } = app;
            fresh.raster(state, env, scratch, &sp);
            draw_plant_preview(&canvas2, &fresh.plant);
            *prev.borrow_mut() = fresh;
        })
    };

    let grow_full = {
        let h2 = h.clone();
        let prev = preview.clone();
        let canvas2 = canvas.clone();
        let id = species.id.clone();
        button("Grow to full", Scope::Panel, move || {
            let mut sh = h2.borrow_mut();
            let app = &mut sh.app;
            let sp = match app.state.find_species(&id).cloned() {
                Some(sp) => sp,
                None => return,
            };
            let mut prev = prev.borrow_mut();
            let mut guard = 0;
            while !prev.plant.mature() && guard < 4000 {
                prev.grow(1.0, &sp);
                guard += 1;
            }
            let App { state, env, scratch, .. } = app;
            prev.raster(state, env, scratch, &sp);
            draw_plant_preview(&canvas2, &prev.plant);
        })
    };

    let pause = {
        let paused2 = paused.clone();
        el("button")
            .class("btn")
            .attr("type", "button")
            .text("Pause")
            .on("click", Scope::Panel, move |e| {
                let next = !paused2.get();
                paused2.set(next);
                if let Some(node) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) {
                    node.set_text_content(Some(if next { "Resume" } else { "Pause" }));
                }
            })
            .get()
    };

    let speed_field = {
        let speed2 = speed.clone();
        crate::ui::number_field(
            "Preview speed",
            speed.get(),
            NumOpts { min: 0.25, max: 40.0, step: 0.25 },
            None,
            move |v| speed2.set(v),
        )
    };

    let eff = effective_limits(&species, &app.state.class_limits);
    let eff_note = note(&format!(
        "Effective limits after the {} class ceiling: radius {} cells, height {} px, spacing {} \
         cells, max {} instances.",
        species.size_class.label(),
        eff.max_radius_cells,
        eff.max_height_px,
        eff.min_spacing,
        eff.max_instances
    ));

    append(root, section("Growth preview", vec![
        el("div").class("preview-wrap tall").child(canvas.unchecked_ref()).get(),
        btn_row(vec![regrow, grow_full, pause]),
        speed_field,
        info.clone(),
        eff_note,
    ]));

    // ---- parameter form --------------------------------------------------
    for group in SPECIES_SCHEMA {
        let fields = group
            .fields
            .iter()
            .map(|f| build_field(h, app, &species, f))
            .collect();
        append(root, section(group.group, fields));
    }

    let mut panel = SpeciesPanel {
        species_id: species.id.clone(),
        canvas,
        info,
        preview,
        speed,
        paused,
    };
    panel.redraw(app);
    Box::new(panel)
}

fn build_field(h: &Handle, app: &App, species: &Species, field: &'static crate::species::Field) -> Element {
    let id = species.id.clone();
    let hint = field.hint;
    match &field.kind {
        FieldKind::Text { get, set } => {
            let set = *set;
            app_text(h, field.label, &get(species), hint, move |app, v| {
                if let Some(i) = app.state.species_index(&id) {
                    set(&mut app.state.species[i], v);
                }
                app.request_save();
            })
        }
        FieldKind::Bool { get, set } => {
            let set = *set;
            app_bool(h, field.label, get(species), hint, move |app, v| {
                if let Some(i) = app.state.species_index(&id) {
                    set(&mut app.state.species[i], v);
                }
                app.species_changed();
            })
        }
        FieldKind::SizeClassPick => {
            let options: Vec<(String, String)> = SIZE_CLASSES
                .iter()
                .map(|c| (c.id().to_string(), c.label().to_string()))
                .collect();
            app_select(h, field.label, species.size_class.id(), &options, hint, move |app, v| {
                if let Some(class) = crate::species::SizeClass::from_id(v) {
                    if let Some(i) = app.state.species_index(&id) {
                        app.state.species[i].size_class = class;
                    }
                }
                app.species_changed();
                app.rebuild_panel();
            })
        }
        FieldKind::SamplerPick { get, set } => {
            let set = *set;
            app_select(h, field.label, &get(species), &sampler_options(app), hint, move |app, v| {
                if let Some(i) = app.state.species_index(&id) {
                    set(&mut app.state.species[i], v);
                }
                app.species_changed();
            })
        }
        FieldKind::Num { get, set, min, max, step } => {
            let set = *set;
            app_num(h, field.label, get(species), NumOpts { min: *min, max: *max, step: *step }, hint,
                move |app, v| {
                    if let Some(i) = app.state.species_index(&id) {
                        set(&mut app.state.species[i], v);
                    }
                    app.species_changed();
                })
        }
        FieldKind::Range { get, set, min, max, step } => {
            let set = *set;
            let (lo, hi) = get(species);
            app_range(h, field.label, lo, hi, NumOpts { min: *min, max: *max, step: *step }, hint,
                move |app, lo, hi| {
                    if let Some(i) = app.state.species_index(&id) {
                        set(&mut app.state.species[i], lo, hi);
                    }
                    app.species_changed();
                })
        }
    }
}

impl Panel for SpeciesPanel {
    fn redraw(&mut self, app: &mut App) {
        let sp = match app.state.find_species(&self.species_id).cloned() {
            Some(sp) => sp,
            None => return,
        };
        let mut preview = self.preview.borrow_mut();
        let App { state, env, scratch, .. } = app;
        preview.raster(state, env, scratch, &sp);
        draw_plant_preview(&self.canvas, &preview.plant);
    }

    fn tick(&mut self, app: &mut App, dt: f64) {
        if self.paused.get() {
            return;
        }
        let sp = match app.state.find_species(&self.species_id).cloned() {
            Some(sp) => sp,
            None => return,
        };
        let mut preview = self.preview.borrow_mut();
        if !preview.plant.mature() {
            preview.grow(dt * self.speed.get(), &sp);
            if preview.plant.dirty {
                let App { state, env, scratch, .. } = app;
                preview.raster(state, env, scratch, &sp);
            }
            draw_plant_preview(&self.canvas, &preview.plant);
        }
        self.info.set_text_content(Some(&format!(
            "age {:.0}, segments {}, leaves {}, active tips {}{}",
            preview.plant.age,
            preview.plant.segments.len(),
            preview.plant.leaves.len(),
            preview.plant.alive_tip_count(),
            if preview.plant.mature() { " (mature)" } else { "" }
        )));
    }
}
