use chumsky::prelude::*;

pub fn padding() -> impl Parser<char, (), Error = Simple<char>> + Clone {
    let comment = just("//")
        .ignore_then(filter(|c: &char| *c != '\n').repeated())
        .ignored();
    comment.padded().repeated().padded().ignored()
}

pub fn kw(s: &'static str) -> impl Parser<char, &'static str, Error = Simple<char>> + Clone {
    just(s).padded_by(padding())
}

pub fn pad_char(c: char) -> impl Parser<char, char, Error = Simple<char>> + Clone {
    just(c).padded_by(padding())
}

pub fn int_u8() -> impl Parser<char, u8, Error = Simple<char>> + Clone {
    text::int::<char, Simple<char>>(10).try_map(|s: String, span| {
        s.parse::<u8>()
            .map_err(|e| Simple::custom(span, format!("Invalid u8: {}", e)))
    })
}

pub fn int_i32() -> impl Parser<char, i32, Error = Simple<char>> + Clone {
    just('-')
        .or_not()
        .then(text::int::<char, Simple<char>>(10))
        .try_map(|(sign, s), span| {
            s.parse::<i32>()
                .map_err(|e| Simple::custom(span, format!("Invalid i32: {}", e)))
                .map(|num| if sign.is_some() { -num } else { num })
        })
}

pub fn float_f32() -> impl Parser<char, f32, Error = Simple<char>> + Clone {
    just('-')
        .or_not()
        .then(
            text::int::<char, Simple<char>>(10)
                .chain::<char, _, _>(just('.').chain(text::digits(10)).or_not().flatten())
                .collect::<String>()
        )
        .try_map(|(sign, s), span| {
            s.parse::<f32>()
                .map_err(|e| Simple::custom(span, format!("Invalid float: {}", e)))
                .map(|num| if sign.is_some() { -num } else { num })
        })
}

pub fn float_f64() -> impl Parser<char, f64, Error = Simple<char>> + Clone {
    just('-')
        .or_not()
        .then(
            text::int::<char, Simple<char>>(10)
                .chain::<char, _, _>(just('.').chain(text::digits(10)).or_not().flatten())
                .collect::<String>()
        )
        .try_map(|(sign, s), span| {
            s.parse::<f64>()
                .map_err(|e| Simple::custom(span, format!("Invalid f64: {}", e)))
                .map(|num| if sign.is_some() { -num } else { num })
        })
}

pub fn pitch_val() -> impl Parser<char, u8, Error = Simple<char>> + Clone {
    let note_name = choice((
        just("C#"),
        just("Db"),
        just("D#"),
        just("Eb"),
        just("F#"),
        just("Gb"),
        just("G#"),
        just("Ab"),
        just("A#"),
        just("Bb"),
        just("C"),
        just("D"),
        just("E"),
        just("F"),
        just("G"),
        just("A"),
        just("B"),
    ));

    note_name
        .then(text::int(10).try_map(|s: String, span| {
            s.parse::<i32>()
                .map_err(|e| Simple::custom(span, format!("Invalid octave: {}", e)))
        }))
        .map(|(n, oct)| {
            let base = match n {
                "C" => 0,
                "C#" | "Db" => 1,
                "D" => 2,
                "D#" | "Eb" => 3,
                "E" => 4,
                "F" => 5,
                "F#" | "Gb" => 6,
                "G" => 7,
                "G#" | "Ab" => 8,
                "A" => 9,
                "A#" | "Bb" => 10,
                "B" => 11,
                _ => 0,
            };
            ((oct + 1) * 12 + base).clamp(0, 127) as u8
        })
}

pub fn drum_val() -> impl Parser<char, u8, Error = Simple<char>> + Clone {
    choice((
        just("bd").to(36u8), // Bass Drum / Kick
        just("sn").to(38u8), // Snare
        just("cp").to(39u8), // Clap
        just("lt").to(41u8), // Low Tom
        just("ch").to(42u8), // Closed Hi-Hat
        just("mt").to(45u8), // Mid Tom
        just("oh").to(46u8), // Open Hi-Hat
        just("ht").to(48u8), // High Tom
    ))
}
