use super::primitives::{float_f32, float_f64, int_i32, int_u8, int_usize, kw, pad_char, padding};
use crate::ast::{ArpStyle, ExtractType, Modifier, Node};
use chumsky::prelude::*;

#[derive(Clone)]
pub enum PostfixOp {
    Euclidean(u8, u8),
    Span(usize),
    Arp(ArpStyle),
    Ratchet(u8),
    Stut(u8, f32, f32),
    Humanize(u8, f64),
    Prob(u8),
    PhaseShift(f32),
    Invert(i32),
    Drop(u8),
    Transpose(i32),
    Off(f32, Vec<PostfixOp>),
    Strum(f64),
    ExtractPitch(ExtractType, Option<i32>, i32),
    Chordify(Option<i32>, i32),
    VelocityOverride(u8),
    GateOverride(u8),
    Wrap,
}

// Helper: Optionally parses a keyword argument label (e.g., "depth:") before a value
fn opt_kw_arg<'a, T: 'a, P: Parser<char, T, Error = Simple<char>> + Clone + 'a>(
    parser: P,
) -> impl Parser<char, T, Error = Simple<char>> + Clone + 'a {
    text::ident()
        .padded_by(padding())
        .then_ignore(pad_char(':'))
        .or_not()
        .ignore_then(parser)
}

pub fn postfix_parser() -> impl Parser<char, PostfixOp, Error = Simple<char>> + Clone {
    recursive(|postfix| {
        let arp_style = choice((
            just("updown").to(ArpStyle::UpDown),
            just("downup").to(ArpStyle::DownUp),
            just("converge").to(ArpStyle::Converge),
            just("diverge").to(ArpStyle::Diverge),
            just("pinkyupdown").to(ArpStyle::PinkyUpDown),
            just("pinkyup").to(ArpStyle::PinkyUp),
            just("up").to(ArpStyle::Up),
            just("down").to(ArpStyle::Down),
        ));

        let extract_type = choice((
            kw("highest").to(ExtractType::Highest),
            kw("high").to(ExtractType::Highest),
            kw("lowest").to(ExtractType::Lowest),
            kw("low").to(ExtractType::Lowest),
        ));

        // Method chained modifiers (require a leading dot, spaces permitted after dot)
        let method = just('.').ignore_then(choice((
            kw("euclid").or(kw("E"))
                .ignore_then(pad_char('('))
                .ignore_then(opt_kw_arg(int_u8()))
                .then_ignore(pad_char(','))
                .then(opt_kw_arg(int_u8()))
                .then_ignore(pad_char(')'))
                .map(|(p, s)| PostfixOp::Euclidean(p, s)),

            kw("arp")
                .ignore_then(pad_char('('))
                .ignore_then(opt_kw_arg(arp_style))
                .then_ignore(pad_char(')'))
                .map(PostfixOp::Arp),

            kw("stut")
                .ignore_then(pad_char('('))
                .ignore_then(opt_kw_arg(int_u8()))
                .then_ignore(pad_char(','))
                .then(opt_kw_arg(float_f32()))
                .then_ignore(pad_char(','))
                .then(opt_kw_arg(float_f32()))
                .then_ignore(pad_char(')'))
                .map(|((d, f), t)| PostfixOp::Stut(d, f, t)),

            kw("drop")
                .ignore_then(pad_char('('))
                .ignore_then(opt_kw_arg(int_u8()))
                .then_ignore(pad_char(')'))
                .map(PostfixOp::Drop),

            kw("shift")
                .ignore_then(pad_char('('))
                .ignore_then(opt_kw_arg(float_f32()))
                .then_ignore(pad_char(')'))
                .map(PostfixOp::PhaseShift),

            kw("humanize")
                .ignore_then(
                    pad_char('(')
                        .ignore_then(
                            opt_kw_arg(int_u8())
                                .then(pad_char(',').ignore_then(opt_kw_arg(float_f64())).then_ignore(kw("ms").or_not()).or_not())
                                .or_not()
                        )
                        .then_ignore(pad_char(')'))
                        .or_not()
                )
                .map(|args_opt| {
                    let (vel, time) = match args_opt.flatten() {
                        Some((v, t)) => (v, t.unwrap_or(0.0)),
                        None => (0, 0.0),
                    };
                    PostfixOp::Humanize(vel, time)
                }),

            kw("off")
                .ignore_then(pad_char('('))
                .ignore_then(opt_kw_arg(float_f32()))
                .then_ignore(pad_char(','))
                .then(postfix.repeated())
                .then_ignore(pad_char(')'))
                .map(|(shift, ops)| PostfixOp::Off(shift, ops)),

            kw("strum")
                .ignore_then(pad_char('('))
                .ignore_then(opt_kw_arg(float_f64()))
                .then_ignore(pad_char(')'))
                .map(PostfixOp::Strum),

            kw("extract")
                .ignore_then(
                    pad_char('(')
                        .ignore_then(opt_kw_arg(extract_type))
                        .then(
                            pad_char(',')
                                .ignore_then(opt_kw_arg(int_i32()).padded_by(padding()))
                                .then(pad_char(',').ignore_then(opt_kw_arg(int_i32()).padded_by(padding())).or_not())
                                .or_not(),
                        )
                        .then_ignore(pad_char(')'))
                )
                .map(|(ext_type, args)| match args {
                    Some((limit, offset)) => PostfixOp::ExtractPitch(ext_type, Some(limit), offset.unwrap_or(0)),
                    None => PostfixOp::ExtractPitch(ext_type, Some(1), 0),
                }),

            kw("chordify")
                .ignore_then(
                    pad_char('(')
                        .ignore_then(opt_kw_arg(int_i32()).padded_by(padding()))
                        .then(pad_char(',').ignore_then(opt_kw_arg(int_i32()).padded_by(padding())).or_not())
                        .then_ignore(pad_char(')'))
                        .or_not()
                )
                .map(|args| match args {
                    Some((limit, offset)) => PostfixOp::Chordify(Some(limit), offset.unwrap_or(0)),
                    None => PostfixOp::Chordify(None, 0),
                }),

            kw("wrap")
                .then_ignore(pad_char('(').then_ignore(pad_char(')')).or_not())
                .map(|_| PostfixOp::Wrap),

            kw("vel").or(kw("v"))
                .ignore_then(pad_char('('))
                .ignore_then(opt_kw_arg(int_u8()))
                .then_ignore(pad_char(')'))
                .map(PostfixOp::VelocityOverride),

            kw("gate").or(kw("g"))
                .ignore_then(pad_char('('))
                .ignore_then(opt_kw_arg(int_u8()))
                .then_ignore(pad_char(')'))
                .map(PostfixOp::GateOverride),
        )));

        // Core structural modifiers - rigidly bound (NO leading padding allowed)
        let symbolic = choice((
            just('*').ignore_then(int_u8().padded_by(padding())).map(PostfixOp::Ratchet),
            just('?').ignore_then(int_u8().padded_by(padding())).map(PostfixOp::Prob),
            just('^').ignore_then(int_i32().padded_by(padding())).map(PostfixOp::Invert),
            just('/').ignore_then(int_usize().padded_by(padding())).map(PostfixOp::Span),
            
            // Transpose modifiers: strictly reject if followed immediately by an accidental (# or b)
            // so we don't accidentally consume negative pitches like -14# as a transpose operation.
            just('+')
                .ignore_then(int_usize())
                .then_ignore(choice((just('#'), just('b'))).not().rewind())
                .map(|v| PostfixOp::Transpose(v as i32)),
                
            just('-')
                .ignore_then(int_usize())
                .then_ignore(choice((just('#'), just('b'))).not().rewind())
                .map(|v| PostfixOp::Transpose(-(v as i32))),
        ));

        // Explicitly NOT padded_by(padding()) overall to ensure tight binding on the left
        choice((method, symbolic))
    })
}

pub fn apply_postfix(mut acc: Node, post: PostfixOp) -> Node {
    match post {
        PostfixOp::Off(shift, mods) => {
            let mut shifted = acc.clone();
            for m in mods {
                shifted = apply_postfix(shifted, m);
            }
            shifted = Node::Modified(Box::new(shifted), vec![Modifier::PhaseShift(shift)]);
            Node::Parallel(vec![vec![acc], vec![shifted]])
        }
        other => {
            let modifier = match other {
                PostfixOp::Euclidean(p, s) => Modifier::Euclidean(p, s),
                PostfixOp::Span(val) => Modifier::Span(val),
                PostfixOp::Arp(style) => Modifier::Arp(style),
                PostfixOp::Ratchet(splits) => Modifier::Ratchet(splits),
                PostfixOp::Stut(d, f, t) => Modifier::Stut(d, f, t),
                PostfixOp::Humanize(vel, time) => Modifier::Humanize(vel, time),
                PostfixOp::Prob(p) => Modifier::Probability(p),
                PostfixOp::PhaseShift(val) => Modifier::PhaseShift(val),
                PostfixOp::Invert(amount) => Modifier::Invert(amount),
                PostfixOp::Drop(voice) => Modifier::Drop(voice),
                PostfixOp::Transpose(amt) => Modifier::Transpose(amt),
                PostfixOp::Strum(amt) => Modifier::Strum(amt),
                PostfixOp::ExtractPitch(ext_type, limit, offset) => Modifier::ExtractPitch(ext_type, limit, offset),
                PostfixOp::Chordify(limit, offset) => Modifier::Chordify(limit, offset),
                PostfixOp::VelocityOverride(v) => Modifier::VelocityOverride(v),
                PostfixOp::GateOverride(g) => Modifier::GateOverride(g),
                PostfixOp::Wrap => Modifier::Wrap,
                PostfixOp::Off(..) => unreachable!(),
            };
            match acc {
                Node::Modified(_, ref mut mods) => {
                    mods.push(modifier);
                    acc
                }
                _ => Node::Modified(Box::new(acc), vec![modifier])
            }
        }
    }
}
