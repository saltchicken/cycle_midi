use super::directives::scale_def;
use super::primitives::{int_i32, kw, padding};
use crate::ast::{Node, ScaleDef, SeedDef, SeedInterval, Track};
use chumsky::prelude::*;

#[derive(Clone)]
enum TrackModifier {
    Span(usize),
    Scale(ScaleDef),
    Seed(SeedDef),
    Octave(i32),
    ProgramChange(u8),
}

fn track_modifier() -> impl Parser<char, TrackModifier, Error = Simple<char>> + Clone {
    choice((
        kw("span")
            .ignore_then(text::int::<char, Simple<char>>(10).try_map(|s, span| {
                s.parse::<usize>()
                    .map_err(|e| Simple::custom(span, format!("Invalid span: {}", e)))
            }))
            .map(TrackModifier::Span),
        kw("scale")
            .ignore_then(scale_def())
            .map(TrackModifier::Scale),
        kw("pc")
            .ignore_then(text::int::<char, Simple<char>>(10).try_map(|s, span| {
                s.parse::<u8>()
                    .map(|v| v.saturating_sub(1))
                    .map_err(|e| Simple::custom(span, format!("Invalid PC: {}", e)))
            }))
            .map(TrackModifier::ProgramChange),
        kw("octave")
            .ignore_then(int_i32().or_not())
            .map(|v| TrackModifier::Octave(v.unwrap_or(1))),
        kw("seed")
            .ignore_then(text::int::<char, Simple<char>>(10).try_map(|s, span| {
                s.parse::<u64>()
                    .map_err(|e| Simple::custom(span, format!("Invalid seed: {}", e)))
            }))
            .then(
                choice((
                    kw("m_every").to(0u8),
                    kw("t_every").to(1u8),
                    kw("every").to(2u8),
                ))
                .then(text::int::<char, Simple<char>>(10).try_map(|s, span| {
                    s.parse::<usize>()
                        .map_err(|e| Simple::custom(span, format!("Invalid interval: {}", e)))
                }))
                .or_not(),
            )
            .map(|(base, interval_data)| {
                let interval = interval_data.map(|(interval_type, val)| match interval_type {
                    0 => SeedInterval::Macro(val),
                    1 => SeedInterval::Track(val),
                    _ => SeedInterval::Micro(val),
                });
                TrackModifier::Seed(SeedDef { base, interval })
            }),
    ))
}

pub fn track_parser<'a>(
    expr: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Track, Error = Simple<char>> + Clone + 'a {
    just('!')
        .or_not()
        .map(|m| m.is_some())
        .then_ignore(just('T'))
        .then(text::int(10).try_map(|s: String, span| {
            s.parse::<u8>()
                .map_err(|e| Simple::custom(span, format!("Invalid channel: {}", e)))
        }))
        .then(track_modifier().repeated())
        .then_ignore(just(':'))
        .padded_by(padding())
        .then(expr.padded_by(padding()).repeated().map(Node::Sequence))
        .map(|(((is_muted, ch), modifiers), mut root_node)| {
            let mut track_scale = None;
            let mut track_span = None;
            let mut track_seed = None;
            let mut track_octave = 0;
            let mut track_pc = None;

            for m in modifiers {
                match m {
                    TrackModifier::Span(s) => track_span = Some(s),
                    TrackModifier::Scale(s) => track_scale = Some(s),
                    TrackModifier::Seed(s) => track_seed = Some(s),
                    TrackModifier::Octave(o) => track_octave += o,
                    TrackModifier::ProgramChange(pc) => track_pc = Some(pc),
                }
            }

            if let Some(s) = track_span {
                root_node = Node::Span(Box::new(root_node), s);
            }

            Track {
                channel: ch.saturating_sub(1).min(15),
                is_muted,
                scale: track_scale,
                seed: track_seed,
                octave_offset: track_octave,
                program_change: track_pc,
                root_node,
            }
        })
        .padded_by(padding())
}
