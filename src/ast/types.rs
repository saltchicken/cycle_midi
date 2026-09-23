#[derive(Debug, Clone, PartialEq)]
pub enum Pitch {
    Numeric(i32, i32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExtractType {
    Highest,
    Lowest,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ArpStyle {
    Up,
    Down,
    UpDown,
    DownUp,
    Converge,
    Diverge,
    PinkyUp,
    PinkyUpDown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QuantizeMode {
    Fixed(usize),
    Auto,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DynamicValue {
    Static(u8),
    Sine(u8, u8, f64),
    Saw(u8, u8, f64),
    Tri(u8, u8, f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScaleDef {
    pub root_pitch: u8,
    pub intervals: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleSequence {
    Explicit(Vec<(usize, usize, ScaleDef)>),
    Algorithmic {
        base_scale: ScaleDef,
        shift_semitones: i32,
        macro_cycles_per_step: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum SeedInterval {
    Micro(usize),
    Macro(usize),
    Track(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeedDef {
    pub base: u64,
    pub interval: Option<SeedInterval>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MidiImportOptions {
    pub path: String,
    pub target_scale: String,
    pub format: String,
    pub grid: usize,
    pub beats: f64,
    pub subdivide: bool,
    pub preserve_velocity: bool,
    pub snap_to_grid: bool,
    pub chords: bool,
    pub debug: bool,
}

impl Default for MidiImportOptions {
    fn default() -> Self {
        Self {
            path: String::new(),
            target_scale: "C4 major".to_string(),
            format: "chain".to_string(),
            grid: 8,
            beats: 2.0,
            subdivide: false,
            preserve_velocity: false,
            snap_to_grid: false,
            chords: false,
            debug: false,
        }
    }
}
