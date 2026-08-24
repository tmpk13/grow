//! Economy panel: the store, the prices that come out of it, the treasury and
//! the parameters behind all three.

use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlCanvasElement};

use crate::app::{App, Handle, Panel};
use crate::civ::economy::{net_worth, stock_targets, EconomyConfig};
use crate::civ::resources::{RES_IDS};
use crate::civ::colony::Colony;
use crate::ui::{
    app_bool, app_num, append, clear, colony_picker, el, note, section, stat, window, NumOpts,
};

pub struct EconomyPanel {
    table: Element,
    summary: Element,
    plot: HtmlCanvasElement,
    log: Element,
    since: f64,
}

pub fn build(root: &Element, app: &mut App, h: &Handle) -> Box<dyn Panel> {
    let table = el("div").class("stock-table").get();
    let summary = el("div").class("stat-grid").get();
    let plot = el("canvas")
        .class("plot-canvas")
        .get()
        .dyn_into::<HtmlCanvasElement>()
        .unwrap();
    let plot_wrap = el("div").class("plot-wrap").child(plot.unchecked_ref()).get();
    let log = el("div").class("event-log").get();

    let mut store = Vec::new();
    if let Some(picker) = colony_picker(app, h) {
        store.push(picker);
        store.push(note("Every town keeps its own store, treasury and prices."));
    }
    store.push(summary.clone());
    store.push(table.clone());
    append(root, section("Store", store));
    append(root, section("History", vec![
        plot_wrap,
        note("One sample per day: population, food in store and coin in the treasury."),
    ]));

    let cfg = app.state.civ.economy;
    append(root, section("Prices and money", vec![
        note("Nothing sets a price directly. Each resource has a target stock that grows with the \
              population, and its price is the base price scaled by how far the store is from it."),
        econ_num(h, "Stock target per person", cfg.stock_per_person, 0.5, 20.0, 0.5, None, |c, v| c.stock_per_person = v),
        econ_num(h, "Raw weight", cfg.raw_weight, 0.1, 4.0, 0.1, None, |c, v| c.raw_weight = v),
        econ_num(h, "Made weight", cfg.made_weight, 0.1, 4.0, 0.1, None, |c, v| c.made_weight = v),
        econ_num(h, "Price elasticity", cfg.elasticity, 0.1, 2.5, 0.05, Some("how hard scarcity moves a price"), |c, v| c.elasticity = v),
        econ_num(h, "Price smoothing", cfg.price_smoothing, 0.01, 2.0, 0.01, None, |c, v| c.price_smoothing = v),
        econ_num(h, "Hoard limit", cfg.hoard_limit, 1.0, 8.0, 0.25, Some("stock above this multiple of the target is left on the ground"), |c, v| c.hoard_limit = v),
        econ_num(h, "Starting treasury", cfg.start_coin, 0.0, 2000.0, 10.0, None, |c, v| c.start_coin = v),
        econ_num(h, "Wage per work second", cfg.wage, 0.0, 5.0, 0.05, Some("paid only once a market stands"), |c, v| c.wage = v),
        app_bool(h, "Pay wages", cfg.pays_wages, None, |app, v| {
            app.state.civ.economy.pays_wages = v;
            app.request_save();
        }),
    ]));

    let boats = app.state.civ.boats;
    append(root, section("Boats", vec![
        note("A colony with a dock builds boats there and sends them to the other colonies with \
              whatever it has too much of. They sell into the far town's store at the far town's \
              prices and come home with what this one is short of."),
        boat_num(h, "Boats per dock", boats.per_dock as f64, 0.0, 8.0, 1.0, None, |c, v| c.per_dock = v as i32),
        boat_num(h, "Hold", boats.capacity, 10.0, 400.0, 10.0, Some("units of cargo"), |c, v| c.capacity = v),
        boat_num(h, "Speed", boats.speed, 0.5, 12.0, 0.1, Some("cells per second"), |c, v| c.speed = v),
        boat_num(h, "Crew", boats.crew as f64, 0.0, 6.0, 1.0, Some("dock workers who sail with it"), |c, v| c.crew = v as i32),
        boat_num(h, "Smallest cargo worth sailing", boats.min_cargo, 0.0, 200.0, 1.0, None, |c, v| c.min_cargo = v),
        boat_num(h, "Port time (s)", boats.port_time, 1.0, 200.0, 1.0, None, |c, v| c.port_time = v),
        boat_num(h, "Port margin", boats.margin, 0.0, 0.8, 0.02, Some("what the far market takes on both halves"), |c, v| c.margin = v),
        boat_num(h, "Hull: wood", boats.hull_wood, 0.0, 100.0, 1.0, None, |c, v| c.hull_wood = v),
        boat_num(h, "Hull: planks", boats.hull_plank, 0.0, 100.0, 1.0, None, |c, v| c.hull_plank = v),
    ]));

    append(root, section("Caravans", vec![
        note("A market brings caravans. They buy whatever the town has too much of and sell \
              it what it is short of, both at the town's price shifted by the margin."),
        econ_num(h, "Days between visits", cfg.trade_interval, 10.0, 600.0, 5.0, Some("in simulated seconds"), |c, v| c.trade_interval = v),
        econ_num(h, "Units per visit", cfg.trade_volume, 1.0, 400.0, 1.0, None, |c, v| c.trade_volume = v),
        econ_num(h, "Trade margin", cfg.trade_margin, 0.0, 0.9, 0.05, None, |c, v| c.trade_margin = v),
        econ_num(h, "Caravan purse", cfg.caravan_coin, 0.0, 5000.0, 20.0, None, |c, v| c.caravan_coin = v),
        el("h4").class("sub-head").text("Recent events").get(),
        log.clone(),
    ]));

    let mut panel = EconomyPanel { table, summary, plot, log, since: 0.0 };
    panel.redraw(app);
    Box::new(panel)
}

#[allow(clippy::too_many_arguments)]
fn boat_num(
    h: &Handle,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    hint: Option<&str>,
    apply: fn(&mut crate::civ::boats::BoatConfig, f64),
) -> Element {
    app_num(h, label, value, NumOpts { min, max, step }, hint, move |app, v| {
        apply(&mut app.state.civ.boats, v);
        app.request_save();
    })
}

#[allow(clippy::too_many_arguments)]
fn econ_num(
    h: &Handle,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    hint: Option<&str>,
    apply: fn(&mut EconomyConfig, f64),
) -> Element {
    app_num(h, label, value, NumOpts { min, max, step }, hint, move |app, v| {
        apply(&mut app.state.civ.economy, v);
        app.request_save();
    })
}

fn header_row(labels: &[&str]) -> Element {
    let row = el("div")
        .class("stock-row head")
        .child(&el("span").class("swatch-dot ghost").get())
        .get();
    for (i, l) in labels.iter().enumerate() {
        let class = if i == 0 { "stock-name" } else { "stock-num" };
        let _ = row.append_child(&el("span").class(class).text(l).get());
    }
    row
}

impl Panel for EconomyPanel {
    fn redraw(&mut self, app: &mut App) {
        clear(&self.table);
        clear(&self.summary);
        clear(&self.log);
        let civ = match &app.settlement {
            Some(c) => c,
            None => return,
        };
        let colony = match civ.focus_colony() {
            Some(c) => c,
            None => return,
        };
        let targets = stock_targets(&app.state.civ.economy, colony.population);

        let _ = self.table.append_child(&header_row(&[
            "Resource", "Stock", "Target", "Price", "In/day", "Out/day",
        ]));
        for res in RES_IDS {
            let i = res as usize;
            let row = el("div")
                .class("stock-row")
                .child(
                    &el("span")
                        .class("swatch-dot")
                        .style("background", res.def().color)
                        .get(),
                )
                .child(&el("span").class("stock-name").text(res.label()).get())
                .child(&el("span").class("stock-num").text(&format!("{}", colony.stock[i].round())).get())
                .child(&el("span").class("stock-num dim").text(&format!("{}", targets[i].round())).get())
                .child(&el("span").class("stock-num").text(&format!("{:.1}", colony.econ.price_of(res))).get())
                .child(&el("span").class("stock-num up").text(&format!("{:.0}", colony.econ.rate_in[i])).get())
                .child(&el("span").class("stock-num down").text(&format!("{:.0}", colony.econ.rate_out[i])).get())
                .get();
            let _ = self.table.append_child(&row);
        }

        let bulk = crate::civ::resources::stock_bulk(&colony.stock);
        let purses: f64 = civ.people.iter().filter(|p| p.colony == colony.id).map(|p| p.coin).sum();
        let boats = civ.boats.iter().filter(|b| b.colony == colony.id).count();
        let rows = [
            ("Town".to_string(), colony.name.clone()),
            ("Treasury".to_string(), format!("{} coin", colony.econ.coin.round())),
            ("In purses".to_string(), format!("{} coin", purses.round())),
            ("Net worth".to_string(), format!("{} coin", net_worth(&colony.econ, &colony.stock).round())),
            ("Storage used".to_string(), format!("{} / {}", bulk.round(), colony.storage)),
            ("Caravans".to_string(), colony.econ.trades.to_string()),
            ("Trade balance".to_string(), format!("{} coin", colony.econ.trade_balance.round())),
            ("Unpaid wages".to_string(), format!("{}", colony.econ.unpaid_wages.round())),
            ("Boats".to_string(), boats.to_string()),
            ("Loads on the ground".to_string(), civ.piles.len().to_string()),
        ];
        for (k, v) in rows {
            let _ = self.summary.append_child(&stat(&k, &v));
        }

        for e in colony.econ.events.iter().rev().take(10) {
            let line = el("div").class("event").text(&format!("day {}  {}", e.day, e.text)).get();
            let _ = self.log.append_child(&line);
        }
        draw_history(&self.plot, colony);
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

/// Four series on one plot, each scaled to its own maximum, because the point
/// is the shape of the run rather than the absolute numbers.
fn draw_history(canvas: &HtmlCanvasElement, colony: &Colony) {
    let ctx = match canvas.get_context("2d").ok().flatten() {
        Some(c) => c.dyn_into::<web_sys::CanvasRenderingContext2d>().unwrap(),
        None => return,
    };
    let r = canvas.get_bounding_client_rect();
    let (rw, rh) = (r.width(), r.height());
    if rw == 0.0 {
        return;
    }
    let dpr = window().device_pixel_ratio();
    let w = ((rw * dpr).round() as u32).max(1);
    let h = ((rh * dpr).round() as u32).max(1);
    if canvas.width() != w || canvas.height() != h {
        canvas.set_width(w);
        canvas.set_height(h);
    }
    let _ = ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
    ctx.clear_rect(0.0, 0.0, rw, rh);
    ctx.set_fill_style_str("#0b0f14");
    ctx.fill_rect(0.0, 0.0, rw, rh);
    let history = &colony.econ.history;
    if history.len() < 2 {
        return;
    }

    type Pick = fn(&crate::civ::economy::Sample) -> f64;
    let series: [(Pick, &str); 4] = [
        ((|s| s.pop) as Pick, "#7fd1a0"),
        ((|s| s.food) as Pick, "#9fd06a"),
        ((|s| s.coin) as Pick, "#ffc978"),
        ((|s| s.buildings) as Pick, "#7fb4ff"),
    ];
    for (pick, color) in series {
        let max = history.iter().map(pick).fold(1.0f64, f64::max);
        ctx.set_stroke_style_str(color);
        ctx.set_line_width(1.2);
        ctx.begin_path();
        for (i, sample) in history.iter().enumerate() {
            let x = (i as f64 / (history.len() - 1) as f64) * rw;
            let y = rh - (pick(sample) / max) * (rh - 4.0) - 2.0;
            if i == 0 {
                ctx.move_to(x, y);
            } else {
                ctx.line_to(x, y);
            }
        }
        ctx.stroke();
    }
    ctx.set_fill_style_str("rgba(141, 155, 176, 0.9)");
    ctx.set_font("10px ui-monospace, monospace");
    let _ = ctx.fill_text(
        &format!("day {} - {}", history[0].day, history[history.len() - 1].day),
        4.0,
        11.0,
    );
}
