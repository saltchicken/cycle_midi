use super::directives::global_directives;
use super::primitives::{pad_char, padding, kw};
use super::track::track_parser;
use crate::ast::{Node, Program, MacroDef};
use chumsky::prelude::*;
use std::collections::HashMap;

enum TopLevelItem {
    Alias(String, Vec<String>, Node),
    Track(crate::ast::Track),
}

pub fn mmn_parser() -> impl Parser<char, Program, Error = Simple<char>> {
    let expr = recursive(move |expr| {
        let rest = just('.').to(Node::Rest);
        let hold = just('_').to(Node::Hold); 

        // Macro/Variable invocation - specifically rejects 'let' and Track headers 'T1'-'T16'
        let alias_ref = text::ident()
            .try_map(|ident: String, span| {
                if ident == "let" {
                    Err(Simple::custom(span, "'let' is a reserved keyword"))
                } else if ident.starts_with('T') && ident[1..].chars().all(|c| c.is_ascii_digit()) && ident.len() > 1 {
                    Err(Simple::custom(span, "Track headers cannot be used as macros"))
                } else {
                    Ok(ident)
                }
            })
            .then(
                expr.clone()
                    .padded_by(padding())
                    .separated_by(pad_char(','))
                    .delimited_by(pad_char('('), pad_char(')'))
                    .or_not()
            )
            .map(|(name, args)| Node::Ref(name, args.unwrap_or_default()));

        let atom = choice((
            rest,
            hold,
            super::combinators::with_scale(expr.clone()),
            super::combinators::implicit_seq(expr.clone()),
            super::combinators::seq_group(expr.clone()),
            super::combinators::alt_group(expr.clone()),
            super::combinators::rnd_group(expr.clone()),
            super::combinators::par_group(expr.clone()),
            super::combinators::poly_group(expr.clone()),
            super::combinators::shuf_group(expr.clone()),
            super::combinators::struct_group(expr.clone()),
            super::combinators::chain_loop(expr.clone()),
            super::combinators::arrange(expr.clone()),
            super::base::cc_parser(),
            super::base::chord_or_note(),
            alias_ref, // Fallback for standard identifiers
        ));

        atom.then(super::modifiers::postfix_parser().repeated())
            .map(|(base, postfixes)| postfixes.into_iter().fold(base, super::modifiers::apply_postfix))
    });

    let alias_def = kw("let")
        .ignore_then(
            text::ident().try_map(|ident: String, span| {
                if ident == "let" {
                    Err(Simple::custom(span, "Cannot name a macro 'let'"))
                } else if ident.starts_with('T') && ident[1..].chars().all(|c| c.is_ascii_digit()) && ident.len() > 1 {
                    Err(Simple::custom(span, "Cannot name a macro after a track"))
                } else {
                    Ok(ident)
                }
            })
        )
        .then(
            text::ident()
                .padded_by(padding())
                .separated_by(pad_char(','))
                .delimited_by(pad_char('('), pad_char(')'))
                .or_not()
        )
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
        .map(|((name, params), node)| TopLevelItem::Alias(name, params.unwrap_or_default(), node));

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
                        TopLevelItem::Alias(name, params, node) => {
                            aliases.insert(name, MacroDef { params, body: node });
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
