#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Note { pitch: u8, velocity: u8, gate: u8, prob: u8 },
    Chord(Vec<Node>),
    Rest,
    Hold,
    Sequence(Vec<Node>),
    Parallel(Vec<Vec<Node>>),
    Euclidean(Box<Node>, u8, u8),
    Alternator(Vec<Node>),
    SpeedModifier(Box<Node>, f32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub channel: u8,
    pub is_muted: bool,
    pub root_node: Node,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub bpm: Option<f64>,
    pub global_silence: bool,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone)]
pub struct ScheduledNote {
    pub channel: u8,
    pub pitch: u8,
    pub velocity: u8,
    pub start_ms: f64,
    pub duration_ms: f64,
}

#[derive(Debug, Clone)]
pub struct RenderContext {
    pub channel: u8,
    pub start_ms: f64,
    pub duration_ms: f64,
    pub cycle_count: usize,
}
