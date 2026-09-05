use super::types::{ArpStyle, DynamicValue, Pitch, QuantizeMode, ScaleDef, SeedDef};
use crate::engine::render::math::lcm;
use std::collections::HashMap;

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
    Ref(String),
    Sequence(Vec<Node>),
    Parallel(Vec<Vec<Node>>),
    Polymeter(Vec<Vec<Node>>),
    SeqP(Vec<(usize, usize, Box<Node>)>, bool),
    Euclidean(Box<Node>, u8, u8),
    Alternator(Vec<Node>),
    RandomChoice(Vec<(u32, Node)>),
    SpeedModifier(Box<Node>, f32),
    Arp(Box<Node>, ArpStyle),
    Ratchet(Box<Node>, u8), 
    Humanize(Box<Node>, u8, f64), // Unified humanize node
    Probability(Box<Node>, u8),
    Invert(Box<Node>, i32),
    Drop(Box<Node>, u8),
    Transpose(Box<Node>, i32),
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
    pub fn expand_refs(&mut self, env: &HashMap<String, Node>, depth: usize) -> Result<(), String> {
        if depth > 32 {
            return Err("Max macro expansion depth exceeded (circular reference?)".to_string());
        }
        match self {
            Node::Ref(name) => {
                if let Some(resolved) = env.get(name) {
                    let mut cloned = resolved.clone();
                    cloned.expand_refs(env, depth + 1)?;
                    *self = cloned;
                } else {
                    return Err(format!("Unresolved alias: ${}", name));
                }
            }
            Node::Chord(elements) | Node::Sequence(elements) | Node::Alternator(elements) => {
                for el in elements {
                    el.expand_refs(env, depth)?;
                }
            }
            Node::RandomChoice(elements) => {
                for (_, el) in elements {
                    el.expand_refs(env, depth)?;
                }
            }
            Node::Parallel(layers) | Node::Polymeter(layers) => {
                for layer in layers {
                    for el in layer {
                        el.expand_refs(env, depth)?;
                    }
                }
            }
            Node::SeqP(segments, _) => {
                for (_, _, child) in segments {
                    child.expand_refs(env, depth)?;
                }
            }
            Node::Euclidean(child, _, _) | Node::Arp(child, _) | Node::Probability(child, _) | Node::PhaseShift(child, _) | Node::SpeedModifier(child, _) | Node::Ratchet(child, _) | Node::Humanize(child, _, _) | Node::Invert(child, _) | Node::Drop(child, _) | Node::Transpose(child, _) => {
                child.expand_refs(env, depth)?;
            }
            Node::Condition { true_branch, false_branch, .. } | Node::MacroCondition { true_branch, false_branch, .. } => {
                true_branch.expand_refs(env, depth)?;
                false_branch.expand_refs(env, depth)?;
            }
            _ => {}
        }
        Ok(())
    }

    pub fn cycle_length(&self) -> usize {
        match self {
            Node::Note { .. } | Node::CC { .. } | Node::Rest | Node::Hold | Node::Ref(_) => 1,
            Node::Chord(elements) | Node::Sequence(elements) => {
                elements.iter().fold(1, |acc, n| lcm(acc, n.cycle_length()))
            }
            Node::RandomChoice(elements) => {
                elements.iter().fold(1, |acc, (_, n)| lcm(acc, n.cycle_length()))
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
            Node::SeqP(segments, _) => {
                segments.iter().map(|s| s.1).max().unwrap_or(1).max(1)
            }
            Node::Euclidean(child, _, _) | Node::Arp(child, _) | Node::Probability(child, _) | Node::PhaseShift(child, _) | Node::Ratchet(child, _) | Node::Humanize(child, _, _) | Node::Invert(child, _) | Node::Drop(child, _) | Node::Transpose(child, _) => {
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
