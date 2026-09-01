//! Economy: prices, wages, the treasury and the caravans.
//!
//! Prices are not set anywhere, they fall out of stock against demand: every
//! resource has a target stock that grows with the population, and its price is
//! the base price scaled by how far the store is from that target. Wages are
//! paid out of the treasury as work is done, people buy their food back from
//! the market, and caravans move coin in and out by trading the surplus.

use serde::{Deserialize, Serialize};

use crate::civ::names::caravan_name;
use crate::civ::people::Person;
use crate::civ::resources::{add_stock, take_stock, Res, ResKind, Stock, RES_COUNT, RES_IDS};
use crate::civ::tech::Mods;
use crate::rng::Rng;
use crate::util::clamp;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EconomyConfig {
    pub start_coin: f64,
    pub wage: f64,
    pub elasticity: f64,
    pub price_smoothing: f64,
    /// Target stock per person, multiplied by a per resource weight below.
    pub stock_per_person: f64,
    pub raw_weight: f64,
    pub made_weight: f64,
    /// Stock above this multiple of the target is not worth carrying home.
    pub hoard_limit: f64,
    pub trade_interval: f64,
    pub trade_volume: f64,
    pub trade_margin: f64,
    pub caravan_coin: f64,
    pub pays_wages: bool,
    pub history_length: usize,
}

impl Default for EconomyConfig {
    fn default() -> Self {
        EconomyConfig {
            start_coin: 80.0,
            wage: 0.5,
            elasticity: 0.85,
            price_smoothing: 0.25,
            stock_per_person: 4.0,
            raw_weight: 1.6,
            made_weight: 0.6,
            hoard_limit: 2.5,
            trade_interval: 100.0,
            trade_volume: 40.0,
            trade_margin: 0.25,
            caravan_coin: 240.0,
            pays_wages: true,
            history_length: 320,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Sample {
    pub day: i32,
    pub pop: f64,
    pub coin: f64,
    pub food: f64,
    pub wood: f64,
    pub research: f64,
    pub buildings: f64,
    pub happiness: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub text: String,
    pub day: i32,
}

#[derive(Serialize, Deserialize)]
pub struct Economy {
    pub coin: f64,
    pub prices: Stock,
    pub produced: Stock,
    pub consumed: Stock,
    pub rate_in: Stock,
    pub rate_out: Stock,
    pub history: Vec<Sample>,
    pub events: Vec<Event>,
    pub unpaid_wages: f64,
    pub trade_timer: f64,
    pub trades: u32,
    pub trade_balance: f64,
}

impl Economy {
    pub fn new(cfg: &EconomyConfig) -> Self {
        let mut prices = [0.0; RES_COUNT];
        for res in RES_IDS {
            prices[res as usize] = res.def().base_price;
        }
        Economy {
            coin: cfg.start_coin,
            prices,
            produced: [0.0; RES_COUNT],
            consumed: [0.0; RES_COUNT],
            rate_in: [0.0; RES_COUNT],
            rate_out: [0.0; RES_COUNT],
            history: Vec::new(),
            events: Vec::new(),
            unpaid_wages: 0.0,
            trade_timer: 0.0,
            trades: 0,
            trade_balance: 0.0,
        }
    }

    pub fn price_of(&self, res: Res) -> f64 {
        self.prices[res as usize]
    }

    pub fn record_produced(&mut self, res: Res, n: f64) {
        self.produced[res as usize] += n;
    }

    pub fn record_consumed(&mut self, res: Res, n: f64) {
        self.consumed[res as usize] += n;
    }

    /// Per day flow rates, smoothed, so the panel shows a trend rather than the
    /// spike of whatever happened in the last tick.
    pub fn roll_flows(&mut self) {
        for res in RES_IDS {
            let i = res as usize;
            self.rate_in[i] = self.rate_in[i] * 0.5 + self.produced[i] * 0.5;
            self.rate_out[i] = self.rate_out[i] * 0.5 + self.consumed[i] * 0.5;
            self.produced[i] = 0.0;
            self.consumed[i] = 0.0;
        }
    }

    pub fn push_history(&mut self, cfg: &EconomyConfig, sample: Sample) {
        self.history.push(sample);
        let max = cfg.history_length.max(20);
        if self.history.len() > max {
            let drop = self.history.len() - max;
            self.history.drain(0..drop);
        }
    }

    pub fn log_event(&mut self, text: String, day: i32) {
        self.events.push(Event { text, day });
        if self.events.len() > 60 {
            self.events.remove(0);
        }
    }
}

pub fn stock_targets(cfg: &EconomyConfig, population: usize) -> Stock {
    let pop = population.max(1) as f64;
    let mut out = [0.0; RES_COUNT];
    for res in RES_IDS {
        let weight = if res.def().kind == ResKind::Raw {
            cfg.raw_weight
        } else {
            cfg.made_weight
        };
        out[res as usize] = (pop * cfg.stock_per_person * weight).max(4.0);
    }
    out
}

pub fn update_prices(econ: &mut Economy, cfg: &EconomyConfig, stock: &Stock, population: usize, dt: f64) {
    let targets = stock_targets(cfg, population);
    for res in RES_IDS {
        let i = res as usize;
        let target = targets[i];
        let have = stock[i];
        let scarcity = clamp((target + 1.0) / (have + 1.0), 0.2, 6.0);
        let want = res.def().base_price * scarcity.powf(cfg.elasticity);
        let k = clamp(cfg.price_smoothing * dt, 0.0, 1.0);
        econ.prices[i] += (want - econ.prices[i]) * k;
    }
}

/// Wages are paid as work happens. An empty treasury does not stop the work, it
/// just leaves the wage unpaid, which shows up as unhappiness.
///
/// Only `savings` of a wage stays in the person's purse; the rest is spent
/// back into the town the same day and returns to the treasury. That share is
/// the whole reason some people end up with enough coin to rebuild their
/// house and most do not.
pub fn pay_wage(
    econ: &mut Economy,
    cfg: &EconomyConfig,
    person: &mut Person,
    work_units: f64,
    savings: f64,
) -> f64 {
    if !cfg.pays_wages {
        return 0.0;
    }
    let due = cfg.wage * work_units;
    if econ.coin < due {
        econ.unpaid_wages += due;
        return 0.0;
    }
    econ.coin -= due;
    let kept = due * savings.clamp(0.0, 1.0);
    person.earn(kept);
    econ.coin += due - kept;
    kept
}

/// A meal is bought from the settlement store at the market price when there is
/// a market; without one people simply take what they need.
pub fn buy_food(
    econ: &mut Economy,
    person: &mut Person,
    stock: &mut Stock,
    amount: f64,
    has_market: bool,
) -> f64 {
    let got = take_stock(stock, Res::Food, amount);
    if got <= 0.0 {
        return 0.0;
    }
    if has_market {
        let cost = econ.price_of(Res::Food) * got;
        let paid = person.coin.min(cost);
        person.coin -= paid;
        econ.coin += paid;
    }
    econ.record_consumed(Res::Food, got);
    got
}

pub struct TradeReport {
    pub sold: Vec<String>,
    pub bought: Vec<String>,
}

/// One caravan visit: it sells what the settlement is short of and buys the
/// surplus, both at the market price shifted by the trade margin.
pub fn run_caravan(
    econ: &mut Economy,
    cfg: &EconomyConfig,
    stock: &mut Stock,
    population: usize,
    mods: &Mods,
    rng: &mut Rng,
    day: i32,
) -> TradeReport {
    let targets = stock_targets(cfg, population);
    let margin = cfg.trade_margin / mods.trade.max(0.2);
    let name = caravan_name(rng);
    let mut purse = cfg.caravan_coin;
    let mut volume = cfg.trade_volume;
    let mut sold = Vec::new();
    let mut bought = Vec::new();

    // The settlement sells its surplus first, which is where its coin comes
    // from.
    let mut surplus: Vec<(Res, f64)> = RES_IDS
        .iter()
        .map(|&res| (res, stock[res as usize] - targets[res as usize] * 1.3))
        .filter(|&(_, over)| over > 1.0)
        .collect();
    surplus.sort_by(|a, b| {
        (b.1 * econ.price_of(b.0))
            .partial_cmp(&(a.1 * econ.price_of(a.0)))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (res, over) in surplus {
        if volume <= 0.0 || purse <= 0.0 {
            break;
        }
        let unit = econ.price_of(res) * (1.0 - margin);
        let affordable = (purse / unit.max(0.01)).floor();
        let n = over.floor().min(volume).min(affordable);
        if n <= 0.0 {
            continue;
        }
        take_stock(stock, res, n);
        let gain = n * unit;
        econ.coin += gain;
        purse -= gain;
        volume -= n;
        econ.trade_balance += gain;
        sold.push(format!("{} {}", n as i64, res.label().to_lowercase()));
    }

    // Then it sells the settlement what it is short of, if there is coin for it.
    let mut shortage: Vec<(Res, f64)> = RES_IDS
        .iter()
        .map(|&res| (res, targets[res as usize] * 0.5 - stock[res as usize]))
        .filter(|&(_, short)| short > 1.0)
        .collect();
    shortage.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    for (res, short) in shortage {
        if volume <= 0.0 || econ.coin <= 0.0 {
            break;
        }
        let unit = econ.price_of(res) * (1.0 + margin);
        let affordable = (econ.coin / unit.max(0.01)).floor();
        let n = short.ceil().min(volume).min(affordable).min(30.0);
        if n <= 0.0 {
            continue;
        }
        add_stock(stock, res, n);
        let spend = n * unit;
        econ.coin -= spend;
        volume -= n;
        econ.trade_balance -= spend;
        bought.push(format!("{} {}", n as i64, res.label().to_lowercase()));
    }

    econ.trades += 1;
    let mut parts: Vec<String> = Vec::new();
    if !sold.is_empty() {
        parts.push(format!("bought {}", sold.join(", ")));
    }
    if !bought.is_empty() {
        parts.push(format!("sold us {}", bought.join(", ")));
    }
    let what = if parts.is_empty() {
        "found nothing to trade".to_string()
    } else {
        parts.join(" and ")
    };
    econ.log_event(format!("{name} {what}"), day);
    TradeReport { sold, bought }
}

pub fn net_worth(econ: &Economy, stock: &Stock) -> f64 {
    let mut value = econ.coin;
    for res in RES_IDS {
        value += stock[res as usize] * econ.price_of(res);
    }
    value
}
