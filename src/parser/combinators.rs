use super::primitives::{int_usize, pad_char, padding};
use crate::ast::Node;
use chumsky::prelude::*;

pub fn subdivision_group<'a>(
    unary: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    
    // Inside brackets, space-separated nodes subdivide time (Sequence)
    let layer = unary.padded_by(padding()).repeated().at_least(1).map(|mut seq| {
        if seq.len() == 1 { seq.remove(0) } else { Node::Sequence(seq) }
    });
    
    // Inside brackets, pipe-separated nodes parallelize time
    let parallel = layer.separated_by(pad_char('|')).at_least(1).map(|mut layers| {
        if layers.len() == 1 { layers.remove(0) } else { Node::Parallel(layers) }
    });

    pad_char('(')
        .ignore_then(parallel.clone().padded_by(padding()).or_not())
        .then_ignore(pad_char(')'))
        .map(|opt_node| opt_node.unwrap_or_else(|| Node::Sequence(vec![])))
}

pub fn cycle_block<'a>(
    unary: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    
    // Inside brackets, space-separated nodes subdivide time (Sequence)
    let layer = unary.padded_by(padding()).repeated().at_least(1).map(|mut seq| {
        if seq.len() == 1 { seq.remove(0) } else { Node::Sequence(seq) }
    });
    
    // Inside brackets, pipe-separated nodes parallelize time
    let parallel = layer.separated_by(pad_char('|')).at_least(1).map(|mut layers| {
        if layers.len() == 1 { layers.remove(0) } else { Node::Parallel(layers) }
    });

    pad_char('[')
        .ignore_then(parallel.clone().padded_by(padding()).or_not())
        .then_ignore(pad_char(']'))
        .then(pad_char('*').ignore_then(int_usize().padded_by(padding())).or_not())
        .map(|(opt_node, multiplier)| {
            let node = opt_node.unwrap_or_else(|| Node::Sequence(vec![]));
            let count = multiplier.unwrap_or(1);
            if count == 1 { node } else { Node::Repeat(Box::new(node), count) }
        })
}
