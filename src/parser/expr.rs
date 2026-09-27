use super::directives::global_directives;
use super::primitives::{pad_char, padding, kw};
use super::track::track_parser;
use crate::ast::{Node, Program};
use chumsky::prelude::*;
use std::collections::HashMap;

enum TopLevelItem {
    Alias(String, Node),
    Track(crate::ast::Track),
}

pub fn node_parser() -> impl Parser<char, Node, Error = Simple<char>> + Clone {
    recursive(move |unary| {
        let rest = just('.').to(Node::Rest);
        let hold = just('_').to(Node::Hold); 

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
            .map(Node::Ref);

        let atom = choice((
            rest,
            hold,
            super::combinators::subdivision_group(unary.clone()),
            super::combinators::cycle_block(unary.clone()),
            super::base::cc_parser(),
            super::base::chord_or_note(),
            super::base::midi_import(),
            alias_ref,
        ));

        // Use apply_postfix to correctly map PostfixOp into the AST Modifier type
        atom.then(super::modifiers::postfix_parser().repeated())
            .map(|(base, postfixes)| postfixes.into_iter().fold(base, super::modifiers::apply_postfix))
    })
}

pub fn mmn_parser() -> impl Parser<char, Program, Error = Simple<char>> {
    let unary = node_parser();

    // Top Level space-separated nodes concatenate time (Macro)
    let layer = unary.clone().padded_by(padding()).repeated().at_least(1).map(|mut seq| {
        if seq.len() == 1 { seq.remove(0) } else { Node::Macro(seq) }
    });
    
    // Top Level pipe-separated nodes parallelize time
    let parallel = layer.separated_by(pad_char('|')).at_least(1).map(|mut layers| {
        if layers.len() == 1 { layers.remove(0) } else { Node::Parallel(layers) }
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
        .then_ignore(pad_char('='))
        .then(parallel.clone().padded_by(padding()).or_not())
        .map(|(name, opt_node)| TopLevelItem::Alias(name, opt_node.unwrap_or_else(|| Node::Macro(vec![]))));

    let track_def = track_parser(unary).map(TopLevelItem::Track);

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
