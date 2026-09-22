use super::primitives::{float_f64, int_i32, int_u8, kw, pad_char};
use crate::ast::{DynamicValue, Node, Pitch};
use chumsky::prelude::*;

pub fn dynamic_value() -> impl Parser<char, DynamicValue, Error = Simple<char>> + Clone {
    let lfo_args = pad_char('(')
        .ignore_then(int_u8()) // min
        .then_ignore(pad_char(','))
        .then(int_u8()) // max
        .then(
            pad_char(',')
                .ignore_then(float_f64()) // speed
                .or_not(),
        )
        .then_ignore(pad_char(')'))
        .or_not();

    choice((
        kw("sine").ignore_then(lfo_args.clone()).map(|args| {
            let ((min, max), spd) = args.unwrap_or(((0, 127), None));
            DynamicValue::Sine(min, max, spd.unwrap_or(1.0))
        }),
        kw("saw").ignore_then(lfo_args.clone()).map(|args| {
            let ((min, max), spd) = args.unwrap_or(((0, 127), None));
            DynamicValue::Saw(min, max, spd.unwrap_or(1.0))
        }),
        kw("tri").ignore_then(lfo_args.clone()).map(|args| {
            let ((min, max), spd) = args.unwrap_or(((0, 127), None));
            DynamicValue::Tri(min, max, spd.unwrap_or(1.0))
        }),
        int_u8().map(DynamicValue::Static),
    ))
}

pub fn cc_parser() -> impl Parser<char, Node, Error = Simple<char>> + Clone {
    just("cc")
        .or(just("CC"))
        .ignore_then(int_u8())
        .then(just('@').ignore_then(dynamic_value()).or_not())
        .map(|(controller, v)| Node::CC {
            controller,
            value: v.unwrap_or(DynamicValue::Static(127)),
        })
}

pub fn diatonic_chord_type() -> impl Parser<char, Vec<i32>, Error = Simple<char>> + Clone {
    choice((
        just("triad").or(just("t")).to(vec![0, 2, 4]),
        just("7th").or(just("7")).to(vec![0, 2, 4, 6]),
        just("9th").or(just("9")).to(vec![0, 2, 4, 6, 8]),
        just("sus2").to(vec![0, 1, 4]),
        just("sus4").to(vec![0, 3, 4]),
        just("m").to(vec![0, 3, 7]),
        just("maj").to(vec![0, 4, 7]),
    ))
}

pub fn chord_or_note() -> impl Parser<char, Node, Error = Simple<char>> + Clone {
    let accidental = choice((just('#').to(1), just('b').to(-1)))
        .repeated()
        .map(|accs| accs.into_iter().sum::<i32>());

    let numeric_pitch = int_i32()
        .then(accidental.clone())
        .map(|(degree, acc)| Pitch::Numeric(degree, acc));

    let single_pitch = numeric_pitch.clone().map(|p| vec![p]);

    let numeric_named_chord = int_i32()
        .then(accidental.clone())
        .then_ignore(just('\'')) 
        .then(diatonic_chord_type())
        .map(|((root_degree, acc), intervals)| {
            intervals
                .into_iter()
                .map(|interval| Pitch::Numeric(root_degree + interval, acc))
                .collect::<Vec<_>>()
        });

    choice((numeric_named_chord, single_pitch)).map(|pitches| {
        if pitches.len() == 1 {
            Node::Note {
                pitch: pitches[0].clone(),
                velocity: 100,
                gate: 100,
            }
        } else {
            let notes = pitches.into_iter().map(|p| Node::Note {
                pitch: p,
                velocity: 100,
                gate: 100,
            }).collect();
            Node::Chord(notes)
        }
    })
}
