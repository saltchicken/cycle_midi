use super::types::{ArpStyle, DynamicValue, ExtractType, Pitch, QuantizeMode, ScaleDef, SeedDef, ScaleSequence, MidiImportOptions};
use crate::engine::render::math::lcm;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct MacroDef {
    pub params: Vec<String>,
    pub body: Node,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Modifier {
    Euclidean(u8, u8),
    Span(usize),
    Arp(ArpStyle),
    Ratchet(u8),
    Stut(u8, f32, f32),
    Humanize(u8, f64),
    Probability(u8),
    Invert(i32),
    Drop(u8),
    Transpose(i32),
    Strum(f64),
    ExtractPitch(ExtractType, Option<i32>, i32),
    Chordify(Option<i32>, i32),
    VelocityOverride(u8),
    GateOverride(u8),
    PhaseShift(f32),
}

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
    Ref(String, Vec<Node>),
    Sequence(Vec<Node>),
    ShuffledSequence(Vec<Node>),
    Parallel(Vec<Vec<Node>>),
    Polymeter(Vec<Vec<Node>>),
    Arrange(Vec<(usize, usize, Box<Node>)>),
    Alternator(Vec<Node>),
    RandomChoice(Vec<(u32, Node)>),
    WithScale(ScaleDef, Box<Node>),
    Struct(Box<Node>, Box<Node>),
    Modified(Box<Node>, Vec<Modifier>),
    MidiImport(MidiImportOptions),
}

impl Node {
    pub fn expand_refs(&mut self, env: &HashMap<String, MacroDef>, depth: usize) -> Result<(), String> {
        if depth > 32 {
            return Err("Max macro expansion depth exceeded (circular reference?)".to_string());
        }
        match self {
            Node::Ref(name, args) => {
                for arg in args.iter_mut() {
                    arg.expand_refs(env, depth)?;
                }

                if let Some(macro_def) = env.get(name) {
                    if args.len() != macro_def.params.len() {
                        return Err(format!("Macro '{}' expects {} args, got {}", name, macro_def.params.len(), args.len()));
                    }

                    let mut local_env = env.clone();
                    for (param_name, arg_val) in macro_def.params.iter().zip(args.iter()) {
                        local_env.insert(
                            param_name.clone(), 
                            MacroDef { params: vec![], body: arg_val.clone() }
                        );
                    }

                    let mut cloned = macro_def.body.clone();
                    cloned.expand_refs(&local_env, depth + 1)?;
                    *self = cloned;
                } else {
                    return Err(format!("Unresolved alias: '{}'", name));
                }
            }
            Node::Chord(elements)
            | Node::Sequence(elements)
            | Node::ShuffledSequence(elements)
            | Node::Alternator(elements) => {
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
            Node::Arrange(segments) => {
                for (_, _, child) in segments {
                    child.expand_refs(env, depth)?;
                }
            }
            Node::WithScale(_, child) => {
                child.expand_refs(env, depth)?;
            }
            Node::Struct(structure, content) => {
                structure.expand_refs(env, depth)?;
                content.expand_refs(env, depth)?;
            }
            Node::Modified(child, _) => {
                child.expand_refs(env, depth)?;
            }
            Node::MidiImport(_) => {
                // Resolved out during the IO parsing phase, ignored here
            }
            _ => {}
        }
        Ok(())
    }

    pub fn cycle_length(&self) -> usize {
        match self {
            Node::Note { .. } | Node::CC { .. } | Node::Rest | Node::Hold | Node::Ref(_, _) | Node::MidiImport(_) => 1,
            Node::Chord(elements) | Node::Sequence(elements) | Node::ShuffledSequence(elements) => {
                elements.iter().fold(1, |acc, n| lcm(acc, n.cycle_length()))
            }
            Node::RandomChoice(elements) => elements
                .iter()
                .fold(1, |acc, (_, n)| lcm(acc, n.cycle_length())),
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
                let l0 = layers[0].len().max(1);
                layers.iter().fold(1, |acc, layer| {
                    let li = layer.len().max(1);
                    let layer_child_lcm = layer.iter().fold(1, |a, n| lcm(a, n.cycle_length()));

                    let sync_macro_cycles = lcm(l0, li * layer_child_lcm) / l0;
                    lcm(acc, sync_macro_cycles)
                })
            }
            Node::Arrange(segments) => segments.iter().map(|s| s.1).max().unwrap_or(1).max(1),
            Node::WithScale(_, child) => child.cycle_length(),
            Node::Struct(structure, content) => {
                lcm(structure.cycle_length(), content.cycle_length())
            }
            Node::Modified(child, mods) => {
                let mut len = child.cycle_length();
                for m in mods {
                    if let Modifier::Span(span) = m {
                        len = (*span).max(1);
                    }
                }
                len
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
    pub aliases: HashMap<String, MacroDef>,
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
