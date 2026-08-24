//! Species definitions: what a plant is, how fast it appears, how it grows and
//! which sampling boxes its materials come from.
//!
//! Every plant belongs to a size class. The class owns the occupancy layer and
//! the hard ceilings for footprint and height; a species sets its own limits
//! within those ceilings (the effective value is the smaller of the two).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SizeClass {
    Ground,
    Herb,
    Shrub,
    Tree,
    Vine,
}

pub const SIZE_CLASSES: [SizeClass; 5] = [
    SizeClass::Ground,
    SizeClass::Herb,
    SizeClass::Shrub,
    SizeClass::Tree,
    SizeClass::Vine,
];

pub const LAYER_COUNT: usize = SIZE_CLASSES.len();

impl SizeClass {
    pub fn layer(self) -> usize {
        match self {
            SizeClass::Ground => 0,
            SizeClass::Herb => 1,
            SizeClass::Shrub => 2,
            SizeClass::Tree => 3,
            SizeClass::Vine => 4,
        }
    }

    /// Draw order inside one row: flat things before standing ones.
    pub fn order(self) -> usize {
        self.layer()
    }

    pub fn label(self) -> &'static str {
        match self {
            SizeClass::Ground => "Ground cover",
            SizeClass::Herb => "Herb",
            SizeClass::Shrub => "Shrub",
            SizeClass::Tree => "Tree",
            SizeClass::Vine => "Vine",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            SizeClass::Ground => "ground",
            SizeClass::Herb => "herb",
            SizeClass::Shrub => "shrub",
            SizeClass::Tree => "tree",
            SizeClass::Vine => "vine",
        }
    }

    pub fn from_id(id: &str) -> Option<SizeClass> {
        SIZE_CLASSES.iter().copied().find(|c| c.id() == id)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ClassLimit {
    pub max_radius_cells: i32,
    pub max_height_px: f64,
    pub min_spacing: i32,
    pub max_instances: i32,
}

impl Default for ClassLimit {
    fn default() -> Self {
        ClassLimit { max_radius_cells: 2, max_height_px: 56.0, min_spacing: 2, max_instances: 60 }
    }
}

/// Per class ceilings, all settable from the World panel.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ClassLimits {
    pub ground: ClassLimit,
    pub herb: ClassLimit,
    pub shrub: ClassLimit,
    pub tree: ClassLimit,
    pub vine: ClassLimit,
}

impl Default for ClassLimits {
    fn default() -> Self {
        ClassLimits {
            ground: ClassLimit { max_radius_cells: 3, max_height_px: 10.0, min_spacing: 1, max_instances: 160 },
            herb: ClassLimit { max_radius_cells: 1, max_height_px: 26.0, min_spacing: 1, max_instances: 120 },
            shrub: ClassLimit { max_radius_cells: 2, max_height_px: 56.0, min_spacing: 2, max_instances: 60 },
            tree: ClassLimit { max_radius_cells: 5, max_height_px: 150.0, min_spacing: 4, max_instances: 26 },
            vine: ClassLimit { max_radius_cells: 3, max_height_px: 130.0, min_spacing: 3, max_instances: 22 },
        }
    }
}

impl ClassLimits {
    pub fn get(&self, class: SizeClass) -> ClassLimit {
        match class {
            SizeClass::Ground => self.ground,
            SizeClass::Herb => self.herb,
            SizeClass::Shrub => self.shrub,
            SizeClass::Tree => self.tree,
            SizeClass::Vine => self.vine,
        }
    }

    pub fn get_mut(&mut self, class: SizeClass) -> &mut ClassLimit {
        match class {
            SizeClass::Ground => &mut self.ground,
            SizeClass::Herb => &mut self.herb,
            SizeClass::Shrub => &mut self.shrub,
            SizeClass::Tree => &mut self.tree,
            SizeClass::Vine => &mut self.vine,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Slots {
    pub trunk: String,
    pub branch: String,
    pub leaf: String,
    pub leaf_edge: String,
    pub stem: String,
    pub ground: String,
}

impl Default for Slots {
    fn default() -> Self {
        Slots {
            trunk: "mat-trunk".into(),
            branch: "mat-branch".into(),
            leaf: "mat-leaf".into(),
            leaf_edge: "mat-leafEdge".into(),
            stem: "mat-stem".into(),
            ground: "mat-ground".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Spawn {
    pub rate: f64,
    pub max_count: i32,
    pub min_spacing: i32,
}

impl Default for Spawn {
    fn default() -> Self {
        Spawn { rate: 0.08, max_count: 20, min_spacing: 3 }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Spread {
    pub rate: f64,
    pub radius_min: f64,
    pub radius_max: f64,
}

impl Default for Spread {
    fn default() -> Self {
        Spread { rate: 0.02, radius_min: 2.0, radius_max: 7.0 }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Growth {
    pub rate_min: f64,
    pub rate_max: f64,
    pub step_min: f64,
    pub step_max: f64,
    pub max_age: f64,
}

impl Default for Growth {
    fn default() -> Self {
        Growth { rate_min: 0.6, rate_max: 1.4, step_min: 2.0, step_max: 4.0, max_age: 900.0 }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Form {
    pub base_width: f64,
    pub taper: f64,
    pub min_width: f64,
    pub branch_chance: f64,
    pub branch_interval: f64,
    pub branch_angle_min: f64,
    pub branch_angle_max: f64,
    pub max_depth: i32,
    pub wander: f64,
    pub phototropism: f64,
    pub gravity: f64,
    pub leaf_depth: i32,
    pub leaf_size_min: f64,
    pub leaf_size_max: f64,
    pub leaf_density: f64,
    pub leaf_edges: bool,
    pub petiole: f64,
    pub wrap: bool,
    pub wrap_pitch: f64,
    pub wrap_amp: f64,
    pub climb_search: i32,
}

impl Default for Form {
    fn default() -> Self {
        Form {
            base_width: 4.0,
            taper: 0.9,
            min_width: 1.0,
            branch_chance: 0.7,
            branch_interval: 8.0,
            branch_angle_min: 16.0,
            branch_angle_max: 40.0,
            max_depth: 4,
            wander: 10.0,
            phototropism: 0.3,
            gravity: 0.06,
            leaf_depth: 2,
            leaf_size_min: 2.0,
            leaf_size_max: 4.0,
            leaf_density: 0.45,
            leaf_edges: true,
            petiole: 2.0,
            wrap: false,
            wrap_pitch: 0.22,
            wrap_amp: 26.0,
            climb_search: 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SpeciesLimits {
    pub max_radius_cells: i32,
    pub max_height_px: f64,
    pub max_tips: i32,
}

impl Default for SpeciesLimits {
    fn default() -> Self {
        SpeciesLimits { max_radius_cells: 2, max_height_px: 56.0, max_tips: 20 }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Shade {
    pub core_wood: f64,
    pub core_leaf: f64,
    pub tones: i32,
    pub jitter: f64,
    pub behind_shade: f64,
    pub adaptive_core: bool,
}

impl Default for Shade {
    fn default() -> Self {
        Shade { core_wood: 4.0, core_leaf: 2.5, tones: 5, jitter: 0.05, behind_shade: 0.18, adaptive_core: false }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Species {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub size_class: SizeClass,
    pub slots: Slots,
    pub spawn: Spawn,
    pub spread: Spread,
    pub growth: Growth,
    pub form: Form,
    pub limits: SpeciesLimits,
    pub shade: Shade,
}

impl Default for Species {
    fn default() -> Self {
        Species {
            id: "sp-new".into(),
            name: "New species".into(),
            enabled: true,
            size_class: SizeClass::Shrub,
            slots: Slots::default(),
            spawn: Spawn::default(),
            spread: Spread::default(),
            growth: Growth::default(),
            form: Form::default(),
            limits: SpeciesLimits::default(),
            shade: Shade::default(),
        }
    }
}

impl Species {
    pub fn new(id: &str, name: &str) -> Self {
        Species { id: id.into(), name: name.into(), ..Default::default() }
    }

    pub fn slot(&self, mat: crate::plant::Mat) -> &str {
        use crate::plant::Mat;
        match mat {
            Mat::Trunk => &self.slots.trunk,
            Mat::Branch => &self.slots.branch,
            Mat::Leaf => &self.slots.leaf,
            Mat::LeafEdge => &self.slots.leaf_edge,
            Mat::Stem => &self.slots.stem,
            Mat::Ground => &self.slots.ground,
            Mat::Empty => "",
        }
    }

    pub fn core_for(&self, wood: bool) -> f64 {
        if wood {
            self.shade.core_wood
        } else {
            self.shade.core_leaf
        }
    }
}

/// Species limits never exceed their size class ceiling.
#[derive(Clone, Copy, Debug)]
pub struct EffectiveLimits {
    pub max_radius_cells: i32,
    pub max_height_px: f64,
    pub max_tips: i32,
    pub min_spacing: i32,
    pub max_instances: i32,
}

pub fn effective_limits(species: &Species, class_limits: &ClassLimits) -> EffectiveLimits {
    let cl = class_limits.get(species.size_class);
    EffectiveLimits {
        max_radius_cells: species.limits.max_radius_cells.min(cl.max_radius_cells),
        max_height_px: species.limits.max_height_px.min(cl.max_height_px),
        max_tips: species.limits.max_tips,
        min_spacing: species.spawn.min_spacing.max(cl.min_spacing),
        max_instances: species.spawn.max_count.min(cl.max_instances),
    }
}

pub fn default_species_list() -> Vec<Species> {
    let mut moss = Species::new("sp-moss", "Moss mat");
    moss.size_class = SizeClass::Ground;
    moss.spawn = Spawn { rate: 0.5, max_count: 90, min_spacing: 1 };
    moss.spread = Spread { rate: 0.12, radius_min: 1.0, radius_max: 4.0 };
    moss.growth = Growth { rate_min: 0.5, rate_max: 1.1, step_min: 1.0, step_max: 2.0, max_age: 3000.0 };
    moss.limits = SpeciesLimits { max_radius_cells: 3, max_height_px: 8.0, max_tips: 1 };
    moss.shade = Shade { core_wood: 3.0, core_leaf: 3.0, tones: 4, jitter: 0.09, behind_shade: 0.15, ..Shade::default() };

    let mut grass = Species::new("sp-grass", "Grass tuft");
    grass.size_class = SizeClass::Herb;
    // Blades are drawn from the stem box, so a tuft reads green rather than as
    // a cluster of tiny brown twigs.
    grass.slots.trunk = "mat-stem".into();
    grass.slots.branch = "mat-stem".into();
    grass.slots.leaf = "mat-leaf".into();
    grass.spawn = Spawn { rate: 0.35, max_count: 70, min_spacing: 1 };
    grass.spread = Spread { rate: 0.06, radius_min: 1.0, radius_max: 5.0 };
    grass.growth = Growth { rate_min: 1.0, rate_max: 2.0, step_min: 2.0, step_max: 4.0, max_age: 700.0 };
    grass.form = Form {
        base_width: 2.0,
        taper: 0.86,
        branch_chance: 0.85,
        branch_interval: 3.0,
        branch_angle_min: 8.0,
        branch_angle_max: 34.0,
        max_depth: 1,
        wander: 6.0,
        phototropism: 0.5,
        gravity: 0.16,
        leaf_depth: 9,
        leaf_density: 0.05,
        leaf_size_min: 1.0,
        leaf_size_max: 2.0,
        petiole: 0.0,
        ..Form::default()
    };
    grass.limits = SpeciesLimits { max_radius_cells: 1, max_height_px: 24.0, max_tips: 10 };
    grass.shade = Shade { core_wood: 2.0, core_leaf: 2.0, tones: 4, jitter: 0.06, behind_shade: 0.15, ..Shade::default() };

    let mut fern = Species::new("sp-fern", "Fern bush");
    fern.size_class = SizeClass::Shrub;
    fern.spawn = Spawn { rate: 0.12, max_count: 30, min_spacing: 2 };
    fern.spread = Spread { rate: 0.03, radius_min: 2.0, radius_max: 6.0 };
    fern.growth = Growth { rate_min: 0.8, rate_max: 1.6, step_min: 2.0, step_max: 4.0, max_age: 1200.0 };
    fern.form = Form {
        base_width: 3.0,
        taper: 0.9,
        branch_chance: 0.8,
        branch_interval: 5.0,
        branch_angle_min: 22.0,
        branch_angle_max: 52.0,
        max_depth: 3,
        wander: 12.0,
        phototropism: 0.28,
        gravity: 0.14,
        leaf_depth: 2,
        leaf_density: 0.6,
        leaf_size_min: 2.0,
        leaf_size_max: 3.0,
        petiole: 1.0,
        ..Form::default()
    };
    fern.limits = SpeciesLimits { max_radius_cells: 2, max_height_px: 46.0, max_tips: 22 };

    let mut oak = Species::new("sp-oak", "Broadleaf tree");
    oak.size_class = SizeClass::Tree;
    oak.spawn = Spawn { rate: 0.05, max_count: 12, min_spacing: 5 };
    oak.spread = Spread { rate: 0.012, radius_min: 4.0, radius_max: 12.0 };
    oak.growth = Growth { rate_min: 0.7, rate_max: 1.3, step_min: 3.0, step_max: 6.0, max_age: 4000.0 };
    oak.form = Form {
        base_width: 7.0,
        taper: 0.93,
        min_width: 1.0,
        branch_chance: 0.75,
        branch_interval: 11.0,
        branch_angle_min: 18.0,
        branch_angle_max: 44.0,
        max_depth: 5,
        wander: 9.0,
        phototropism: 0.22,
        gravity: 0.05,
        leaf_depth: 3,
        leaf_density: 0.5,
        leaf_size_min: 3.0,
        leaf_size_max: 5.0,
        petiole: 2.0,
        ..Form::default()
    };
    oak.limits = SpeciesLimits { max_radius_cells: 4, max_height_px: 130.0, max_tips: 40 };
    oak.shade = Shade { core_wood: 5.0, core_leaf: 3.0, tones: 5, jitter: 0.05, behind_shade: 0.2, ..Shade::default() };

    let mut ivy = Species::new("sp-ivy", "Climbing ivy");
    ivy.size_class = SizeClass::Vine;
    ivy.spawn = Spawn { rate: 0.06, max_count: 12, min_spacing: 3 };
    ivy.spread = Spread { rate: 0.02, radius_min: 2.0, radius_max: 8.0 };
    ivy.growth = Growth { rate_min: 1.1, rate_max: 2.2, step_min: 2.0, step_max: 3.0, max_age: 2500.0 };
    ivy.form = Form {
        base_width: 2.0,
        taper: 0.98,
        branch_chance: 0.35,
        branch_interval: 14.0,
        branch_angle_min: 30.0,
        branch_angle_max: 70.0,
        max_depth: 3,
        wander: 14.0,
        phototropism: 0.12,
        gravity: 0.1,
        leaf_depth: 0,
        leaf_density: 0.35,
        leaf_size_min: 2.0,
        leaf_size_max: 3.0,
        petiole: 1.0,
        wrap: true,
        wrap_pitch: 0.24,
        wrap_amp: 30.0,
        climb_search: 4,
        ..Form::default()
    };
    ivy.limits = SpeciesLimits { max_radius_cells: 3, max_height_px: 120.0, max_tips: 18 };
    ivy.shade = Shade { core_wood: 2.0, core_leaf: 2.5, tones: 5, jitter: 0.06, behind_shade: 0.22, ..Shade::default() };

    vec![moss, grass, fern, oak, ivy]
}

// ---- the generated parameter form ----------------------------------------

/// One editable species parameter. The accessors are plain function pointers,
/// which is what lets the panel be generated from this table rather than
/// written out field by field.
pub enum FieldKind {
    Text {
        get: fn(&Species) -> String,
        set: fn(&mut Species, &str),
    },
    Bool {
        get: fn(&Species) -> bool,
        set: fn(&mut Species, bool),
    },
    SizeClassPick,
    SamplerPick {
        get: fn(&Species) -> String,
        set: fn(&mut Species, &str),
    },
    Num {
        get: fn(&Species) -> f64,
        set: fn(&mut Species, f64),
        min: f64,
        max: f64,
        step: f64,
    },
    Range {
        get: fn(&Species) -> (f64, f64),
        set: fn(&mut Species, f64, f64),
        min: f64,
        max: f64,
        step: f64,
    },
}

pub struct Field {
    pub label: &'static str,
    pub hint: Option<&'static str>,
    pub kind: FieldKind,
}

pub struct FieldGroup {
    pub group: &'static str,
    pub fields: &'static [Field],
}

macro_rules! num_field {
    ($label:expr, $path:ident $(. $rest:ident)*, $min:expr, $max:expr, $step:expr, $hint:expr) => {
        Field {
            label: $label,
            hint: $hint,
            kind: FieldKind::Num {
                get: |s| s.$path $(. $rest)* as f64,
                set: |s, v| s.$path $(. $rest)* = v as _,
                min: $min,
                max: $max,
                step: $step,
            },
        }
    };
}

macro_rules! range_field {
    ($label:expr, $lo:ident . $lo2:ident, $hi:ident . $hi2:ident, $min:expr, $max:expr, $step:expr, $hint:expr) => {
        Field {
            label: $label,
            hint: $hint,
            kind: FieldKind::Range {
                get: |s| (s.$lo.$lo2 as f64, s.$hi.$hi2 as f64),
                set: |s, lo, hi| {
                    s.$lo.$lo2 = lo as _;
                    s.$hi.$hi2 = hi as _;
                },
                min: $min,
                max: $max,
                step: $step,
            },
        }
    };
}

pub static SPECIES_SCHEMA: &[FieldGroup] = &[
    FieldGroup {
        group: "Identity",
        fields: &[
            Field {
                label: "Name",
                hint: None,
                kind: FieldKind::Text { get: |s| s.name.clone(), set: |s, v| s.name = v.to_string() },
            },
            Field { label: "Size class", hint: None, kind: FieldKind::SizeClassPick },
            Field {
                label: "Enabled",
                hint: None,
                kind: FieldKind::Bool { get: |s| s.enabled, set: |s, v| s.enabled = v },
            },
        ],
    },
    FieldGroup {
        group: "Materials",
        fields: &[
            Field {
                label: "Trunk",
                hint: None,
                kind: FieldKind::SamplerPick { get: |s| s.slots.trunk.clone(), set: |s, v| s.slots.trunk = v.into() },
            },
            Field {
                label: "Branch",
                hint: None,
                kind: FieldKind::SamplerPick { get: |s| s.slots.branch.clone(), set: |s, v| s.slots.branch = v.into() },
            },
            Field {
                label: "Leaf",
                hint: None,
                kind: FieldKind::SamplerPick { get: |s| s.slots.leaf.clone(), set: |s, v| s.slots.leaf = v.into() },
            },
            Field {
                label: "Leaf edge",
                hint: None,
                kind: FieldKind::SamplerPick { get: |s| s.slots.leaf_edge.clone(), set: |s, v| s.slots.leaf_edge = v.into() },
            },
            Field {
                label: "Stem to leaf",
                hint: None,
                kind: FieldKind::SamplerPick { get: |s| s.slots.stem.clone(), set: |s, v| s.slots.stem = v.into() },
            },
            Field {
                label: "Ground",
                hint: None,
                kind: FieldKind::SamplerPick { get: |s| s.slots.ground.clone(), set: |s, v| s.slots.ground = v.into() },
            },
        ],
    },
    FieldGroup {
        group: "Spawn and spread",
        fields: &[
            num_field!("Spawn rate", spawn.rate, 0.0, 4.0, 0.01, Some("attempts per simulation second")),
            num_field!("Max instances", spawn.max_count, 0.0, 400.0, 1.0, None),
            num_field!("Min spacing (cells)", spawn.min_spacing, 0.0, 20.0, 1.0, None),
            num_field!("Spread rate", spread.rate, 0.0, 2.0, 0.005, Some("offspring per parent per second")),
            range_field!("Spread distance (cells)", spread.radius_min, spread.radius_max, 0.0, 40.0, 1.0, None),
        ],
    },
    FieldGroup {
        group: "Growth",
        fields: &[
            range_field!("Growth rate", growth.rate_min, growth.rate_max, 0.05, 6.0, 0.05, Some("segments per simulation second")),
            range_field!("Segment length (px)", growth.step_min, growth.step_max, 1.0, 14.0, 0.5, None),
            num_field!("Max age", growth.max_age, 10.0, 10000.0, 10.0, None),
        ],
    },
    FieldGroup {
        group: "Form and branching",
        fields: &[
            num_field!("Base width (px)", form.base_width, 1.0, 24.0, 0.5, None),
            num_field!("Taper per segment", form.taper, 0.5, 1.0, 0.005, None),
            num_field!("Min width (px)", form.min_width, 0.5, 6.0, 0.25, None),
            num_field!("Branch chance", form.branch_chance, 0.0, 1.0, 0.01, None),
            num_field!("Branch interval (px)", form.branch_interval, 1.0, 40.0, 0.5, None),
            range_field!("Branch angle (deg)", form.branch_angle_min, form.branch_angle_max, 0.0, 120.0, 1.0, None),
            num_field!("Max branch depth", form.max_depth, 0.0, 9.0, 1.0, None),
            num_field!("Wander (deg)", form.wander, 0.0, 60.0, 0.5, None),
            num_field!("Phototropism", form.phototropism, 0.0, 1.0, 0.01, Some("pull of tips back toward vertical")),
            num_field!("Droop", form.gravity, 0.0, 1.0, 0.01, None),
        ],
    },
    FieldGroup {
        group: "Leaves",
        fields: &[
            num_field!("First leaf depth", form.leaf_depth, 0.0, 9.0, 1.0, None),
            num_field!("Leaf density", form.leaf_density, 0.0, 1.0, 0.01, None),
            range_field!("Leaf size (px)", form.leaf_size_min, form.leaf_size_max, 1.0, 12.0, 0.5, None),
            num_field!("Stem to leaf (px)", form.petiole, 0.0, 10.0, 0.5, None),
            Field {
                label: "Draw leaf edges",
                hint: None,
                kind: FieldKind::Bool { get: |s| s.form.leaf_edges, set: |s, v| s.form.leaf_edges = v },
            },
        ],
    },
    FieldGroup {
        group: "Climbing and wrapping",
        fields: &[
            Field {
                label: "Wrap around supports",
                hint: None,
                kind: FieldKind::Bool { get: |s| s.form.wrap, set: |s, v| s.form.wrap = v },
            },
            num_field!("Support search (cells)", form.climb_search, 0.0, 12.0, 1.0, None),
            num_field!("Wrap pitch", form.wrap_pitch, 0.02, 1.2, 0.01, None),
            num_field!("Wrap sway (deg)", form.wrap_amp, 0.0, 90.0, 1.0, None),
        ],
    },
    FieldGroup {
        group: "Limits",
        fields: &[
            num_field!("Footprint radius (cells)", limits.max_radius_cells, 0.0, 20.0, 1.0, Some("clamped by the size class ceiling")),
            num_field!("Max height (px)", limits.max_height_px, 4.0, 400.0, 2.0, None),
            num_field!("Max active tips", limits.max_tips, 1.0, 120.0, 1.0, None),
        ],
    },
    FieldGroup {
        group: "Shading",
        fields: &[
            num_field!("Tone steps", shade.tones, 2.0, 16.0, 1.0, None),
            num_field!("Wood core depth (px)", shade.core_wood, 0.5, 16.0, 0.5, None),
            num_field!("Leaf core depth (px)", shade.core_leaf, 0.5, 16.0, 0.5, None),
            Field {
                label: "Adaptive core depth",
                hint: Some("off keeps thin parts light, on lets every shape use the full ramp"),
                kind: FieldKind::Bool { get: |s| s.shade.adaptive_core, set: |s, v| s.shade.adaptive_core = v },
            },
            num_field!("Tone jitter", shade.jitter, 0.0, 0.4, 0.005, None),
            num_field!("Behind-support darkening", shade.behind_shade, 0.0, 0.6, 0.01, None),
        ],
    },
];
