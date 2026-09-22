use super::directives::global_directives;
use super::primitives::{pad_char, padding};
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

        let alias_ref = just('$')
            .ignore_then(text::ident())
            .then(
                expr.clone()
                    .padded_by(padding())
                    .separated_by(pad_char(','))
                    .delimited_by(pad_char('('), pad_char(')'))
                    .or_not()
            )
            .then_ignore(
                padding()
                    .ignore_then(just('='))
                    .not()
                    .rewind()
                    .ignored()
                    .or(end())
            )
            .map(|(name, args)| Node::Ref(name, args.unwrap_or_default()));

        let atom = choice((
            rest,
            hold,
            alias_ref,
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
        ));

        atom.then(super::modifiers::postfix_parser().repeated())
            .map(|(base, postfixes)| postfixes.into_iter().fold(base, super::modifiers::apply_postfix))
    });

    let alias_def = just('$')
        .ignore_then(text::ident())
        .then(
            just('$')
                .ignore_then(text::ident())
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
