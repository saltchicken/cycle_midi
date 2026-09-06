use super::primitives::{float_f64, padding, pitch_val};
use crate::ast::{QuantizeMode, ScaleDef};
use chumsky::prelude::*;

#[derive(Clone)]
enum Directive {
    Bpm(f64),
    Signature(u8, u8),
    Quantize(QuantizeMode),
    Scale(ScaleDef),
    Silence,
    Include(String),
}

pub fn scale_name() -> impl Parser<char, Vec<u8>, Error = Simple<char>> + Clone {
    choice((
        just("major").to(vec![0, 2, 4, 5, 7, 9, 11]),
        just("minor_pentatonic").to(vec![0, 3, 5, 7, 10]),
        just("minor").to(vec![0, 2, 3, 5, 7, 8, 10]),
        just("dorian").to(vec![0, 2, 3, 5, 7, 9, 10]),
        just("phrygian").to(vec![0, 1, 3, 5, 7, 8, 10]),
        just("lydian").to(vec![0, 2, 4, 6, 7, 9, 11]),
        just("mixolydian").to(vec![0, 2, 4, 5, 7, 9, 10]),
        just("locrian").to(vec![0, 1, 3, 5, 6, 8, 10]),
        just("pentatonic").to(vec![0, 2, 4, 7, 9]),
    ))
}

pub fn scale_def() -> impl Parser<char, ScaleDef, Error = Simple<char>> + Clone {
    pitch_val()
        .then_ignore(just(' ').repeated().at_least(1))
        .then(scale_name())
        .map(|(root, intervals)| ScaleDef {
            root_pitch: root,
            intervals,
        })
}

pub fn global_directives()
-> impl Parser<char, (Option<f64>, Option<(u8, u8)>, Option<QuantizeMode>, Option<ScaleDef>, bool, Vec<String>), Error = Simple<char>>
+ Clone {
    let directive = choice((
        just("#BPM=").ignore_then(float_f64()).map(Directive::Bpm),
        just("#SIG=")
            .ignore_then(
                text::int::<char, Simple<char>>(10).try_map(|s, span| {
                    s.parse::<u8>()
                        .map_err(|e| Simple::custom(span, format!("Invalid numerator: {}", e)))
                })
            )
            .then_ignore(just('/'))
            .then(
                text::int::<char, Simple<char>>(10).try_map(|s, span| {
                    s.parse::<u8>()
                        .map_err(|e| Simple::custom(span, format!("Invalid denominator: {}", e)))
                })
            )
            .map(|(num, den)| Directive::Signature(num, den)),
        just("#QUANTIZE=")
            .ignore_then(choice((
                just("auto").to(QuantizeMode::Auto),
                just("AUTO").to(QuantizeMode::Auto),
                text::int::<char, Simple<char>>(10).try_map(|s, span| {
                    s.parse::<usize>()
                        .map_err(|e| Simple::custom(span, format!("Invalid quantize: {}", e)))
                        .map(QuantizeMode::Fixed)
                }),
            )))
            .map(Directive::Quantize),
        just("#SCALE=")
            .ignore_then(scale_def())
            .map(Directive::Scale),
        just("#SILENCE").to(Directive::Silence),
        just("#INCLUDE").padded_by(padding())
            .ignore_then(just('"'))
            .ignore_then(filter(|c: &char| *c != '"').repeated().collect::<String>())
            .then_ignore(just('"'))
            .map(Directive::Include),
    ))
    .padded_by(padding());

    directive.repeated().map(|dirs| {
        let mut bpm = None;
        let mut signature = None;
        let mut quantize = None;
        let mut scale = None;
        let mut global_silence = false;
        let mut includes = Vec::new();

        for d in dirs {
            match d {
                Directive::Bpm(v) => bpm = Some(v),
                Directive::Signature(n, d) => signature = Some((n, d)),
                Directive::Quantize(v) => quantize = Some(v),
                Directive::Scale(v) => scale = Some(v),
                Directive::Silence => global_silence = true,
                Directive::Include(path) => includes.push(path),
            }
        }

        (bpm, signature, quantize, scale, global_silence, includes)
    })
}
