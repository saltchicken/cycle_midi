use super::primitives::{float_f64, pad_char, padding, pitch_val, int_u8, int_usize, int_i32, kw};
use crate::ast::{QuantizeMode, ScaleDef, ScaleSequence};
use chumsky::prelude::*;

#[derive(Clone)]
enum Directive {
    Bpm(f64),
    Signature(u8, u8),
    Quantize(QuantizeMode),
    Scale(ScaleDef),
    ScaleSeq(ScaleSequence),
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
        just("chromatic").to(vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]),
    ))
}

pub fn scale_def() -> impl Parser<char, ScaleDef, Error = Simple<char>> + Clone {
    pitch_val() // Swapped back to parsing string notation like C4
        .then_ignore(just(' ').repeated().at_least(1))
        .then(scale_name())
        .map(|(root, intervals)| ScaleDef {
            root_pitch: root,
            intervals,
        })
}

pub fn scale_seq_def() -> impl Parser<char, ScaleSequence, Error = Simple<char>> + Clone {
    let explicit = pad_char('(')
        .ignore_then(int_usize())
        .then_ignore(pad_char(','))
        .then(int_usize())
        .then_ignore(pad_char(')'))
        .then_ignore(pad_char(':'))
        .then(scale_def())
        .map(|((start, end), scale)| (start, end, scale))
        .separated_by(pad_char('|'))
        .delimited_by(pad_char('{'), pad_char('}'))
        .map(ScaleSequence::Explicit);

    let shift_parser = kw("shift")
        .ignore_then(pad_char('('))
        .ignore_then(scale_def().padded_by(padding()))
        .then_ignore(pad_char(','))
        .then(int_i32().padded_by(padding()))
        .then(pad_char(',').ignore_then(int_usize().padded_by(padding())).or_not())
        .then_ignore(pad_char(')'))
        .map(|((base_scale, shift_semitones), cycles)| ScaleSequence::Algorithmic {
            base_scale,
            shift_semitones,
            macro_cycles_per_step: cycles.unwrap_or(1),
        });

    let circle_parser = kw("circle_of_fifths")
        .ignore_then(pad_char('('))
        .ignore_then(scale_def().padded_by(padding()))
        .then(pad_char(',').ignore_then(int_usize().padded_by(padding())).or_not())
        .then_ignore(pad_char(')'))
        .map(|(base_scale, cycles)| ScaleSequence::Algorithmic {
            base_scale,
            shift_semitones: 7, // Perfect fifth up
            macro_cycles_per_step: cycles.unwrap_or(1),
        });

    choice((circle_parser, shift_parser, explicit))
}

pub fn global_directives() -> impl Parser<
    char,
    (
        Option<f64>,
        Option<(u8, u8)>,
        Option<QuantizeMode>,
        Option<ScaleDef>,
        Option<ScaleSequence>,
        bool,
        Vec<String>,
    ),
    Error = Simple<char>,
> + Clone {
    let directive = choice((
        just("#BPM=").ignore_then(float_f64()).map(Directive::Bpm),
        just("#SIG=")
            .ignore_then(int_u8())
            .then_ignore(just('/'))
            .then(int_u8())
            .map(|(num, den)| Directive::Signature(num, den)),
        just("#QUANTIZE=")
            .ignore_then(choice((
                just("auto").to(QuantizeMode::Auto),
                just("AUTO").to(QuantizeMode::Auto),
                int_usize().map(QuantizeMode::Fixed),
            )))
            .map(Directive::Quantize),
        just("#SCALE=")
            .ignore_then(scale_def())
            .map(Directive::Scale),
        just("#SCALE_SEQ=")
            .or(just("#SCALE_SEQ"))
            .padded_by(padding())
            .ignore_then(scale_seq_def())
            .map(Directive::ScaleSeq),
        just("#SILENCE").to(Directive::Silence),
        just("#INCLUDE")
            .padded_by(padding())
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
        let mut scale_seq = None;
        let mut global_silence = false;
        let mut includes = Vec::new();

        for d in dirs {
            match d {
                Directive::Bpm(v) => bpm = Some(v),
                Directive::Signature(n, d) => signature = Some((n, d)),
                Directive::Quantize(v) => quantize = Some(v),
                Directive::Scale(v) => scale = Some(v),
                Directive::ScaleSeq(v) => scale_seq = Some(v),
                Directive::Silence => global_silence = true,
                Directive::Include(path) => includes.push(path),
            }
        }

        (
            bpm,
            signature,
            quantize,
            scale,
            scale_seq,
            global_silence,
            includes,
        )
    })
}
