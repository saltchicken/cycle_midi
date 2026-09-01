use chumsky::prelude::*;
use crate::ast::{Node, Program, Track};

pub fn mmn_parser() -> impl Parser<char, Program, Error = Simple<char>> {
    let expr = recursive(|expr| {
        let rest = just('.').to(Node::Rest);
        let hold = just('_').to(Node::Hold);

        let int_u8 = text::int::<char, Simple<char>>(10)
            .map(|s: String| s.parse::<u8>().unwrap());
        
        let float = text::int(10)
            .chain::<char, _, _>(just('.').chain(text::digits(10)).or_not().flatten())
            .collect::<String>()
            .map(|s| s.parse::<f32>().unwrap());

        let note_name = choice((
            just("C#"), just("Db"), just("D#"), just("Eb"),
            just("F#"), just("Gb"), just("G#"), just("Ab"),
            just("A#"), just("Bb"),
            just("C"), just("D"), just("E"), just("F"),
            just("G"), just("A"), just("B")
        ));

        let pitch_str = note_name
            .then(text::int(10).map(|s: String| s.parse::<i32>().unwrap()))
            .map(|(n, oct)| {
                let base = match n {
                    "C" => 0, "C#" | "Db" => 1, "D" => 2, "D#" | "Eb" => 3,
                    "E" => 4, "F" => 5, "F#" | "Gb" => 6, "G" => 7,
                    "G#" | "Ab" => 8, "A" => 9, "A#" | "Bb" => 10, "B" => 11,
                    _ => 0,
                };
                ((oct + 1) * 12 + base).clamp(0, 127) as u8
            });

        let pitch = pitch_str.or(int_u8);
        let velocity = just('@').ignore_then(int_u8);
        let gate = just('%').ignore_then(int_u8);
        let prob = just('?').ignore_then(int_u8);

        let chord_or_note = pitch
            .separated_by(just('+'))
            .at_least(1)
            .then(velocity.or_not())
            .then(gate.or_not())
            .then(prob.or_not())
            .map(|(((pitches, v), g), pr)| {
                let notes: Vec<Node> = pitches.into_iter().map(|p| Node::Note {
                    pitch: p,
                    velocity: v.unwrap_or(100),
                    gate: g.unwrap_or(100),
                    prob: pr.unwrap_or(100),
                }).collect();
                
                if notes.len() == 1 {
                    notes.into_iter().next().unwrap()
                } else {
                    Node::Chord(notes)
                }
            });

        let seq_group = expr.clone()
            .padded()
            .repeated()
            .delimited_by(just('['), just(']'))
            .map(Node::Sequence);

        let alt_group = expr.clone()
            .padded()
            .repeated()
            .delimited_by(just('<'), just('>'))
            .map(Node::Alternator);

        let parallel_layer = expr.clone()
            .padded()
            .repeated();

        let parallel_group = parallel_layer
            .separated_by(just('|'))
            .delimited_by(just('{'), just('}'))
            .map(Node::Parallel);

        let atom = choice((
            rest, hold, seq_group, alt_group, parallel_group, chord_or_note,
        ));

        enum Postfix {
            Euclidean(u8, u8),
            Mul(f32),
            Div(f32),
        }

        let euclidean = just('(')
            .ignore_then(int_u8)
            .then_ignore(just(','))
            .then(int_u8)
            .then_ignore(just(')'))
            .map(|(p, s)| Postfix::Euclidean(p, s));

        let speed_mul = just('*').ignore_then(float).map(Postfix::Mul);
        let speed_div = just('/').ignore_then(float).map(Postfix::Div);

        let postfix = choice((euclidean, speed_mul, speed_div));

        atom.then(postfix.repeated()).map(|(base, postfixes)| {
            postfixes.into_iter().fold(base, |acc, post| match post {
                Postfix::Euclidean(p, s) => Node::Euclidean(Box::new(acc), p, s),
                Postfix::Mul(val) => Node::SpeedModifier(Box::new(acc), val),
                Postfix::Div(val) => Node::SpeedModifier(Box::new(acc), 1.0 / val),
            })
        })
    });

    let float_f64 = text::int::<char, Simple<char>>(10)
        .chain::<char, _, _>(just('.').chain(text::digits(10)).or_not().flatten())
        .collect::<String>()
        .map(|s| s.parse::<f64>().unwrap());

    let bpm_decl = just("#BPM=")
        .ignore_then(float_f64)
        .padded()
        .or_not();

    let silence_decl = just("#SILENCE")
        .padded()
        .or_not()
        .map(|s| s.is_some());

    let track = just('!')
        .or_not()
        .map(|m| m.is_some())
        .then_ignore(just('T'))
        .then(text::int(10).map(|s: String| s.parse::<u8>().unwrap()))
        .then_ignore(just(':'))
        .padded()
        .then(expr.padded().repeated().map(Node::Sequence))
        .map(|((is_muted, ch), root_node)| Track {
            channel: ch.saturating_sub(1).min(15), 
            is_muted,
            root_node,
        });

    bpm_decl
        .then(silence_decl)
        .then(track.repeated())
        .map(|((bpm, global_silence), tracks)| Program { bpm, global_silence, tracks })
        .then_ignore(end())
}
