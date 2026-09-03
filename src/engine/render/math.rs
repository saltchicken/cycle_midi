use crate::ast::{Pitch, ScaleDef};

pub fn lcm(a: usize, b: usize) -> usize {
    if a == 0 || b == 0 {
        return 0;
    }
    let mut x = a;
    let mut y = b;
    while y != 0 {
        let t = y;
        y = x % y;
        x = t;
    }
    let gcd = x;
    (a * b) / gcd
}

pub fn resolve_pitch(pitch: &Pitch, scale: &Option<ScaleDef>, octave_offset: i32) -> u8 {
    let shift = octave_offset * 12;
    match pitch {
        Pitch::Absolute(p) => (*p as i32 + shift).clamp(0, 127) as u8,
        Pitch::Numeric(val) => {
            let val = *val;
            if let Some(scale) = scale {
                let scale_len = scale.intervals.len() as i32;
                let octave = val.div_euclid(scale_len);
                let degree = val.rem_euclid(scale_len) as usize;
                let note = scale.root_pitch as i32
                    + (octave * 12)
                    + scale.intervals[degree] as i32
                    + shift;
                note.clamp(0, 127) as u8
            } else {
                (val + shift).clamp(0, 127) as u8
            }
        }
    }
}
