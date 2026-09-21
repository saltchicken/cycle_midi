use super::directives::global_directives;
use super::primitives::{float_f32, float_f64, int_i32, int_u8, kw, pad_char, padding};
use super::track::track_parser;
use crate::ast::{ArpStyle, DynamicValue, Modifier, Node, Pitch, Program};
use chumsky::prelude::*;
use std::collections::HashMap;

#[derive(Clone)]
enum PostfixOp {
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
    ExtractPitch(crate::ast::ExtractType, Option<i32>, i32),
    Chordify(Option<i32>, i32),
    VelocityOverride(u8),
    GateOverride(u8),
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

fn diatonic_chord_type() -> impl Parser<char, Vec<i32>, Error = Simple<char>> + Clone {
    choice((
        just("triad").or(just("t")).to(vec![0, 2, 4]),
        just("7th").or(just("7")).to(vec![0, 2, 4, 6]),
        just("9th").or(just("9")).to(vec![0, 2, 4, 6, 8]),
        just("sus2").to(vec![0, 1, 4]),
        just("sus4").to(vec![0, 3, 4]),
        just("m").to(vec![0, 3, 7]),
        just("maj").to(vec![0, 4, 7]),
    ))
}

fn chord_or_note() -> impl Parser<char, Node, Error = Simple<char>> + Clone {
    let accidental = choice((just('#').to(1), just('b').to(-1)))
        .repeated()
        .map(|accs| accs.into_iter().sum::<i32>());

    let numeric_pitch = int_i32()
        .then(accidental.clone())
        .map(|(degree, acc)| Pitch::Numeric(degree, acc));

    let single_pitch = numeric_pitch.clone().map(|p| vec![p]);

    let numeric_named_chord = int_i32()
        .then(accidental.clone())
        .then_ignore(just('\'')) 
        .then(diatonic_chord_type())
        .map(|((root_degree, acc), intervals)| {
            intervals
                .into_iter()
                .map(|interval| Pitch::Numeric(root_degree + interval, acc))
                .collect::<Vec<_>>()
        });

    choice((numeric_named_chord, single_pitch)).map(|pitches| {
        if pitches.len() == 1 {
            Node::Note {
                pitch: pitches[0].clone(),
                velocity: 100,
                gate: 100,
            }
        } else {
            let notes = pitches.into_iter().map(|p| Node::Note {
                pitch: p,
                velocity: 100,
                gate: 100,
            }).collect();
            Node::Chord(notes)
        }
    })
}

fn postfix_parser() -> impl Parser<char, PostfixOp, Error = Simple<char>> + Clone {
    recursive(|postfix| {
        let euclid_mod = pad_char('.').ignore_then(kw("euclid").or(kw("E")))
            .ignore_then(pad_char('('))
            .ignore_then(int_u8())
            .then_ignore(pad_char(','))
            .then(int_u8())
            .then_ignore(pad_char(')'))
            .map(|(p, s)| PostfixOp::Euclidean(p, s));

        let span_mod = pad_char('.').ignore_then(kw("span"))
            .ignore_then(pad_char('('))
            .ignore_then(text::int::<char, Simple<char>>(10).try_map(|s, span| {
                s.parse::<usize>().map_err(|e| Simple::custom(span, format!("Invalid span: {}", e)))
            }))
            .then_ignore(pad_char(')'))
            .or(pad_char('.').ignore_then(pad_char('/')).ignore_then(text::int::<char, Simple<char>>(10).try_map(|s, span| {
                s.parse::<usize>().map_err(|e| Simple::custom(span, format!("Invalid span: {}", e)))
            })))
            .map(PostfixOp::Span);

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

        let arp_mod = pad_char('.').ignore_then(kw("arp"))
            .ignore_then(pad_char('('))
            .ignore_then(arp_style)
            .then_ignore(pad_char(')'))
            .map(PostfixOp::Arp);

        let ratchet_mod = pad_char('.').ignore_then(kw("ratchet"))
            .ignore_then(pad_char('('))
            .ignore_then(int_u8())
            .then_ignore(pad_char(')'))
            .or(pad_char('.').ignore_then(pad_char('*')).ignore_then(int_u8()))
            .map(PostfixOp::Ratchet);

        let stut_mod = pad_char('.').ignore_then(kw("stut"))
            .ignore_then(pad_char('('))
            .ignore_then(int_u8())
            .then_ignore(pad_char(','))
            .then(float_f32())
            .then_ignore(pad_char(','))
            .then(float_f32())
            .then_ignore(pad_char(')'))
            .map(|((d, f), t)| PostfixOp::Stut(d, f, t));

        let prob_mod = pad_char('.').ignore_then(kw("prob")).ignore_then(pad_char('(')).ignore_then(int_u8()).then_ignore(pad_char(')'))
            .or(pad_char('?').ignore_then(int_u8()))
            .map(PostfixOp::Prob);

        let invert_mod = pad_char('.').ignore_then(kw("invert")).ignore_then(pad_char('(')).ignore_then(int_i32()).then_ignore(pad_char(')'))
            .or(pad_char('^').ignore_then(int_i32()))
            .map(PostfixOp::Invert);

        let drop_mod = pad_char('.').ignore_then(kw("drop"))
            .ignore_then(pad_char('('))
            .ignore_then(int_u8())
            .then_ignore(pad_char(')'))
            .map(PostfixOp::Drop);

        let phase_shift = pad_char('.').ignore_then(kw("shift"))
            .ignore_then(pad_char('('))
            .ignore_then(float_f32())
            .then_ignore(pad_char(')'))
            .map(PostfixOp::PhaseShift);

        let humanize_args = int_u8()
            .then(
                pad_char(',')
                    .ignore_then(float_f64())
                    .then_ignore(just("ms").padded_by(padding()).or_not())
                    .or_not(),
            )
            .or_not();

        let humanize_mod = pad_char('.').ignore_then(kw("humanize"))
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

        let octave_mod = pad_char('.').ignore_then(kw("octave"))
            .ignore_then(pad_char('(')).ignore_then(int_i32()).then_ignore(pad_char(')'))
            .map(PostfixOp::Transpose);

        let off_mod = pad_char('.').ignore_then(kw("off"))
            .ignore_then(pad_char('('))
            .ignore_then(float_f32())
            .then_ignore(pad_char(','))
            .then(postfix.repeated())
            .then_ignore(pad_char(')'))
            .map(|(shift, ops)| PostfixOp::Off(shift, ops));

        let strum_mod = pad_char('.').ignore_then(kw("strum"))
            .ignore_then(pad_char('('))
            .ignore_then(float_f64())
            .then_ignore(pad_char(')'))
            .map(PostfixOp::Strum);

        let extract_type = choice((
            kw("highest").to(crate::ast::ExtractType::Highest),
            kw("high").to(crate::ast::ExtractType::Highest),
            kw("lowest").to(crate::ast::ExtractType::Lowest),
            kw("low").to(crate::ast::ExtractType::Lowest),
        ));

        let extract_args = pad_char('(')
            .ignore_then(extract_type)
            .then(
                pad_char(',')
                    .ignore_then(int_i32())
                    .then(pad_char(',').ignore_then(int_i32()).or_not())
                    .or_not(),
            )
            .then_ignore(pad_char(')'));

        let extract_mod = pad_char('.').ignore_then(kw("extract"))
            .ignore_then(extract_args)
            .map(|(ext_type, args)| match args {
                Some((limit, offset)) => {
                    PostfixOp::ExtractPitch(ext_type, Some(limit), offset.unwrap_or(0))
                }
                None => PostfixOp::ExtractPitch(ext_type, Some(1), 0),
            });

        let chordify_args = pad_char('(')
            .ignore_then(int_i32())
            .then(pad_char(',').ignore_then(int_i32()).or_not())
            .then_ignore(pad_char(')'))
            .or_not();

        let chordify_mod = pad_char('.').ignore_then(kw("chordify"))
            .ignore_then(chordify_args)
            .map(|args| match args {
                Some((limit, offset)) => PostfixOp::Chordify(Some(limit), offset.unwrap_or(0)),
                None => PostfixOp::Chordify(None, 0),
            });

        let velocity_mod = pad_char('.').ignore_then(kw("vel").or(kw("v")))
            .ignore_then(pad_char('(')).ignore_then(int_u8()).then_ignore(pad_char(')'))
            .map(PostfixOp::VelocityOverride);

        let gate_mod = pad_char('.').ignore_then(kw("gate").or(kw("g")))
            .ignore_then(pad_char('(')).ignore_then(int_u8()).then_ignore(pad_char(')'))
            .map(PostfixOp::GateOverride);

        choice((
            ratchet_mod,
            span_mod,
            euclid_mod,
            arp_mod,
            stut_mod,
            invert_mod,
            drop_mod,
            prob_mod,
            phase_shift,
            humanize_mod,
            octave_mod,
            off_mod,
            strum_mod,
            extract_mod,
            chordify_mod,
            velocity_mod,
            gate_mod,
        ))
        .padded_by(padding())
    })
}

fn apply_postfix(mut acc: Node, post: PostfixOp) -> Node {
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

pub fn mmn_parser() -> impl Parser<char, Program, Error = Simple<char>> {
    let expr = recursive(move |expr| {
        let rest = just('.').to(Node::Rest);
        let hold = just('_').to(Node::Hold); 

        let alias_ref = just('$')
            .ignore_then(text::ident())
            .then_ignore(just('=').padded_by(padding()).not())
            .map(Node::Ref);

        let with_scale = kw("scale")
            .ignore_then(pad_char('('))
            .ignore_then(super::directives::scale_def())
            .then_ignore(pad_char(')'))
            .then(expr.clone())
            .map(|(scale, child)| Node::WithScale(scale, Box::new(child)));

        let seq_group = kw("seq")
            .ignore_then(pad_char('('))
            .ignore_then(expr.clone().padded_by(padding()).repeated())
            .then_ignore(pad_char(')'))
            .map(|mut seq| if seq.len() == 1 { seq.remove(0) } else { Node::Sequence(seq) });

        let alt_group = kw("alt")
            .ignore_then(pad_char('('))
            .ignore_then(expr.clone().padded_by(padding()).repeated())
            .then_ignore(pad_char(')'))
            .map(Node::Alternator);

        let rnd_branch = int_u8().padded_by(padding()).then_ignore(pad_char(':')).or_not()
            .then(expr.clone().padded_by(padding()).repeated().map(|mut seq| if seq.len() == 1 { seq.remove(0) } else { Node::Sequence(seq) }));
        let rnd_group = kw("rnd")
            .ignore_then(pad_char('('))
            .ignore_then(rnd_branch.separated_by(pad_char(',')))
            .then_ignore(pad_char(')'))
            .map(|choices| Node::RandomChoice(choices.into_iter().map(|(w, n)| (w.unwrap_or(1) as u32, n)).collect()));

        let par_group = kw("par")
            .ignore_then(pad_char('('))
            .ignore_then(expr.clone().padded_by(padding()).repeated().separated_by(pad_char(',')))
            .then_ignore(pad_char(')'))
            .map(Node::Parallel);

        let poly_group = kw("poly")
            .ignore_then(pad_char('('))
            .ignore_then(expr.clone().padded_by(padding()).repeated().separated_by(pad_char(',')))
            .then_ignore(pad_char(')'))
            .map(Node::Polymeter);
            
        let shuf_group = kw("shuf")
            .ignore_then(pad_char('('))
            .ignore_then(expr.clone().padded_by(padding()).repeated())
            .then_ignore(pad_char(')'))
            .map(Node::ShuffledSequence);

        let struct_group = kw("struct")
            .ignore_then(pad_char('('))
            .ignore_then(expr.clone())
            .then_ignore(pad_char(','))
            .then(expr.clone())
            .then_ignore(pad_char(')'))
            .map(|(mask, content)| Node::Struct(Box::new(mask), Box::new(content)));

        // Default grouping (Implicit Sequence)
        let implicit_seq = pad_char('[')
            .ignore_then(expr.clone().padded_by(padding()).repeated())
            .then_ignore(pad_char(']'))
            .map(|mut seq| if seq.len() == 1 { seq.remove(0) } else { Node::Sequence(seq) });

        let arrange_segment = pad_char('(')
            .ignore_then(text::int::<char, Simple<char>>(10).try_map(|s, span| {
                s.parse::<usize>().map_err(|e| Simple::custom(span, format!("Invalid start: {}", e)))
            }))
            .then_ignore(pad_char(','))
            .then(text::int::<char, Simple<char>>(10).try_map(|s, span| {
                s.parse::<usize>().map_err(|e| Simple::custom(span, format!("Invalid end: {}", e)))
            }))
            .then_ignore(pad_char(')'))
            .then_ignore(pad_char(':'))
            .then(expr.clone().padded_by(padding()).repeated().at_least(1).map(|mut seq| if seq.len() == 1 { seq.remove(0) } else { Node::Sequence(seq) }))
            .map(|((start, end), node)| (start, end, Box::new(node)));

        let arrange = kw("arrange")
            .ignore_then(arrange_segment.padded_by(padding()).then_ignore(pad_char(',').or_not()).repeated().delimited_by(pad_char('['), pad_char(']')))
            .map(|segments| Node::Arrange(segments));

        let chain_segment = text::int::<char, Simple<char>>(10).try_map(|s, span| {
                s.parse::<usize>().map_err(|e| Simple::custom(span, format!("Invalid duration: {}", e)))
            })
            .then_ignore(pad_char(':'))
            .then(expr.clone().padded_by(padding()).repeated().at_least(1).map(|mut seq| if seq.len() == 1 { seq.remove(0) } else { Node::Sequence(seq) }));

        let chain_loop = kw("chain")
            .ignore_then(chain_segment.padded_by(padding()).then_ignore(pad_char(',').or_not()).repeated().delimited_by(pad_char('['), pad_char(']')))
            .map(|segments| {
                let mut current_start = 0;
                let mut arrange_segments = Vec::new();

                for (duration, node) in segments {
                    let end = current_start + duration;
                    arrange_segments.push((current_start, end, Box::new(node)));
                    current_start = end;
                }

                Node::Arrange(arrange_segments)
            });

        let atom = choice((
            rest,
            hold,
            alias_ref,
            with_scale,
            implicit_seq,
            seq_group,
            alt_group,
            rnd_group,
            par_group,
            poly_group,
            shuf_group,
            struct_group,
            chain_loop,
            arrange,
            cc_parser(),
            chord_or_note(),
        ));

        atom.then(postfix_parser().repeated())
            .map(|(base, postfixes)| postfixes.into_iter().fold(base, apply_postfix))
    });

    let alias_def = just('$')
        .ignore_then(text::ident())
        .then_ignore(pad_char('='))
        .then(
            expr.clone()
                .padded_by(padding())
                .repeated()
                .at_least(1)
                .map(|mut seq| {
                    if seq.len() == 1 {
                        seq.remove(0)
                    } else {
                        Node::Sequence(seq)
                    }
                }),
        )
        .map(|(name, node)| TopLevelItem::Alias(name, node));

    let track_def = track_parser(expr).map(TopLevelItem::Track);

    let item = choice((alias_def, track_def)).padded_by(padding());

    global_directives()
        .then(item.repeated())
        .map(
            |((bpm, signature, quantize, scale, scale_seq, global_silence, includes), items)| {
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

                Program {
                    bpm,
                    signature,
                    quantize,
                    scale,
                    scale_seq,
                    global_silence,
                    includes,
                    aliases,
                    tracks,
                }
            },
        )
        .padded_by(padding())
        .then_ignore(end())
}
