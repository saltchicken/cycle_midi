use super::directives::scale_def;
use super::primitives::{int_i32, int_u64, int_u8, int_usize, kw, pad_char, padding};
use crate::ast::{Modifier, Node, ScaleDef, SeedDef, SeedInterval, Track};
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
            .ignore_then(pad_char(':'))
            .ignore_then(int_usize())
            .map(TrackModifier::Span),
        kw("scale")
            .ignore_then(pad_char(':'))
            .ignore_then(scale_def())
            .map(TrackModifier::Scale),
        kw("pc")
            .ignore_then(pad_char(':'))
            .ignore_then(int_u8().map(|v| v.saturating_sub(1)))
            .map(TrackModifier::ProgramChange),
        kw("octave")
            .ignore_then(pad_char(':'))
            .ignore_then(int_i32())
            .map(TrackModifier::Octave),
        kw("seed")
            .ignore_then(pad_char(':'))
            .ignore_then(int_u64())
            .then(
                choice((
                    kw("m_every").to(0u8),
                    kw("t_every").to(1u8),
                    kw("every").to(2u8),
                ))
                .then(int_usize())
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
        .then(int_u8())
        .then(
            track_modifier()
                .padded_by(padding())
                .separated_by(pad_char(','))
                .delimited_by(pad_char('('), pad_char(')'))
                .or_not()
                .map(|modifiers| modifiers.unwrap_or_default()),
        )
        .then_ignore(pad_char(':'))
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
                root_node = Node::Modified(Box::new(root_node), vec![Modifier::Span(s)]);
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
