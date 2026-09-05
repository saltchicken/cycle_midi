#[derive(Debug, Clone, PartialEq)]
pub enum Pitch {
    Absolute(u8),
    Numeric(i32),
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
