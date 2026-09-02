//! Dropping images onto a person animation.
//!
//! One card per motion, each its own drop target. A single image is read as a
//! strip of frames, several images are read as one frame each in the order
//! their names sort, and the frame count stays editable afterwards because the
//! sheet is kept whole rather than cut up.
//!
//! What a dropped file becomes is `ui/decode`'s business; this is only where a
//! motion's slot is, and how the clip in it is tuned afterwards.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::{Clamped, JsCast};
use web_sys::{
    CanvasRenderingContext2d, DragEvent, Element, Event, FileList, HtmlCanvasElement, ImageData,
};

use crate::app::{App, Handle};
use crate::civ::sprites::{
    guess_frames, Clip, Frame, FromSheet, Motion, MAX_FRAMES, MAX_SCALE, MIN_SCALE, MOTIONS,
};
use crate::find::{Entry, Index, Search};
use crate::ui::{
    app_bool, app_num, button, danger_button, document, el, input_el, note, number_field, on,
    section, select_field, NumOpts, Scope, Tap,
};
use crate::util::unpack_rgba;

/// Sheet size past which a project stops fitting comfortably in local storage.
/// Nothing is refused at it; the panel just says so, because a save that fails
/// is a worse way to find out.
const SIZE_WARN: usize = 1 << 20;

/// The whole "Person sprites" section: the switch, a card per motion, and what
/// the sheets are costing.
/// What a dropped image is for. The drop target, the file reading and the
/// picker beside them are the same either way; only what the clip lands in
/// differs.
#[derive(Clone, PartialEq, Eq)]
pub enum Slot {
    /// One of a person's motions.
    Motion(Motion),
    /// A thing people make, by its name in the catalog.
    Made(String),
}

impl Slot {
    /// Which drop target's pixel size this slot reads. People are drawn at one
    /// size and the things they build at another, so the two kinds of slot
    /// keep separate answers; every motion shares one, because a person's
    /// motions are one set of art.
    pub fn scale_key(&self) -> &'static str {
        match self {
            Slot::Motion(_) => "motion",
            Slot::Made(_) => "made",
        }
    }
}

pub fn sprites_section(app: &App, h: &Handle) -> Element {
    let sprites = &app.state.civ.sprites;
    let mut rows = vec![
        note(
            "Drop images on a motion to draw people with them instead of with the \
             generated body. One image is read as a strip of frames; several are read \
             as one frame each, in the order their names sort. A motion with nothing \
             on it borrows from a related one, so a single walk sheet is enough to \
             replace the person everywhere. A frame is drawn whole, exactly as it was \
             drawn, so motions exported from one canvas line up however much room each \
             of them uses.",
        ),
        art_scale_row(app, h),
        crate::ui::decode::scale_field(
            app,
            h,
            "motion",
            "how much of a dropped picture goes to one pixel of the person; 0 works it out \
             from the picture",
        ),
        app_bool(
            h,
            "Draw people from dropped images",
            sprites.enabled,
            Some("off keeps every sheet and goes back to the generated person"),
            |app, v| {
                app.state.civ.sprites.enabled = v;
                app.sprites_changed();
            },
        ),
    ];
    for motion in MOTIONS {
        rows.push(slot_card(app, h, motion));
    }
    let bytes = sprites.bytes();
    if bytes > 0 {
        let text = format!("Sheets in this project: {}.", size_text(bytes));
        rows.push(note(&if bytes > SIZE_WARN {
            format!("{text} Past about a megabyte a project may be too large to save in the browser; drop smaller art or fewer frames.")
        } else {
            text
        }));
    }
    section("Person sprites", rows)
}

/// The one number that says how large every dropped picture comes out: the
/// resolution the art was drawn at, against a map cell. It sits in both sprite
/// sections because people and the things they build have to agree about it
/// or a person ends up towering over a house.
fn art_scale_row(app: &App, h: &Handle) -> Element {
    let cell = app.state.civ.world.cell_px;
    let per_cell = app.state.civ.art_px_per_cell;
    let hint = format!(
        "art pixels to one map cell; a cell is {cell} px on the map, so art drawn at \
         {per_cell:.0} px per cell comes out at {:.2} of its own size",
        cell as f64 / per_cell.max(1.0)
    );
    app_num(
        h,
        "Art pixels per cell",
        per_cell,
        NumOpts { min: 1.0, max: 64.0, step: 1.0 },
        Some(&hint),
        |app, v| {
            app.state.civ.art_px_per_cell = v;
            app.state.civ.made.touch();
            app.sprites_changed();
            app.rebuild_panel();
        },
    )
}

/// Pictures for the things people make, one per state of each. There are forty
/// odd things and four states apiece, so the list is not simply shown: it is
/// searched, and the whole of it is behind a switch.
pub fn made_section(app: &App, h: &Handle) -> Element {
    let made = &app.state.civ.made;
    let mut rows = vec![
        note(
            "Buildings, walls, boats and the loads people carry are drawn out of the sampling \
             boxes unless there is a picture for them. A picture comes out at the size it was \
             drawn: its own pixels against the art resolution below, never stretched to the box \
             the generator would have filled, and stood on the front edge of the footprint with \
             whatever it does not cover hanging evenly either side. A thing with a picture for \
             one state only is drawn from it in that state and generated the rest of the time.",
        ),
        art_scale_row(app, h),
        crate::ui::decode::scale_field(
            app,
            h,
            "made",
            "how much of a dropped picture goes to one pixel of the thing; 0 works it out \
             from the picture",
        ),
        app_bool(
            h,
            "Draw made things from pictures",
            made.enabled,
            Some("off keeps every picture and goes back to the generated shapes"),
            |app, v| {
                app.state.civ.made.enabled = v;
                app.sprites_changed();
            },
        ),
        made_search(app, h),
    ];

    let query = app.ui.made_search.clone();
    // With nothing typed and the switch off, only what has been given a
    // picture is listed: a wall of empty slots is not a menu.
    let show_all = app.ui.made_all || !query.is_empty();
    let list = el("div").class("group-body").get();
    let (shown, hidden) = with_made_index(|index| {
        let hits = index.search(Search {
            query: &query,
            by_meaning: app.ui.made_meaning,
            here: ("", ""),
            limit: if query.is_empty() { usize::MAX } else { 40 },
        });
        let (mut shown, mut hidden) = (0, 0);
        for hit in hits {
            let entry = &index.entries[hit.idx];
            if !show_all && made.slot(&entry.anchor).is_none() {
                hidden += 1;
                continue;
            }
            shown += 1;
            let _ = list.append_child(&made_row(app, h, entry));
        }
        (shown, hidden)
    });
    if shown == 0 {
        rows.push(note(if query.is_empty() {
            "Nothing has a picture yet. Turn on Every slot, or search for the thing you want one \
             for."
        } else {
            "Nothing by that name."
        }));
    } else {
        rows.push(list);
    }
    if hidden > 0 {
        rows.push(note(&format!("{hidden} more without a picture, behind Every slot.")));
    }

    let bytes = made.bytes();
    if bytes > 0 {
        rows.push(note(&format!("Pictures in this project: {}.", size_text(bytes))));
    }
    section("Pictures for made things", rows)
}

/// The box that narrows the list, and the two switches beside it. None of this
/// is the project, so none of it is recorded for undo.
fn made_search(app: &App, h: &Handle) -> Element {
    let box_ = input_el("search");
    box_.set_value(&app.ui.made_search);
    let _ = box_.set_attribute("placeholder", "Search the things");
    {
        let h2 = h.clone();
        on(box_.unchecked_ref(), "input", Scope::Panel, move |e| {
            let mut sh = h2.borrow_mut();
            sh.app.ui.made_search = crate::ui::value_of(&e);
            sh.app.rebuild_panel();
        });
    }
    let row = el("div").class("made-search").child(box_.unchecked_ref()).get();

    let h2 = h.clone();
    let _ = row.append_child(&crate::ui::toggle_button(
        "Every slot",
        app.ui.made_all,
        Scope::Panel,
        move |on| {
            let mut sh = h2.borrow_mut();
            sh.app.ui.made_all = on;
            sh.app.rebuild_panel();
        },
    ));
    if with_made_index(|index| index.has_terms()) {
        let h3 = h.clone();
        let _ = row.append_child(&crate::ui::toggle_button(
            "Meaning",
            app.ui.made_meaning,
            Scope::Panel,
            move |on| {
                let mut sh = h3.borrow_mut();
                sh.app.ui.made_meaning = on;
                sh.app.rebuild_panel();
            },
        ));
    }
    row
}

thread_local! {
    /// Every thing and state as something the menu ranker can search. Built
    /// once: the catalog does not change while the page is open.
    static MADE_INDEX: Index = build_made_index();
}

fn with_made_index<R>(f: impl FnOnce(&Index) -> R) -> R {
    MADE_INDEX.with(f)
}

fn build_made_index() -> Index {
    let mut index = Index::new(crate::civ::sprites::made_entries());
    if let Ok(terms) = serde_json::from_str::<crate::find::Terms>(crate::find::MADE_TERMS_JSON) {
        index.set_terms(terms);
    }
    index
}

/// One thing in one state: its picture and the way to change it.
fn made_row(app: &App, h: &Handle, entry: &Entry) -> Element {
    let key = entry.anchor.clone();
    let clip = app.state.civ.made.slot(&key);
    let row = el("div")
        .class("made-slot")
        .attr("data-find", &crate::ui::slug(&entry.label))
        .child(
            &el("span")
                .class("field-label")
                .text(&entry.label)
                .child(&el("span").class("field-hint").text(&entry.group).get())
                .get(),
        )
        .child(&drop_zone(h, Slot::Made(key.clone()), clip))
        .get();
    if let Some(c) = clip.filter(|c| c.ready()) {
        let (dw, dh) = c.drawn_cells(app.state.civ.world.cell_px, app.state.civ.art_px_per_cell);
        let _ = row.append_child(
            &el("span")
                .class("field-hint")
                .text(&format!(
                    "{}x{} px, drawn {dw:.1}x{dh:.1} cells",
                    c.frame_w(),
                    c.h
                ))
                .get(),
        );
        let _ = row.append_child(&made_num(h, &key, c.scale));
    }
    if clip.is_some() {
        let h2 = h.clone();
        let _ = row.append_child(&crate::ui::danger_button("Clear", Scope::Panel, move || {
            let mut sh = h2.borrow_mut();
            sh.app.record("made art", false);
            sh.app.state.civ.made.clear(&key);
            sh.app.sprites_changed();
            sh.app.rebuild_panel();
        }));
    }
    row
}

/// The scale of one picture, changed in place. A made slot has no playback to
/// tune, so this is the whole of it: how large the art comes out, against what
/// it was drawn at.
fn made_num(h: &Handle, key: &str, value: f64) -> Element {
    let h2 = h.clone();
    let key = key.to_string();
    number_field(
        "Scale",
        value,
        NumOpts { min: MIN_SCALE, max: MAX_SCALE, step: 0.05 },
        None,
        move |v| {
            let mut sh = h2.borrow_mut();
            sh.app.record("made scale", true);
            if let Some(clip) = sh.app.state.civ.made.slot_mut(&key) {
                clip.scale = v;
            }
            sh.app.state.civ.made.touch();
            sh.app.sprites_changed();
        },
    )
}

fn size_text(bytes: usize) -> String {
    if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f64 / (1 << 20) as f64)
    } else {
        format!("{} kB", (bytes as f64 / 1024.0).round() as i64)
    }
}

fn meta_text(app: &App, clip: Option<&Clip>) -> String {
    match clip.filter(|c| c.ready()) {
        Some(c) => {
            let (w, h) =
                c.drawn_cells(app.state.civ.world.cell_px, app.state.civ.art_px_per_cell);
            format!(
                "{} frames, {}x{} px, drawn {w:.1}x{h:.1} cells",
                c.frame_count(),
                c.frame_w(),
                c.h
            )
        }
        None => "nothing dropped".to_string(),
    }
}

fn slot_card(app: &App, h: &Handle, motion: Motion) -> Element {
    let clip = app.state.civ.sprites.clip(motion);
    let meta = el("span").class("sprite-meta").text(&meta_text(app, clip)).get();
    let head = el("header")
        .class("sprite-head")
        .child(&el("span").class("sprite-name").text(motion.label()).get())
        .child(&meta)
        .get();
    let mut body = vec![
        head,
        el("p").class("field-hint").text(motion.hint()).get(),
        drop_zone(h, Slot::Motion(motion), clip),
        sheet_row(app, h, motion),
    ];
    if let Some(c) = clip.filter(|c| c.ready()) {
        let per_unit = if c.stride { "frames per cell walked" } else { "frames per second" };
        body.push(clip_num(
            h,
            motion,
            &meta,
            "Frames",
            c.frame_count() as f64,
            1.0,
            MAX_FRAMES as f64,
            1.0,
            Some("how many equal columns the sheet is read as"),
            |clip, v| clip.frames = (v.round() as i32).clamp(1, MAX_FRAMES),
        ));
        body.push(clip_num(
            h,
            motion,
            &meta,
            "Rate",
            c.fps,
            0.0,
            24.0,
            0.5,
            Some(per_unit),
            |clip, v| clip.fps = v,
        ));
        body.push(clip_bool(
            h,
            motion,
            "Tie to steps",
            c.stride,
            Some("the frame follows ground covered, so a walk never slides"),
            |clip, v| clip.stride = v,
        ));
        body.push(clip_num(
            h,
            motion,
            &meta,
            "Scale",
            c.scale,
            MIN_SCALE,
            MAX_SCALE,
            0.05,
            Some("1 is the art at its own size; both sides move together, so the frame's shape is never changed"),
            |clip, v| clip.scale = v,
        ));
        body.push(clip_num(
            h,
            motion,
            &meta,
            "Lift (cells)",
            c.lift,
            -1.0,
            2.0,
            0.05,
            Some("raises the art off the ground, for frames drawn with their own footing"),
            |clip, v| clip.lift = v,
        ));
        body.push(clip_bool(
            h,
            motion,
            "Mirror when facing left",
            c.flip,
            Some("off for art drawn facing the viewer"),
            |clip, v| clip.flip = v,
        ));
        body.push(clip_bool(
            h,
            motion,
            "Mirror the art",
            c.mirror,
            Some("for a sheet drawn facing the other way than the person walks"),
            |clip, v| clip.mirror = v,
        ));
        if let Some(sheet) = app.state.art.find(&c.sheet) {
            let h2 = h.clone();
            let id = sheet.id.clone();
            body.push(
                el("div")
                    .class("btn-row")
                    .child(&el("span").class("field-hint").text("drawn in the editor").get())
                    .child(&button(&format!("Take {} again", sheet.name), Scope::Panel, move || {
                        let mut sh = h2.borrow_mut();
                        let id = id.clone();
                        build_from_sheet(&mut sh.app, motion, &id);
                    }))
                    .get(),
            );
        }
        let h2 = h.clone();
        body.push(
            el("div")
                .class("btn-row")
                .child(&danger_button("Clear", Scope::Panel, move || {
                    let mut sh = h2.borrow_mut();
                    sh.app.state.civ.sprites.set(motion, None);
                    sh.app.sprites_changed();
                    sh.app.rebuild_panel = true;
                }))
                .get(),
        );
    }
    el("div").class("sprite-slot").children(body).get()
}

/// Pointing a motion at a sheet drawn in the sprite editor, which is the other
/// way art gets here. The sheet is copied into a clip rather than followed, so
/// carrying on drawing does not change the people until it is sent again.
fn sheet_row(app: &App, h: &Handle, motion: Motion) -> Element {
    let options = app.state.art.options();
    if options.is_empty() {
        return el("div").get();
    }
    let clip = app.state.civ.sprites.clip(motion);
    // A motion that already came from a sheet points at that one, so the
    // button beside it means "the same sheet again" rather than "some sheet".
    let held = clip.map(|c| c.sheet.clone()).unwrap_or_default();
    let first = if options.iter().any(|(id, _)| *id == held) {
        held
    } else {
        options.first().map(|(id, _)| id.clone()).unwrap_or_default()
    };
    let chosen = Rc::new(RefCell::new(first.clone()));
    let picker = {
        let chosen = chosen.clone();
        select_field("From editor", &first, &options, None, move |v| {
            *chosen.borrow_mut() = v;
        })
    };
    let state = clip.map(|c| c.against(app.state.art.find(&c.sheet)));
    let label = match state {
        Some(FromSheet::Behind) => "Take again",
        Some(FromSheet::Current) => "Taken",
        _ => "Use sheet",
    };
    let send = {
        let h2 = h.clone();
        let chosen = chosen.clone();
        button(label, Scope::Panel, move || {
            let id = chosen.borrow().clone();
            let mut sh = h2.borrow_mut();
            build_from_sheet(&mut sh.app, motion, &id);
        })
    };
    if state == Some(FromSheet::Behind) {
        let _ = send.class_list().add_1("accent");
    }
    let row = el("div").class("sprite-from").child(&picker).child(&send).get();
    if let Some(said) = sheet_state_text(app, clip) {
        let class = if state == Some(FromSheet::Current) { "field-hint" } else { "field-hint stale" };
        let _ = row.append_child(&el("span").class(class).text(&said).get());
    }
    row
}

/// What to say under the picker about the sheet this motion came from. None
/// when it did not come from one, which is the case the row already reads as.
fn sheet_state_text(app: &App, clip: Option<&Clip>) -> Option<String> {
    let clip = clip.filter(|c| c.ready())?;
    let sheet = app.state.art.find(&clip.sheet);
    let name = sheet.map(|s| s.name.clone()).unwrap_or_else(|| clip.sheet.clone());
    match clip.against(sheet) {
        FromSheet::Dropped => None,
        FromSheet::Current => Some(format!("from {name}, which has not been drawn on since")),
        FromSheet::Behind => {
            Some(format!("from {name}, which has been drawn on since - take it again to catch up"))
        }
        FromSheet::Gone => Some(format!("from {name}, which is no longer in the project")),
    }
}

/// Builds a motion's clip from a sheet, whether it is being pointed at one for
/// the first time or being sent the same sheet again.
pub fn build_from_sheet(app: &mut App, motion: Motion, id: &str) {
    match app.state.art.find(id).and_then(Clip::from_sheet) {
        Some(clip) => {
            app.state.civ.sprites.enabled = true;
            apply_clip(app, motion, clip);
        }
        None => app.set_note("nothing drawn on that sheet"),
    }
    app.rebuild_panel = true;
}

// ---- the drop target -----------------------------------------------------

fn drop_zone(h: &Handle, slot: Slot, clip: Option<&Clip>) -> Element {
    let picker = input_el("file").tap(|i| {
        i.set_accept("image/*");
        i.set_multiple(true);
        let _ = i.set_attribute("hidden", "hidden");
    });

    let zone = el("div").class("dropzone").get();
    if let Some(c) = clip.filter(|c| c.ready()) {
        if let Some(strip) = strip_canvas(c) {
            let _ = zone.append_child(&strip);
        }
    }
    let hint = if clip.is_some_and(|c| c.ready()) {
        "Drop images to replace"
    } else {
        "Drop a strip, or one image per frame"
    };
    let _ = zone.append_child(&el("span").class("dropzone-hint").text(hint).get());
    let _ = zone.append_child(picker.unchecked_ref());

    for event in ["dragenter", "dragover"] {
        let zone2 = zone.clone();
        on(zone.unchecked_ref(), event, Scope::Panel, move |e: Event| {
            e.prevent_default();
            let _ = zone2.class_list().add_1("over");
        });
    }
    {
        let zone2 = zone.clone();
        on(zone.unchecked_ref(), "dragleave", Scope::Panel, move |_| {
            let _ = zone2.class_list().remove_1("over");
        });
    }
    {
        let zone2 = zone.clone();
        let h2 = h.clone();
        let slot = slot.clone();
        on(zone.unchecked_ref(), "drop", Scope::Panel, move |e: Event| {
            e.prevent_default();
            let _ = zone2.class_list().remove_1("over");
            let files = e
                .dyn_ref::<DragEvent>()
                .and_then(|d| d.data_transfer())
                .and_then(|t| t.files());
            if let Some(files) = files {
                load_files(&h2, slot.clone(), files);
            }
        });
    }
    {
        // The same thing without a mouse that can drag: the hidden picker is
        // what a click and a keyboard both reach.
        let picker2 = picker.clone();
        on(zone.unchecked_ref(), "click", Scope::Panel, move |_| {
            picker2.click();
        });
    }
    {
        let picker2 = picker.clone();
        on(zone.unchecked_ref(), "keydown", Scope::Panel, move |e: Event| {
            let key = match e.dyn_ref::<web_sys::KeyboardEvent>() {
                Some(k) => k.key(),
                None => return,
            };
            if key == "Enter" || key == " " {
                e.prevent_default();
                picker2.click();
            }
        });
    }
    {
        // The picker sits inside the zone, so the click that opens it would
        // bubble straight back to the handler that opened it.
        on(picker.unchecked_ref(), "click", Scope::Panel, move |e: Event| {
            e.stop_propagation();
        });
    }
    {
        let picker2 = picker.clone();
        let h2 = h.clone();
        let slot = slot.clone();
        on(picker.unchecked_ref(), "change", Scope::Panel, move |_| {
            if let Some(files) = picker2.files() {
                load_files(&h2, slot.clone(), files);
            }
            picker2.set_value("");
        });
    }
    let _ = zone.set_attribute("role", "button");
    let _ = zone.set_attribute("tabindex", "0");
    zone
}

/// The sheet as it will be read: every frame, side by side, at one pixel each.
/// The canvas is sized in source pixels and scaled by the stylesheet, so the
/// preview is the art rather than a resampling of it.
fn strip_canvas(clip: &Clip) -> Option<Element> {
    let fw = clip.frame_w();
    let frames = clip.frame_count();
    let (w, h) = (fw * frames, clip.h);
    if w <= 0 || h <= 0 {
        return None;
    }
    let canvas = document()
        .create_element("canvas")
        .ok()?
        .dyn_into::<HtmlCanvasElement>()
        .ok()?;
    canvas.set_class_name("sprite-strip");
    canvas.set_width(w as u32);
    canvas.set_height(h as u32);
    let ctx = canvas
        .get_context("2d")
        .ok()??
        .dyn_into::<CanvasRenderingContext2d>()
        .ok()?;
    let mut bytes = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let c = unpack_rgba(clip.pixel(x / fw, x % fw, y));
            bytes.extend_from_slice(&[c.r, c.g, c.b, c.a]);
        }
    }
    let image =
        ImageData::new_with_u8_clamped_array_and_sh(Clamped(&bytes), w as u32, h as u32).ok()?;
    let _ = ctx.put_image_data(&image, 0.0, 0.0);
    Some(canvas.unchecked_into())
}

// ---- reading what was dropped --------------------------------------------

fn load_files(h: &Handle, slot: Slot, files: FileList) {
    let h = h.clone();
    crate::ui::decode::read_files(files, move |frames, strip, source| {
        apply(&h, slot.clone(), frames, strip, &source);
    });
}

fn apply(h: &Handle, slot: Slot, frames: Vec<Frame>, strip: bool, source: &str) {
    let mut sh = h.borrow_mut();
    // Read down to the size it was drawn at first: everything below counts
    // pixels - the columns a strip is cut into, how many cells the art covers
    // - and all of it is about the art rather than about how large a copy of
    // it was handed over.
    let want = crate::ui::decode::scale_of(&sh.app, slot.scale_key());
    let (frames, n) = crate::ui::decode::scaled(frames, want);
    // A person's motion is an animation, so a single image is read as a strip
    // of equal frames. A thing people make stands still and is drawn from its
    // first frame, so guessing at columns there would only cut a wide picture
    // of a barn into pieces of one.
    let animated = matches!(slot, Slot::Motion(_));
    let built = if strip {
        frames.into_iter().next().and_then(|(w, height, px)| {
            let frames = if animated { guess_frames(w, height) } else { 1 };
            Clip::from_strip(w, height, px, frames, source.to_string())
        })
    } else {
        Clip::from_frames(frames, source.to_string())
    };
    match built {
        Some(clip) => {
            apply_to(&mut sh.app, &slot, clip);
            if n > 1 {
                sh.app.set_note(&format!("{source} read at {n} px per pixel"));
            }
        }
        None => sh.app.set_note("nothing readable in that drop"),
    }
    sh.app.rebuild_panel = true;
}

/// Drops a freshly built clip into a motion. Playback that has already been
/// tuned for this motion outlives the art it was tuned on; only a fresh slot
/// takes the defaults. Which sheet a clip came from is part of the art rather
/// than part of the tuning, so it is not carried over.
/// Puts a clip in whichever kind of slot it was dropped on.
pub fn apply_to(app: &mut App, slot: &Slot, clip: Clip) {
    match slot {
        Slot::Motion(m) => apply_clip(app, *m, clip),
        Slot::Made(id) => apply_made(app, id, clip),
    }
}

/// A picture for a thing people make. Nothing here has playback to keep: a
/// building stands still, and how large it is drawn is the box it fills rather
/// than a number on the clip.
pub fn apply_made(app: &mut App, id: &str, clip: Clip) {
    app.record("made art", false);
    let count = clip.frame_count();
    app.state.civ.made.enabled = true;
    app.state.civ.made.set(id, clip);
    app.set_note(&format!("{id}: {count} frames"));
    app.sprites_changed();
}

pub fn apply_clip(app: &mut App, motion: Motion, mut clip: Clip) {
    app.record("person art", false);
    match app.state.civ.sprites.clip(motion) {
        Some(old) => {
            clip.fps = old.fps;
            clip.stride = old.stride;
            // A scale the new art brought with it is that art putting back what
            // it lost fitting the cap, and outranks the old tuning; a clip that
            // came in at its own size keeps whatever the slot was set to.
            if clip.scale == 1.0 {
                clip.scale = old.scale;
            }
            clip.lift = old.lift;
            clip.flip = old.flip;
            clip.mirror = old.mirror;
        }
        None => {
            let (fps, stride) = motion.playback();
            if clip.fps <= 0.0 {
                clip.fps = fps;
            }
            clip.stride = stride;
        }
    }
    let count = clip.frame_count();
    app.state.civ.sprites.set(motion, Some(clip));
    app.set_note(&format!("{}: {count} frames", motion.label().to_lowercase()));
    app.sprites_changed();
}

// ---- fields --------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn clip_num(
    h: &Handle,
    motion: Motion,
    meta: &Element,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    hint: Option<&str>,
    apply: impl Fn(&mut Clip, f64) + 'static,
) -> Element {
    let h2 = h.clone();
    let meta = meta.clone();
    let key = label.to_string();
    number_field(label, value, NumOpts { min, max, step }, hint, move |v| {
        let mut sh = h2.borrow_mut();
        sh.app.record(&key, true);
        if let Some(clip) = sh.app.state.civ.sprites.slot_mut(motion).as_mut() {
            apply(clip, v);
        }
        // The header carries the frame size, which the frame count changes, and
        // rebuilding the whole panel on every drag of a slider would not.
        let text = meta_text(&sh.app, sh.app.state.civ.sprites.clip(motion));
        meta.set_text_content(Some(&text));
        sh.app.sprites_changed();
    })
}

fn clip_bool(
    h: &Handle,
    motion: Motion,
    label: &str,
    value: bool,
    hint: Option<&str>,
    apply: impl Fn(&mut Clip, bool) + 'static,
) -> Element {
    let h2 = h.clone();
    let key = label.to_string();
    crate::ui::bool_field(label, value, hint, move |v| {
        let mut sh = h2.borrow_mut();
        sh.app.record(&key, false);
        if let Some(clip) = sh.app.state.civ.sprites.slot_mut(motion).as_mut() {
            apply(clip, v);
        }
        sh.app.sprites_changed();
    })
}
