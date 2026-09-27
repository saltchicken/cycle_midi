use super::primitives::{kw, pad_char, padding};
use crate::ast::Node;
use chumsky::prelude::*;

pub fn with_scale<'a>(
    unary: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    kw("scale")
        .ignore_then(pad_char('('))
        .ignore_then(super::directives::scale_def())
        .then_ignore(pad_char(')'))
        .then(unary)
        .map(|(scale, child)| Node::WithScale(scale, Box::new(child)))
}

pub fn subdivision_group<'a>(
    unary: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    let layer = unary.padded_by(padding()).repeated().at_least(1).map(|mut seq| {
        if seq.len() == 1 { seq.remove(0) } else { Node::Sequence(seq) }
    });
    let parallel = layer.separated_by(pad_char('|')).at_least(1).map(|mut layers| {
        if layers.len() == 1 { layers.remove(0) } else { Node::Parallel(layers.into_iter().map(|l| vec![l]).collect()) }
    });

    pad_char('(')
        .ignore_then(parallel.clone().padded_by(padding()).or_not())
        .then_ignore(pad_char(')'))
        .map(|opt_node| opt_node.unwrap_or_else(|| Node::Sequence(vec![])))
}

pub fn cycle_block<'a>(
    unary: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    let layer = unary.padded_by(padding()).repeated().at_least(1).map(|mut seq| {
        if seq.len() == 1 { seq.remove(0) } else { Node::Sequence(seq) }
    });
    let parallel = layer.separated_by(pad_char('|')).at_least(1).map(|mut layers| {
        if layers.len() == 1 { layers.remove(0) } else { Node::Parallel(layers.into_iter().map(|l| vec![l]).collect()) }
    });

    pad_char('[')
        .ignore_then(parallel.clone().padded_by(padding()).or_not())
        .then_ignore(pad_char(']'))
        .then(
            pad_char('*').ignore_then(super::primitives::int_usize().padded_by(padding())).or_not()
        )
        .map(|(opt_node, multiplier)| {
            let node = opt_node.unwrap_or_else(|| Node::Sequence(vec![]));
            let count = multiplier.unwrap_or(1);
            if count == 1 { node } else { Node::Macro(vec![node; count]) }
        })
}

pub fn seq_group<'a>(
    unary: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    let layer = unary.padded_by(padding()).repeated().at_least(1).map(|mut seq| {
        if seq.len() == 1 { seq.remove(0) } else { Node::Sequence(seq) }
    });
    let parallel = layer.separated_by(pad_char('|')).at_least(1).map(|mut layers| {
        if layers.len() == 1 { layers.remove(0) } else { Node::Parallel(layers.into_iter().map(|l| vec![l]).collect()) }
    });

    kw("seq")
        .ignore_then(pad_char('('))
        .ignore_then(parallel.clone().padded_by(padding()).or_not())
        .then_ignore(pad_char(')'))
        .map(|opt_node| opt_node.unwrap_or_else(|| Node::Sequence(vec![])))
}

pub fn alt_group<'a>(
    unary: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    kw("alt")
        .ignore_then(pad_char('('))
        .ignore_then(unary.padded_by(padding()).repeated())
        .then_ignore(pad_char(')'))
        .map(Node::Alternator)
}

pub fn rnd_group<'a>(
    unary: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    let layer = unary.padded_by(padding()).repeated().at_least(1).map(|mut seq| {
        if seq.len() == 1 { seq.remove(0) } else { Node::Sequence(seq) }
    });
    let parallel = layer.separated_by(pad_char('|')).at_least(1).map(|mut layers| {
        if layers.len() == 1 { layers.remove(0) } else { Node::Parallel(layers.into_iter().map(|l| vec![l]).collect()) }
    });

    let rnd_branch = super::primitives::int_u8()
        .padded_by(padding())
        .then_ignore(pad_char(':'))
        .or_not()
        .then(parallel.clone().padded_by(padding()).or_not());

    kw("rnd")
        .ignore_then(pad_char('('))
        .ignore_then(rnd_branch.separated_by(pad_char(',')))
        .then_ignore(pad_char(')'))
        .map(|choices| {
            Node::RandomChoice(choices.into_iter().map(|(w, opt_n)| (w.unwrap_or(1) as u32, opt_n.unwrap_or_else(|| Node::Sequence(vec![])))).collect())
        })
}

pub fn poly_group<'a>(
    unary: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    let layer = unary.padded_by(padding()).repeated().at_least(1).map(|mut seq| {
        if seq.len() == 1 { seq.remove(0) } else { Node::Macro(seq) }
    });

    kw("poly")
        .ignore_then(pad_char('('))
        .ignore_then(layer.separated_by(pad_char(',')).or_not())
        .then_ignore(pad_char(')'))
        .map(|opt_layers| Node::Polymeter(opt_layers.unwrap_or_default().into_iter().map(|l| vec![l]).collect()))
}

pub fn shuf_group<'a>(
    unary: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    kw("shuf")
        .ignore_then(pad_char('('))
        .ignore_then(unary.padded_by(padding()).repeated())
        .then_ignore(pad_char(')'))
        .map(Node::ShuffledSequence)
}

pub fn struct_group<'a>(
    unary: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    kw("struct")
        .ignore_then(pad_char('('))
        .ignore_then(unary.clone())
        .then_ignore(pad_char(','))
        .then(unary)
        .then_ignore(pad_char(')'))
        .map(|(mask, content)| Node::Struct(Box::new(mask), Box::new(content)))
}
