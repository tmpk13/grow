//! Resources: what the settlement gathers, refines, stores and trades.
//!
//! Everything downstream (recipes, building costs, prices, the stock panel) is
//! generated from this table, so adding a resource here is enough to make it
//! appear everywhere it is relevant.

use serde::{Deserialize, Deserializer, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum Res {
    Wood = 0,
    Stone = 1,
    Clay = 2,
    Ore = 3,
    Fiber = 4,
    Food = 5,
    Plank = 6,
    Brick = 7,
    Charcoal = 8,
    Metal = 9,
    Tool = 10,
    Cloth = 11,
}

pub const RES_COUNT: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResKind {
    Raw,
    Made,
}

pub struct ResDef {
    pub res: Res,
    pub id: &'static str,
    pub label: &'static str,
    pub kind: ResKind,
    pub color: &'static str,
    pub base_price: f64,
    pub bulk: f64,
    pub decay: f64,
}

pub static RESOURCES: [ResDef; RES_COUNT] = [
    ResDef { res: Res::Wood, id: "wood", label: "Wood", kind: ResKind::Raw, color: "#8a6644", base_price: 2.0, bulk: 1.0, decay: 0.0 },
    ResDef { res: Res::Stone, id: "stone", label: "Stone", kind: ResKind::Raw, color: "#8d97a3", base_price: 3.0, bulk: 1.4, decay: 0.0 },
    ResDef { res: Res::Clay, id: "clay", label: "Clay", kind: ResKind::Raw, color: "#b07a5a", base_price: 2.0, bulk: 1.2, decay: 0.0 },
    ResDef { res: Res::Ore, id: "ore", label: "Ore", kind: ResKind::Raw, color: "#7d6f8f", base_price: 6.0, bulk: 1.5, decay: 0.0 },
    ResDef { res: Res::Fiber, id: "fiber", label: "Fiber", kind: ResKind::Raw, color: "#c8b46a", base_price: 2.0, bulk: 0.6, decay: 0.0 },
    ResDef { res: Res::Food, id: "food", label: "Food", kind: ResKind::Raw, color: "#9fd06a", base_price: 3.0, bulk: 0.8, decay: 0.0004 },
    ResDef { res: Res::Plank, id: "plank", label: "Plank", kind: ResKind::Made, color: "#c39a63", base_price: 6.0, bulk: 1.0, decay: 0.0 },
    ResDef { res: Res::Brick, id: "brick", label: "Brick", kind: ResKind::Made, color: "#c06a4e", base_price: 7.0, bulk: 1.6, decay: 0.0 },
    ResDef { res: Res::Charcoal, id: "charcoal", label: "Charcoal", kind: ResKind::Made, color: "#57575f", base_price: 5.0, bulk: 0.7, decay: 0.0 },
    ResDef { res: Res::Metal, id: "metal", label: "Metal", kind: ResKind::Made, color: "#9fb6c9", base_price: 14.0, bulk: 1.3, decay: 0.0 },
    ResDef { res: Res::Tool, id: "tool", label: "Tool", kind: ResKind::Made, color: "#d8d2b8", base_price: 22.0, bulk: 0.9, decay: 0.0 },
    ResDef { res: Res::Cloth, id: "cloth", label: "Cloth", kind: ResKind::Made, color: "#cf8fb0", base_price: 10.0, bulk: 0.5, decay: 0.0 },
];

pub const RES_IDS: [Res; RES_COUNT] = [
    Res::Wood,
    Res::Stone,
    Res::Clay,
    Res::Ore,
    Res::Fiber,
    Res::Food,
    Res::Plank,
    Res::Brick,
    Res::Charcoal,
    Res::Metal,
    Res::Tool,
    Res::Cloth,
];

impl Res {
    pub fn def(self) -> &'static ResDef {
        &RESOURCES[self as usize]
    }

    pub fn id(self) -> &'static str {
        self.def().id
    }

    pub fn label(self) -> &'static str {
        self.def().label
    }

    pub fn from_id(id: &str) -> Option<Res> {
        RESOURCES.iter().find(|d| d.id == id).map(|d| d.res)
    }
}

/// One number per resource. Stocks, benches and reservations all use this.
pub type Stock = [f64; RES_COUNT];

pub fn make_stock(fill: f64) -> Stock {
    [fill; RES_COUNT]
}

pub fn add_stock(stock: &mut Stock, res: Res, n: f64) -> f64 {
    if n == 0.0 {
        return 0.0;
    }
    stock[res as usize] += n;
    n
}

pub fn take_stock(stock: &mut Stock, res: Res, n: f64) -> f64 {
    let have = stock[res as usize];
    let got = have.min(n);
    stock[res as usize] = have - got;
    got
}

pub fn stock_bulk(stock: &Stock) -> f64 {
    RES_IDS.iter().map(|&r| stock[r as usize] * r.def().bulk).sum()
}

pub fn stock_total(stock: &Stock) -> f64 {
    stock.iter().sum()
}

/// A cost is short (two or three entries), so it stays an ordered list rather
/// than being normalized into a full stock array.
pub type Cost = &'static [(Res, f64)];

pub fn can_afford(stock: &Stock, cost: &[(Res, f64)]) -> bool {
    cost.iter().all(|&(res, n)| stock[res as usize] >= n)
}

pub fn missing_from(stock: &Stock, cost: &[(Res, f64)]) -> Vec<(Res, f64)> {
    cost.iter()
        .filter_map(|&(res, n)| {
            let short = n - stock[res as usize];
            if short > 0.0 {
                Some((res, short))
            } else {
                None
            }
        })
        .collect()
}

pub fn format_cost(cost: &[(Res, f64)]) -> String {
    cost.iter()
        .map(|&(res, n)| format!("{} {}", trim_num(n), res.label().to_lowercase()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn trim_num(n: f64) -> String {
    if (n - n.round()).abs() < 1e-9 {
        format!("{}", n.round() as i64)
    } else {
        format!("{n:.1}")
    }
}

/// Stocks travel through JSON as an object keyed by resource id, which is how
/// the project file has always stored the founding supplies.
pub mod stock_map {
    use super::{Res, Stock, RES_IDS};
    use serde::de::{MapAccess, Visitor};
    use serde::ser::SerializeMap;
    use serde::{Deserializer, Serializer};
    use std::fmt;

    pub fn serialize<S: Serializer>(stock: &Stock, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(RES_IDS.len()))?;
        for res in RES_IDS {
            map.serialize_entry(res.id(), &stock[res as usize])?;
        }
        map.end()
    }

    struct StockVisitor;

    impl<'de> Visitor<'de> for StockVisitor {
        type Value = Stock;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a map of resource id to amount")
        }

        fn visit_map<M: MapAccess<'de>>(self, mut access: M) -> Result<Stock, M::Error> {
            let mut stock = [0.0; super::RES_COUNT];
            while let Some((key, value)) = access.next_entry::<String, f64>()? {
                if let Some(res) = Res::from_id(&key) {
                    stock[res as usize] = value;
                }
            }
            Ok(stock)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Stock, D::Error> {
        d.deserialize_map(StockVisitor)
    }
}

/// A resource id as it appears in JSON, for the few places that store one.
pub fn serialize_res<S: Serializer>(res: &Res, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(res.id())
}

pub fn deserialize_res<'de, D: Deserializer<'de>>(d: D) -> Result<Res, D::Error> {
    let id = String::deserialize(d)?;
    Res::from_id(&id).ok_or_else(|| serde::de::Error::custom("unknown resource"))
}
