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

pub fn lcm(a: usize, b: usize) -> usize {
    if a == 0 || b == 0 { return 0; }
    let mut x = a;
    let mut y = b;
    while y != 0 {
        let t = y;
        y = x % y;
        x = t;
    }
    let gcd = x;
    (a * b) / gcd
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Note { pitch: Pitch, velocity: u8, gate: u8, prob: u8 },
    Chord(Vec<Node>),
    Rest,
    Hold,
    Sequence(Vec<Node>),
    Parallel(Vec<Vec<Node>>),
    Euclidean(Box<Node>, u8, u8),
    Alternator(Vec<Node>),
    RandomChoice(Vec<Node>), // <-- Added RandomChoice
    SpeedModifier(Box<Node>, f32),
    Arp(Box<Node>, ArpStyle),
    Condition {
        interval: usize,
        offset: usize,
        true_branch: Box<Node>,
        false_branch: Box<Node>,
    },
    MacroCondition {
        interval: usize,
        offset: usize,
        is_gate: bool, 
        true_branch: Box<Node>,
        false_branch: Box<Node>,
    },
}

impl Node {
    pub fn cycle_length(&self) -> usize {
        match self {
            Node::Note { .. } | Node::Rest | Node::Hold => 1,
            Node::Chord(elements) | Node::Sequence(elements) | Node::RandomChoice(elements) => {
                elements.iter().fold(1, |acc, n| lcm(acc, n.cycle_length()))
            }
            Node::Alternator(elements) => {
                let children_lcm = elements.iter().fold(1, |acc, n| lcm(acc, n.cycle_length()));
                children_lcm * elements.len() 
            }
            Node::Parallel(layers) => {
                layers.iter().fold(1, |acc, l| {
                    let layer_len = l.iter().fold(1, |a, n| lcm(a, n.cycle_length()));
                    lcm(acc, layer_len)
                })
            }
            Node::Euclidean(child, _, _) | Node::Arp(child, _) => child.cycle_length(),
            Node::Condition { interval, true_branch, false_branch, .. } => {
                let branches_lcm = lcm(true_branch.cycle_length(), false_branch.cycle_length());
                lcm(*interval, branches_lcm)
            }
            Node::MacroCondition { true_branch, false_branch, .. } => {
                lcm(true_branch.cycle_length(), false_branch.cycle_length())
            }
            Node::SpeedModifier(child, speed) => {
                let child_len = child.cycle_length();
                
                let mut num = speed.round() as usize;
                let mut den = 1;
                
                for d in 1..=128 {
                    let n = *speed * (d as f32);
                    if (n - n.round()).abs() < 0.005 {
                        num = n.round() as usize;
                        den = d;
                        break;
                    }
                }
                
                if num == 0 {
                    return child_len;
                }
                
                lcm(num, child_len * den) / num
            }
        }
    }
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeedDef {
    pub base: u64,
    pub interval: Option<SeedInterval>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub channel: u8,
    pub is_muted: bool,
    pub scale: Option<ScaleDef>,
    pub seed: Option<SeedDef>,
    pub octave_offset: i32,
    pub root_node: Node,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub bpm: Option<f64>,
    pub quantize: Option<QuantizeMode>,
    pub scale: Option<ScaleDef>,
    pub global_silence: bool,
    pub tracks: Vec<Track>,
}

impl Program {
    pub fn pattern_length_cycles(&self) -> usize {
        self.tracks.iter().fold(1, |acc, track| lcm(acc, track.root_node.cycle_length()))
    }
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
    pub window_start_ms: f64,
    pub window_end_ms: f64,
    pub cycle_count: usize,
    pub macro_cycle_length: usize,
    pub scale: Option<ScaleDef>,
    pub active_chord_indices: Vec<usize>,
    pub octave_offset: i32,
}
