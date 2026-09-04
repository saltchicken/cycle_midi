use super::types::{ArpStyle, DynamicValue, Pitch, QuantizeMode, ScaleDef, SeedDef};
use crate::engine::render::math::lcm;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Note {
        pitch: Pitch,
        velocity: u8,
        gate: u8,
    },
    CC {
        controller: u8,
        value: DynamicValue,
    },
    Chord(Vec<Node>),
    Rest,
    Hold,
    Sequence(Vec<Node>),
    Parallel(Vec<Vec<Node>>),
    Polymeter(Vec<Vec<Node>>),
    Euclidean(Box<Node>, u8, u8),
    Alternator(Vec<Node>),
    RandomChoice(Vec<Node>),
    SpeedModifier(Box<Node>, f32),
    Arp(Box<Node>, ArpStyle),
    Probability(Box<Node>, u8),
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
    PhaseShift(Box<Node>, f32),
}

impl Node {
    pub fn cycle_length(&self) -> usize {
        match self {
            Node::Note { .. } | Node::CC { .. } | Node::Rest | Node::Hold => 1,
            Node::Chord(elements) | Node::Sequence(elements) | Node::RandomChoice(elements) => {
                elements.iter().fold(1, |acc, n| lcm(acc, n.cycle_length()))
            }
            Node::Alternator(elements) => {
                let children_lcm = elements.iter().fold(1, |acc, n| lcm(acc, n.cycle_length()));
                children_lcm * elements.len()
            }
            Node::Parallel(layers) => layers.iter().fold(1, |acc, l| {
                let layer_len = l.iter().fold(1, |a, n| lcm(a, n.cycle_length()));
                lcm(acc, layer_len)
            }),
            Node::Polymeter(layers) => {
                if layers.is_empty() {
                    return 1;
                }
                // Base pulse is derived from the first layer
                let l0 = layers[0].len().max(1);
                layers.iter().fold(1, |acc, layer| {
                    let li = layer.len().max(1);
                    let layer_child_lcm = layer.iter().fold(1, |a, n| lcm(a, n.cycle_length()));
                    
                    // How many macro-cycles (of length L0) it takes for this layer to perfectly sync back to beat 1
                    let sync_macro_cycles = lcm(l0, li * layer_child_lcm) / l0;
                    lcm(acc, sync_macro_cycles)
                })
            }
            Node::Euclidean(child, _, _) | Node::Arp(child, _) | Node::Probability(child, _) | Node::PhaseShift(child, _) => {
                child.cycle_length()
            }
            Node::Condition {
                interval,
                true_branch,
                false_branch,
                ..
            } => {
                let branches_lcm = lcm(true_branch.cycle_length(), false_branch.cycle_length());
                lcm(*interval, branches_lcm)
            }
            Node::MacroCondition {
                true_branch,
                false_branch,
                ..
            } => lcm(true_branch.cycle_length(), false_branch.cycle_length()),
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
        self.tracks
            .iter()
            .fold(1, |acc, track| lcm(acc, track.root_node.cycle_length()))
    }
}
