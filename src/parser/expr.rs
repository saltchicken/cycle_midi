use super::directives::global_directives;
use super::primitives::{float_f32, float_f64, int_i32, int_u8, padding, pitch_val, kw, pad_char};
use super::track::track_parser;
use crate::ast::{ArpStyle, DynamicValue, Node, Pitch, Program};
use chumsky::prelude::*;
use std::collections::HashMap;

#[derive(Clone)]
enum PostfixOp {
    Euclidean(u8, u8),
    Mul(f32),
    Div(f32),
    Arp(ArpStyle),
    Only(usize, usize),
    MacroOnly(usize, usize),
    If(usize, usize),
    MacroIf(usize, usize),
    Prob(u8),
    PhaseShift(f32),
}

struct Postfix {
    op: PostfixOp,
    cond: Option<(usize, usize)>,
    m_cond: Option<(usize, usize)>,
}

enum TopLevelItem {
    Alias(String, Node),
    Track(crate::ast::Track),
}

fn dynamic_value() -> impl Parser<char, DynamicValue, Error = Simple<char>> + Clone {
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

fn cc_parser() -> impl Parser<char, Node, Error = Simple<char>> + Clone {
    just("cc")
        .or(just("CC"))
        .ignore_then(int_u8())
        .then(just('@').ignore_then(dynamic_value()).or_not())
        .map(|(controller, v)| Node::CC {
            controller,
            value: v.unwrap_or(DynamicValue::Static(127)),
        })
}

fn chord_or_note() -> impl Parser<char, Node, Error = Simple<char>> + Clone {
    let pitch = pitch_val()
        .map(Pitch::Absolute)
        .or(int_i32().map(Pitch::Numeric));

    let velocity = just('@').ignore_then(int_u8());
    let gate = just('%').ignore_then(int_u8());

    pitch
        .separated_by(pad_char('+'))
        .at_least(1)
        .then(velocity.or_not())
        .then(gate.or_not())
        .map(|((pitches, v), g)| {
            let notes: Vec<Node> = pitches
                .into_iter()
                .map(|p| Node::Note {
                    pitch: p,
                    velocity: v.unwrap_or(100),
                    gate: g.unwrap_or(100),
                })
                .collect();

            if notes.len() == 1 {
                notes.into_iter().next().unwrap()
            } else {
                Node::Chord(notes)
            }
        })
}

fn postfix_parser() -> impl Parser<char, Postfix, Error = Simple<char>> + Clone {
    let condition_clause = just("if(")
        .ignore_then(int_u8())
        .then(
            pad_char(',')
                .ignore_then(int_u8())
                .or_not(),
        )
        .then_ignore(just(')'))
        .map(|(interval, offset)| (interval as usize, offset.unwrap_or(interval.saturating_sub(1)) as usize));

    let m_condition_clause = just("m_if(")
        .ignore_then(int_u8())
        .then(
            pad_char(',')
                .ignore_then(int_u8())
                .or_not(),
        )
        .then_ignore(just(')'))
        .map(|(interval, offset)| (interval as usize, offset.unwrap_or(interval.saturating_sub(1)) as usize));

    let only_mod = just("only(")
        .ignore_then(int_u8())
        .then(
            pad_char(',')
                .ignore_then(int_u8())
                .or_not(),
        )
        .then_ignore(just(')'))
        .map(|(interval, offset)| PostfixOp::Only(interval as usize, offset.unwrap_or(interval.saturating_sub(1)) as usize));

    let m_only_mod = just("m_only(")
        .ignore_then(int_u8())
        .then(
            pad_char(',')
                .ignore_then(int_u8())
                .or_not(),
        )
        .then_ignore(just(')'))
        .map(|(interval, offset)| {
            PostfixOp::MacroOnly(interval as usize, offset.unwrap_or(interval.saturating_sub(1)) as usize)
        });

    let if_mod = just("if(")
        .ignore_then(int_u8())
        .then(
            pad_char(',')
                .ignore_then(int_u8())
                .or_not(),
        )
        .then_ignore(just(')'))
        .map(|(interval, offset)| PostfixOp::If(interval as usize, offset.unwrap_or(interval.saturating_sub(1)) as usize));

    let m_if_mod = just("m_if(")
        .ignore_then(int_u8())
        .then(
            pad_char(',')
                .ignore_then(int_u8())
                .or_not(),
        )
        .then_ignore(just(')'))
        .map(|(interval, offset)| {
            PostfixOp::MacroIf(interval as usize, offset.unwrap_or(interval.saturating_sub(1)) as usize)
        });

    let euclidean = just('(')
        .ignore_then(int_u8())
        .then_ignore(just(','))
        .then(int_u8())
        .then_ignore(just(')'))
        .map(|(p, s)| PostfixOp::Euclidean(p, s));

    let speed_mul = just('*').ignore_then(float_f32()).map(PostfixOp::Mul);
    let speed_div = just('/').ignore_then(float_f32()).map(PostfixOp::Div);

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

    let arp_mod = just("arp")
        .ignore_then(pad_char('('))
        .ignore_then(arp_style)
        .then_ignore(pad_char(')'))
        .map(PostfixOp::Arp);

    let prob_mod = just('?').ignore_then(int_u8()).map(PostfixOp::Prob);
    
    let phase_shift = choice((
        just("~>").padded_by(padding()).ignore_then(float_f32()),
        just("<~").padded_by(padding()).ignore_then(float_f32()).map(|v| -v),
        kw("shift").ignore_then(pad_char('(')).ignore_then(float_f32()).then_ignore(pad_char(')'))
    )).map(PostfixOp::PhaseShift);

    let postfix_op = choice((
        euclidean, speed_mul, speed_div, arp_mod, only_mod, m_only_mod, if_mod, m_if_mod, prob_mod, phase_shift
    ))
    .padded_by(padding());

    postfix_op
        .then(condition_clause.padded_by(padding()).or_not())
        .then(m_condition_clause.padded_by(padding()).or_not())
        .map(|((op, cond), m_cond)| Postfix { op, cond, m_cond })
}

pub fn mmn_parser() -> impl Parser<char, Program, Error = Simple<char>> {
    let pad_expr = padding();
    let expr = recursive(move |expr| {
        let rest = just('.').to(Node::Rest);
        let hold = just('_').to(Node::Hold);

        let alias_ref = just('$')
            .ignore_then(text::ident())
            .map(Node::Ref);

        let seq_group = expr
            .clone()
            .padded_by(pad_expr.clone())
            .repeated()
            .separated_by(pad_char('|'))
            .delimited_by(just('['), just(']'))
            .map(|choices| {
                if choices.len() == 1 {
                    Node::Sequence(choices.into_iter().next().unwrap())
                } else {
                    Node::RandomChoice(
                        choices
                            .into_iter()
                            .map(|seq| {
                                if seq.len() == 1 {
                                    seq.into_iter().next().unwrap()
                                } else {
                                    Node::Sequence(seq)
                                }
                            })
                            .collect(),
                    )
                }
            });

        let alt_group = expr
            .clone()
            .padded_by(pad_expr.clone())
            .repeated()
            .delimited_by(just('<'), just('>'))
            .map(Node::Alternator);

        let parallel_layer = expr.clone().padded_by(pad_expr.clone()).repeated();

        let parallel_group = parallel_layer
            .clone()
            .separated_by(pad_char('|'))
            .delimited_by(just('{'), just('}'))
            .map(Node::Parallel);

        let polymeter_group = parallel_layer
            .separated_by(pad_char(','))
            .delimited_by(just('{'), just('}'))
            .map(Node::Polymeter);
            
        let seqp_segment = pad_char('(')
            .ignore_then(text::int::<char, Simple<char>>(10).try_map(|s, span| {
                s.parse::<usize>()
                    .map_err(|e| Simple::custom(span, format!("Invalid start: {}", e)))
            }))
            .then_ignore(pad_char(','))
            .then(text::int::<char, Simple<char>>(10).try_map(|s, span| {
                s.parse::<usize>()
                    .map_err(|e| Simple::custom(span, format!("Invalid end: {}", e)))
            }))
            .then_ignore(pad_char(')'))
            .then_ignore(pad_char(':'))
            .then(expr.clone())
            .map(|((start, end), node)| (start, end, Box::new(node)));

        let seqp = kw("seqP")
            .ignore_then(
                seqp_segment
                    .clone()
                    .separated_by(pad_char('|'))
                    .delimited_by(pad_char('{'), pad_char('}')) // <-- FIX: Consume trailing padding before '}'
            )
            .map(|segments| Node::SeqP(segments, false));

        let seqploop = kw("seqPLoop")
            .ignore_then(
                seqp_segment
                    .separated_by(pad_char('|'))
                    .delimited_by(pad_char('{'), pad_char('}')) // <-- FIX: Consume trailing padding before '}'
            )
            .map(|segments| Node::SeqP(segments, true));

        let atom = choice((
            rest,
            hold,
            alias_ref,
            seq_group,
            alt_group,
            choice((
                parallel_group,
                polymeter_group,
                seqploop,  // <-- FIX: Reordered before seqp so "seqPLoop" isn't hijacked
                seqp,      
                cc_parser(),
                chord_or_note(),
            ))
        ));

        atom.then(postfix_parser().repeated())
            .map(|(base, postfixes)| {
                postfixes.into_iter().fold(base, |acc, post| {
                    let true_branch = match post.op {
                        PostfixOp::Euclidean(p, s) => Node::Euclidean(Box::new(acc.clone()), p, s),
                        PostfixOp::Mul(val) => Node::SpeedModifier(Box::new(acc.clone()), val),
                        PostfixOp::Div(val) => {
                            Node::SpeedModifier(Box::new(acc.clone()), 1.0 / val)
                        }
                        PostfixOp::Arp(style) => Node::Arp(Box::new(acc.clone()), style),
                        PostfixOp::Only(interval, offset) => Node::Condition {
                            interval,
                            offset,
                            true_branch: Box::new(acc.clone()),
                            false_branch: Box::new(Node::Rest),
                        },
                        PostfixOp::MacroOnly(interval, offset) => Node::MacroCondition {
                            interval,
                            offset,
                            is_gate: true,
                            true_branch: Box::new(acc.clone()),
                            false_branch: Box::new(Node::Rest),
                        },
                        PostfixOp::If(interval, offset) => Node::Condition {
                            interval,
                            offset,
                            true_branch: Box::new(acc.clone()),
                            false_branch: Box::new(Node::Rest),
                        },
                        PostfixOp::MacroIf(interval, offset) => Node::MacroCondition {
                            interval,
                            offset,
                            is_gate: false,
                            true_branch: Box::new(acc.clone()),
                            false_branch: Box::new(Node::Rest),
                        },
                        PostfixOp::Prob(p) => Node::Probability(Box::new(acc.clone()), p),
                        PostfixOp::PhaseShift(val) => Node::PhaseShift(Box::new(acc.clone()), val),
                    };

                    let micro_applied = match post.cond {
                        Some((interval, offset)) => Node::Condition {
                            interval,
                            offset,
                            true_branch: Box::new(true_branch),
                            false_branch: Box::new(acc.clone()),
                        },
                        None => true_branch,
                    };

                    match post.m_cond {
                        Some((interval, offset)) => Node::MacroCondition {
                            interval,
                            offset,
                            is_gate: false,
                            true_branch: Box::new(micro_applied),
                            false_branch: Box::new(acc),
                        },
                        None => micro_applied,
                    }
                })
            })
    });

    let alias_def = just('$')
        .ignore_then(text::ident())
        .then_ignore(pad_char('='))
        .then(expr.clone())
        .map(|(name, node)| TopLevelItem::Alias(name, node));

    let track_def = track_parser(expr)
        .map(TopLevelItem::Track);

    let item = choice((alias_def, track_def)).padded_by(padding());

    global_directives()
        .then(item.repeated())
        .try_map(|((bpm, quantize, scale, global_silence), items), span| {
            let mut aliases = HashMap::new();
            let mut tracks = Vec::new();

            for item in items {
                match item {
                    TopLevelItem::Alias(name, node) => {
                        aliases.insert(name, node);
                    }
                    TopLevelItem::Track(track) => {
                        tracks.push(track);
                    }
                }
            }

            for track in &mut tracks {
                if let Err(e) = track.root_node.expand_refs(&aliases, 0) {
                    return Err(Simple::custom(span, e));
                }
            }

            Ok(Program {
                bpm,
                quantize,
                scale,
                global_silence,
                tracks,
            })
        })
        .padded_by(padding())
        .then_ignore(end())
}
