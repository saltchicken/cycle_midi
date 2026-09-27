use super::types::{ArpStyle, DynamicValue, MidiImportOptions, Pitch, QuantizeMode, ScaleDef, ScaleSequence, SeedDef};
use crate::engine::render::math::lcm;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Modifier {
    Velocity(u8),
    Gate(u8),
    Humanize(u8, f64),
    Probability(u8),
    Transpose(i32),
    Arp(ArpStyle),
    Span(usize),
    Speed(f64),
    Stut(u8, f32, f32),
    Euclidean(u8, u8),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Note { pitch: Pitch, velocity: u8, gate: u8 },
    CC { controller: u8, value: DynamicValue },
    Chord(Vec<Node>),
    Rest,
    Hold,
    Ref(String),
    Sequence(Vec<Node>), // Subdivides time (e.g. inside `[ ]`)
    Macro(Vec<Node>),    // Concatenates time (e.g. top level)
    Parallel(Vec<Node>), // Plays multiple nodes concurrently
    Repeat(Box<Node>, usize),
    Modified(Box<Node>, Vec<Modifier>),
    MidiImport(MidiImportOptions),
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
                    return Err(format!("Unresolved alias: '{}'", name));
                }
            }
            Node::Chord(seq) | Node::Sequence(seq) | Node::Macro(seq) | Node::Parallel(seq) => {
                for el in seq {
                    el.expand_refs(env, depth)?;
                }
            }
            Node::Repeat(child, _) | Node::Modified(child, _) => {
                child.expand_refs(env, depth)?;
            }
            _ => {}
        }
        Ok(())
    }

    pub fn cycle_length(&self) -> usize {
        match self {
            Node::Macro(elements) => elements.iter().map(|n| n.cycle_length()).sum::<usize>().max(1),
            Node::Repeat(child, count) => child.cycle_length() * count.max(&1),
            Node::Parallel(layers) => layers.iter().map(|n| n.cycle_length()).max().unwrap_or(1).max(1),
            _ => 1, // Sequences, notes, chords all evaluate to 1 cycle length by default
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
    pub program_change: Option<u8>,
    pub root_node: Node,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub bpm: Option<f64>,
    pub signature: Option<(u8, u8)>,
    pub quantize: Option<QuantizeMode>,
    pub scale: Option<ScaleDef>,
    pub scale_seq: Option<ScaleSequence>,
    pub global_silence: bool,
    pub includes: Vec<String>,
    pub aliases: HashMap<String, Node>,
    pub tracks: Vec<Track>,
}

impl Program {
    pub fn expand_all_refs(&mut self) -> Result<(), String> {
        let env = self.aliases.clone();
        for track in &mut self.tracks {
            track.root_node.expand_refs(&env, 0)?;
        }
        Ok(())
    }

    pub fn pattern_length_cycles(&self) -> usize {
        self.tracks
            .iter()
            .fold(1, |acc, track| lcm(acc, track.root_node.cycle_length()))
    }
}
