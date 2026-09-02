use chumsky::prelude::*;
use crate::ast::{Node, Pitch, Program, ScaleDef, Track};

pub fn mmn_parser() -> impl Parser<char, Program, Error = Simple<char>> {
    let int_u8 = text::int::<char, Simple<char>>(10)
        .map(|s: String| s.parse::<u8>().unwrap());

    let int_i32 = just('-').or_not()
        .then(text::int::<char, Simple<char>>(10))
        .map(|(sign, s)| {
            let num = s.parse::<i32>().unwrap();
            if sign.is_some() { -num } else { num }
        });

    let note_name = choice((
        just("C#"), just("Db"), just("D#"), just("Eb"),
        just("F#"), just("Gb"), just("G#"), just("Ab"),
        just("A#"), just("Bb"),
        just("C"), just("D"), just("E"), just("F"),
        just("G"), just("A"), just("B")
    ));

    let pitch_val = note_name
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

    let expr = recursive(|expr| {
        let rest = just('.').to(Node::Rest);
        let hold = just('_').to(Node::Hold);

        let float = text::int(10)
            .chain::<char, _, _>(just('.').chain(text::digits(10)).or_not().flatten())
            .collect::<String>()
            .map(|s| s.parse::<f32>().unwrap());

        let pitch = pitch_val.clone().map(Pitch::Absolute)
            .or(int_i32.clone().map(Pitch::Numeric));

        let velocity = just('@').ignore_then(int_u8.clone());
        let gate = just('%').ignore_then(int_u8.clone());
        let prob = just('?').ignore_then(int_u8.clone());

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
            .ignore_then(int_u8.clone())
            .then_ignore(just(','))
            .then(int_u8)
            .then_ignore(just(')'))
            .map(|(p, s)| Postfix::Euclidean(p, s));

        let speed_mul = just('*').ignore_then(float.clone()).map(Postfix::Mul);
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

    // Fix: Added #[derive(Clone)] so chumsky can clone the enum during parsing combinations
    #[derive(Clone)]
    enum Directive {
        Bpm(f64),
        Quantize(usize),
        Scale(ScaleDef),
        Silence,
    }

    let scale_name = choice((
        just("major").to(vec![0, 2, 4, 5, 7, 9, 11]),
        just("minor_pentatonic").to(vec![0, 3, 5, 7, 10]),
        just("minor").to(vec![0, 2, 3, 5, 7, 8, 10]),
        just("dorian").to(vec![0, 2, 3, 5, 7, 9, 10]),
        just("phrygian").to(vec![0, 1, 3, 5, 7, 8, 10]),
        just("lydian").to(vec![0, 2, 4, 6, 7, 9, 11]),
        just("mixolydian").to(vec![0, 2, 4, 5, 7, 9, 10]),
        just("locrian").to(vec![0, 1, 3, 5, 6, 8, 10]),
        just("pentatonic").to(vec![0, 2, 4, 7, 9]),
    ));

    let scale_def = pitch_val.clone()
        .then_ignore(just(' ').repeated().at_least(1))
        .then(scale_name)
        .map(|(root, intervals)| ScaleDef { root_pitch: root, intervals });

    let directive = choice((
        just("#BPM=").ignore_then(float_f64).map(Directive::Bpm),
        just("#QUANTIZE=").ignore_then(text::int::<char, Simple<char>>(10).map(|s| s.parse::<usize>().unwrap())).map(Directive::Quantize),
        just("#SCALE=").ignore_then(scale_def.clone()).map(Directive::Scale),
        just("#SILENCE").to(Directive::Silence),
    )).padded();

    let directives = directive.repeated().map(|dirs| {
        let mut bpm = None;
        let mut quantize = None;
        let mut scale = None;
        let mut global_silence = false;
        
        for d in dirs {
            match d {
                Directive::Bpm(v) => bpm = Some(v),
                Directive::Quantize(v) => quantize = Some(v),
                Directive::Scale(v) => scale = Some(v),
                Directive::Silence => global_silence = true,
            }
        }
        
        (bpm, quantize, scale, global_silence)
    });

    let track_scale = scale_def
        .delimited_by(just('('), just(')'))
        .padded()
        .or_not();

    let track = just('!')
        .or_not()
        .map(|m| m.is_some())
        .then_ignore(just('T'))
        .then(text::int(10).map(|s: String| s.parse::<u8>().unwrap()))
        .then(track_scale)
        .then_ignore(just(':'))
        .padded()
        .then(expr.padded().repeated().map(Node::Sequence))
        .map(|(((is_muted, ch), scale), root_node)| Track {
            channel: ch.saturating_sub(1).min(15), 
            is_muted,
            scale,
            root_node,
        });

    directives
        .then(track.repeated())
        .map(|((bpm, quantize, scale, global_silence), tracks)| Program { 
            bpm, 
            quantize, 
            scale, 
            global_silence, 
            tracks 
        })
        .then_ignore(end())
}
