use super::directives::global_directives;
use super::primitives::{float_f32, float_f64, int_i32, int_u8, kw, pad_char, padding};
use super::track::track_parser;
use crate::ast::{ArpStyle, DynamicValue, Node, Pitch, Program};
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

// Scale degree offsets for numeric diatonic chords (e.g., 0'triad)
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
        .then_ignore(just('\'')) // FIX: Use apostrophe to avoid '_' hold clash
        .then(diatonic_chord_type())
        .map(|((root_degree, acc), intervals)| {
            intervals
                .into_iter()
                .map(|interval| Pitch::Numeric(root_degree + interval, acc))
                .collect::<Vec<_>>()
        });

    let pitch_group = choice((numeric_named_chord, single_pitch));

    let velocity = pad_char('@').ignore_then(int_u8());
    let gate = pad_char('%').ignore_then(int_u8());

    // Allow velocities and gates on each individual pitch component within the chord
    let modified_pitch_group = pitch_group
        .then(velocity.clone().or_not())
        .then(gate.clone().or_not());

    modified_pitch_group
        .separated_by(pad_char('&')) // FIX: Use '&' to avoid mathematical '+' clash
        .at_least(1)
        .then(velocity.or_not())
        .then(gate.or_not())
        .map(|((pitch_groups, global_v), global_g)| {
            let mut notes = Vec::new();
            for ((pitches, local_v), local_g) in pitch_groups {
                let v = global_v.unwrap_or_else(|| local_v.unwrap_or(100));
                let g = global_g.unwrap_or_else(|| local_g.unwrap_or(100));

                for p in pitches {
                    notes.push(Node::Note {
                        pitch: p,
                        velocity: v,
                        gate: g,
                    });
                }
            }

            if notes.len() == 1 {
                notes.into_iter().next().unwrap()
            } else {
                Node::Chord(notes)
            }
        })
}

fn postfix_parser() -> impl Parser<char, PostfixOp, Error = Simple<char>> + Clone {
    recursive(|postfix| {
        let euclidean = pad_char('(')
            .ignore_then(int_u8())
            .then_ignore(pad_char(','))
            .then(int_u8())
            .then_ignore(pad_char(')'))
            .map(|(p, s)| PostfixOp::Euclidean(p, s));

        // DX FIX: Shorthand multiplier for span (e.g., /2 instead of span(2))
        let shorthand_span = pad_char('/')
            .ignore_then(text::int::<char, Simple<char>>(10).try_map(|s, span| {
                s.parse::<usize>()
                    .map_err(|e| Simple::custom(span, format!("Invalid span: {}", e)))
            }))
            .map(PostfixOp::Span);

        let span_mod = kw("span")
            .ignore_then(pad_char('('))
            .ignore_then(text::int::<char, Simple<char>>(10).try_map(|s, span| {
                s.parse::<usize>()
                    .map_err(|e| Simple::custom(span, format!("Invalid span: {}", e)))
            }))
            .then_ignore(pad_char(')'))
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

        let arp_mod = kw("arp")
            .ignore_then(pad_char('('))
            .ignore_then(arp_style)
            .then_ignore(pad_char(')'))
            .map(PostfixOp::Arp);

        // DX FIX: Shorthand multiplier for ratchets (e.g., *4 instead of ratchet(4))
        let shorthand_ratchet = pad_char('*')
            .ignore_then(int_u8())
            .map(PostfixOp::Ratchet);

        let ratchet_mod = kw("ratchet")
            .ignore_then(pad_char('('))
            .ignore_then(int_u8())
            .then_ignore(pad_char(')'))
            .map(PostfixOp::Ratchet);

        let stut_mod = kw("stut")
            .ignore_then(pad_char('('))
            .ignore_then(int_u8()) // depth
            .then_ignore(pad_char(','))
            .then(float_f32()) // feedback multiplier
            .then_ignore(pad_char(','))
            .then(float_f32()) // time shift fraction
            .then_ignore(pad_char(')'))
            .map(|((d, f), t)| PostfixOp::Stut(d, f, t));

        let prob_mod = pad_char('?').ignore_then(int_u8()).map(PostfixOp::Prob);

        let invert_mod = pad_char('^').ignore_then(int_i32()).map(PostfixOp::Invert);

        let drop_mod = kw("drop")
            .ignore_then(pad_char('('))
            .ignore_then(int_u8())
            .then_ignore(pad_char(')'))
            .map(PostfixOp::Drop);

        let phase_shift = kw("shift")
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

        let octave_mod = kw("octave")
            .ignore_then(int_i32())
            .map(PostfixOp::Transpose);

        let off_mod = kw("off")
            .ignore_then(pad_char('('))
            .ignore_then(float_f32())
            .then_ignore(pad_char(','))
            .then(postfix.repeated())
            .then_ignore(pad_char(')'))
            .map(|(shift, ops)| PostfixOp::Off(shift, ops));

        let strum_mod = kw("strum")
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

        let extract_mod = kw("extract")
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

        let chordify_mod = kw("chordify")
            .ignore_then(chordify_args)
            .map(|args| match args {
                Some((limit, offset)) => PostfixOp::Chordify(Some(limit), offset.unwrap_or(0)),
                None => PostfixOp::Chordify(None, 0),
            });

        let velocity_mod = pad_char('@')
            .ignore_then(int_u8())
            .map(PostfixOp::VelocityOverride);

        choice((
            shorthand_ratchet,
            shorthand_span,
            euclidean,
            span_mod,
            arp_mod,
            ratchet_mod,
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
        ))
        .padded_by(padding())
    })
}

fn apply_postfix(acc: Node, post: PostfixOp) -> Node {
    match post {
        PostfixOp::Euclidean(p, s) => Node::Euclidean(Box::new(acc), p, s),
        PostfixOp::Span(val) => Node::Span(Box::new(acc), val),
        PostfixOp::Arp(style) => Node::Arp(Box::new(acc), style),
        PostfixOp::Ratchet(splits) => Node::Ratchet(Box::new(acc), splits),
        PostfixOp::Stut(d, f, t) => Node::Stut(Box::new(acc), d, f, t),
        PostfixOp::Humanize(vel, time) => Node::Humanize(Box::new(acc), vel, time),
        PostfixOp::PhaseShift(val) => Node::PhaseShift(Box::new(acc), val),
        PostfixOp::Invert(amount) => Node::Invert(Box::new(acc), amount),
        PostfixOp::Drop(voice) => Node::Drop(Box::new(acc), voice),
        PostfixOp::Prob(p) => Node::Probability(Box::new(acc), p),
        PostfixOp::Transpose(amt) => Node::Transpose(Box::new(acc), amt),
        PostfixOp::Strum(amt) => Node::Strum(Box::new(acc), amt),
        PostfixOp::ExtractPitch(ext_type, limit, offset) => {
            Node::ExtractPitch(Box::new(acc), ext_type, limit, offset)
        }
        PostfixOp::Chordify(limit, offset) => Node::Chordify(Box::new(acc), limit, offset),
        PostfixOp::VelocityOverride(v) => Node::VelocityOverride(Box::new(acc), v),

        PostfixOp::Off(shift, mods) => {
            let mut shifted = acc.clone();
            for m in mods {
                shifted = apply_postfix(shifted, m);
            }
            shifted = Node::PhaseShift(Box::new(shifted), shift);
            Node::Parallel(vec![vec![acc], vec![shifted]])
        }
    }
}

pub fn mmn_parser() -> impl Parser<char, Program, Error = Simple<char>> {
    let pad_expr = padding();
    let expr = recursive(move |expr| {
        let rest = just('.').to(Node::Rest);
        let hold = just('_').to(Node::Hold); // FIX: Now completely decoupled from chords

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

        // FIX: Unified Random Choice & Sequence to prevent parsing overlaps inside `[` `]`
        let choice_branch = int_u8()
            .padded_by(pad_expr.clone())
            .then_ignore(pad_char(':'))
            .or_not()
            .then(
                expr.clone()
                    .padded_by(pad_expr.clone())
                    .repeated()
                    .map(|seq| {
                        if seq.len() == 1 {
                            seq.into_iter().next().unwrap()
                        } else {
                            Node::Sequence(seq)
                        }
                    }),
            );

        let bracket_group = choice_branch
            .separated_by(pad_char('|'))
            .at_least(1)
            .delimited_by(pad_char('['), pad_char(']'))
            .map(|choices| {
                if choices.len() == 1 && choices[0].0.is_none() {
                    choices.into_iter().next().unwrap().1 // Plain Sequence
                } else {
                    Node::RandomChoice(
                        choices
                            .into_iter()
                            .map(|(w, node)| (w.unwrap_or(1) as u32, node))
                            .collect(),
                    )
                }
            });

        let shuf_group = kw("shuf")
            .ignore_then(
                expr.clone()
                    .padded_by(pad_expr.clone())
                    .repeated()
                    .delimited_by(pad_char('['), pad_char(']')),
            )
            .map(Node::ShuffledSequence);

        // FIX: Unambiguous < > syntax for Alternator
        let alt_group = expr
            .clone()
            .padded_by(pad_expr.clone())
            .repeated()
            .delimited_by(pad_char('<'), pad_char('>'))
            .map(Node::Alternator);

        let parallel_layer = expr.clone().padded_by(pad_expr.clone()).repeated();

        // FIX: Strict {| |} for Parallel removes `{ }` Polymeter clash completely
        let parallel_group = parallel_layer
            .clone()
            .separated_by(pad_char('|'))
            .delimited_by(just("{|").padded_by(padding()), just("|}").padded_by(padding()))
            .map(Node::Parallel);

        // FIX: Regular { , } reserved safely for Polymeter
        let polymeter_group = parallel_layer
            .separated_by(pad_char(','))
            .delimited_by(pad_char('{'), pad_char('}'))
            .map(Node::Polymeter);

        let arrange_segment = pad_char('(')
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
            .map(|((start, end), node)| (start, end, Box::new(node)));

        let arrange = kw("arrange")
            .ignore_then(
                arrange_segment
                    .clone()
                    .padded_by(padding())
                    .then_ignore(pad_char(',').or_not())
                    .repeated()
                    .delimited_by(pad_char('['), pad_char(']')),
            )
            .map(|segments| Node::Arrange(segments));

        let chain_segment = text::int::<char, Simple<char>>(10)
            .try_map(|s, span| {
                s.parse::<usize>()
                    .map_err(|e| Simple::custom(span, format!("Invalid duration: {}", e)))
            })
            .then_ignore(pad_char(':'))
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
            );

        let chain_loop = kw("chain")
            .ignore_then(
                chain_segment
                    .padded_by(padding())
                    .then_ignore(pad_char(',').or_not())
                    .repeated()
                    .delimited_by(pad_char('['), pad_char(']')),
            )
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

        let struct_group = kw("struct")
            .ignore_then(pad_char('('))
            .ignore_then(expr.clone())
            .then_ignore(pad_char(','))
            .then(expr.clone())
            .then_ignore(pad_char(')'))
            .map(|(mask, content)| Node::Struct(Box::new(mask), Box::new(content)));

        let euclid_group = just('E')
            .ignore_then(pad_char('('))
            .ignore_then(int_u8())
            .then_ignore(pad_char(','))
            .then(int_u8())
            .then_ignore(pad_char(')'))
            .map(|(p, s)| {
                Node::Euclidean(
                    Box::new(Node::Note {
                        pitch: Pitch::Numeric(0, 0),
                        velocity: 100,
                        gate: 100,
                    }),
                    p,
                    s,
                )
            });

        let atom = choice((
            rest,
            hold,
            alias_ref,
            with_scale,
            bracket_group,
            shuf_group,
            alt_group,
            struct_group,
            euclid_group,
            choice((
                parallel_group,
                polymeter_group,
                chain_loop,
                arrange,
                cc_parser(),
                chord_or_note(),
            )),
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
