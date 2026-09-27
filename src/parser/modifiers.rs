use super::primitives::{float_f32, float_f64, int_u8, int_usize, kw, pad_char, padding};
use crate::ast::{ArpStyle, Modifier, Node};
use chumsky::prelude::*;

#[derive(Clone)]
pub enum PostfixOp {
    Velocity(u8),
    Gate(u8),
    Humanize(u8, f64),
    Probability(u8),
    Transpose(i32),
    Arp(ArpStyle),
    Span(usize),
    Speed(f64),
    Stut(u8, f32, f32),
    Euclidean(u8, u8),
}

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
    recursive(|_postfix| {
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
            kw("speed")
                .ignore_then(pad_char('('))
                .ignore_then(opt_kw_arg(float_f64()))
                .then_ignore(pad_char(')'))
                .map(PostfixOp::Speed),
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
            kw("vel").or(kw("v"))
                .ignore_then(pad_char('('))
                .ignore_then(opt_kw_arg(int_u8()))
                .then_ignore(pad_char(')'))
                .map(PostfixOp::Velocity),
            kw("gate").or(kw("g"))
                .ignore_then(pad_char('('))
                .ignore_then(opt_kw_arg(int_u8()))
                .then_ignore(pad_char(')'))
                .map(PostfixOp::Gate),
        )));

        let symbolic = choice((
            just('?').ignore_then(int_u8().padded_by(padding())).map(PostfixOp::Probability),
            just('/').ignore_then(int_usize().padded_by(padding())).map(PostfixOp::Span),
            just('+')
                .ignore_then(int_usize())
                .then_ignore(choice((just('#'), just('b'))).not().rewind())
                .map(|v| PostfixOp::Transpose(v as i32)),
            just('-')
                .ignore_then(int_usize())
                .then_ignore(choice((just('#'), just('b'))).not().rewind())
                .map(|v| PostfixOp::Transpose(-(v as i32))),
        ));

        choice((method, symbolic))
    })
}

pub fn apply_postfix(mut acc: Node, post: PostfixOp) -> Node {
    let modifier = match post {
        PostfixOp::Velocity(v) => Modifier::Velocity(v),
        PostfixOp::Gate(g) => Modifier::Gate(g),
        PostfixOp::Humanize(vel, time) => Modifier::Humanize(vel, time),
        PostfixOp::Probability(p) => Modifier::Probability(p),
        PostfixOp::Transpose(amt) => Modifier::Transpose(amt),
        PostfixOp::Arp(style) => Modifier::Arp(style),
        PostfixOp::Span(val) => Modifier::Span(val),
        PostfixOp::Speed(val) => Modifier::Speed(val),
        PostfixOp::Stut(d, f, t) => Modifier::Stut(d, f, t),
        PostfixOp::Euclidean(p, s) => Modifier::Euclidean(p, s),
    };
    match acc {
        Node::Modified(_, ref mut mods) => {
            mods.push(modifier);
            acc
        }
        _ => Node::Modified(Box::new(acc), vec![modifier]),
    }
}
