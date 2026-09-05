use super::directives::global_directives;
use super::primitives::{float_f32, float_f64, int_i32, int_u8, padding, pitch_val, drum_val, kw, pad_char};
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
    Ratchet(u8),
    Humanize(u8, f64), // Unified humanize
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

// Absolute semitone offsets for literal pitches (e.g., C4_maj)
fn absolute_chord_type() -> impl Parser<char, Vec<i32>, Error = Simple<char>> + Clone {
    choice((
        just("maj7").or(just("M7")).to(vec![0, 4, 7, 11]),
        just("min7").or(just("m7")).to(vec![0, 3, 7, 10]),
        just("dom7").to(vec![0, 4, 7, 10]),
        just("dim7").to(vec![0, 3, 6, 9]),
        just("m7b5").or(just("halfdim")).to(vec![0, 3, 6, 10]),
        just("aug7").to(vec![0, 4, 8, 10]),
        just("sus2").to(vec![0, 2, 7]),
        just("sus4").to(vec![0, 5, 7]),
        just("power").to(vec![0, 7]),
    ))
    .or(choice((
        just("maj").or(just("M")).to(vec![0, 4, 7]),
        just("min").or(just("m")).to(vec![0, 3, 7]),
        just("dim").to(vec![0, 3, 6]),
        just("aug").to(vec![0, 4, 8]),
        just("7").to(vec![0, 4, 7, 10]),
        just("5").to(vec![0, 7]),
    )))
}

// Scale degree offsets for numeric diatonic chords (e.g., 0_triad)
fn diatonic_chord_type() -> impl Parser<char, Vec<i32>, Error = Simple<char>> + Clone {
    choice((
        just("triad").or(just("t")).to(vec![0, 2, 4]),
        just("7th").or(just("7")).to(vec![0, 2, 4, 6]),
        just("9th").or(just("9")).to(vec![0, 2, 4, 6, 8]),
        just("sus2").to(vec![0, 1, 4]),
        just("sus4").to(vec![0, 3, 4]),
    ))
}

fn chord_or_note() -> impl Parser<char, Node, Error = Simple<char>> + Clone {
    let single_pitch = pitch_val()
        .or(drum_val())
        .map(Pitch::Absolute)
        .or(int_i32().map(Pitch::Numeric))
        .map(|p| vec![p]);

    let absolute_named_chord = pitch_val()
        .then_ignore(just('_'))
        .then(absolute_chord_type())
        .map(|(root, intervals)| {
            intervals
                .into_iter()
                .map(|interval| Pitch::Absolute((root as i32 + interval).clamp(0, 127) as u8))
                .collect::<Vec<_>>()
        });

    let numeric_named_chord = int_i32()
        .then_ignore(just('_'))
        .then(diatonic_chord_type())
        .map(|(root_degree, intervals)| {
            intervals
                .into_iter()
                .map(|interval| Pitch::Numeric(root_degree + interval))
                .collect::<Vec<_>>()
        });

    let pitch_group = choice((absolute_named_chord, numeric_named_chord, single_pitch));

    let velocity = pad_char('@').ignore_then(int_u8());
    let gate = pad_char('%').ignore_then(int_u8());

    pitch_group
        .separated_by(pad_char('+'))
        .at_least(1)
        .then(velocity.or_not())
        .then(gate.or_not())
        .map(|((pitch_groups, v), g)| {
            let pitches: Vec<Pitch> = pitch_groups.into_iter().flatten().collect();
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

    let euclidean = pad_char('(')
        .ignore_then(int_u8())
        .then_ignore(pad_char(','))
        .then(int_u8())
        .then_ignore(pad_char(')'))
        .map(|(p, s)| PostfixOp::Euclidean(p, s));

    let speed_mul = pad_char('*').ignore_then(float_f32()).map(PostfixOp::Mul);
    let speed_div = pad_char('/').ignore_then(float_f32()).map(PostfixOp::Div);

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

    let arp_mod = kw("arp")
        .ignore_then(pad_char('('))
        .ignore_then(arp_style)
        .then_ignore(pad_char(')'))
        .map(PostfixOp::Arp);

    let ratchet_mod = kw("ratchet")
        .ignore_then(pad_char('('))
        .ignore_then(int_u8())
        .then_ignore(pad_char(')'))
        .map(PostfixOp::Ratchet);

    let prob_mod = pad_char('?').ignore_then(int_u8()).map(PostfixOp::Prob);
    
    let phase_shift = choice((
        just("~>").padded_by(padding()).ignore_then(float_f32()),
        just("<~").padded_by(padding()).ignore_then(float_f32()).map(|v| -v),
        kw("shift").ignore_then(pad_char('(')).ignore_then(float_f32()).then_ignore(pad_char(')'))
    )).map(PostfixOp::PhaseShift);

    let humanize_args = int_u8()
        .then(
            pad_char(',')
                .ignore_then(float_f64())
                .then_ignore(just("ms").padded_by(padding()).or_not())
                .or_not()
        )
        .or_not();

    let humanize_mod = kw("humanize")
        .ignore_then(pad_char('('))
        .ignore_then(humanize_args)
        .then_ignore(pad_char(')'))
        .map(|args| {
            let (vel, time) = match args {
                Some((v, t)) => (v, t.unwrap_or(0.0)),
                None => (0, 0.0),
            };
            PostfixOp::Humanize(vel, time)
        });

    let postfix_op = choice((
        euclidean, speed_mul, speed_div, arp_mod, ratchet_mod, only_mod, m_only_mod, if_mod, m_if_mod, prob_mod, phase_shift, humanize_mod
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

        let choice_branch = int_u8()
            .padded_by(pad_expr.clone())
            .then_ignore(pad_char(':'))
            .or_not()
            .then(expr.clone().padded_by(pad_expr.clone()).repeated());

        let seq_group = choice_branch
            .separated_by(pad_char('|'))
            .delimited_by(just('['), just(']'))
            .map(|choices| {
                if choices.len() == 1 {
                    let (_, seq) = choices.into_iter().next().unwrap();
                    if seq.len() == 1 {
                        seq.into_iter().next().unwrap()
                    } else {
                        Node::Sequence(seq)
                    }
                } else {
                    Node::RandomChoice(
                        choices
                            .into_iter()
                            .map(|(w, seq)| {
                                let node = if seq.len() == 1 {
                                    seq.into_iter().next().unwrap()
                                } else {
                                    Node::Sequence(seq)
                                }
                                ;
                                (w.unwrap_or(1) as u32, node)
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
                    .delimited_by(pad_char('{'), pad_char('}'))
            )
            .map(|segments| Node::SeqP(segments, false));

        let seqploop = kw("seqPLoop")
            .ignore_then(
                seqp_segment
                    .separated_by(pad_char('|'))
                    .delimited_by(pad_char('{'), pad_char('}'))
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
                seqploop,
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
                        PostfixOp::Ratchet(splits) => Node::Ratchet(Box::new(acc.clone()), splits), // NEW
                        PostfixOp::Humanize(vel, time) => Node::Humanize(Box::new(acc.clone()), vel, time),
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
