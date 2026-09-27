use super::primitives::{int_u8, int_usize, kw, pad_char, padding};
use crate::ast::Node;
use chumsky::prelude::*;

pub fn with_scale<'a>(
    expr: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    kw("scale")
        .ignore_then(pad_char('('))
        .ignore_then(super::directives::scale_def())
        .then_ignore(pad_char(')'))
        .then(expr)
        .map(|(scale, child)| Node::WithScale(scale, Box::new(child)))
}

pub fn subdivision_group<'a>(
    expr: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    pad_char('(')
        .ignore_then(expr.padded_by(padding()).repeated())
        .then_ignore(pad_char(')'))
        .map(|mut seq| if seq.len() == 1 { seq.remove(0) } else { Node::Sequence(seq) })
}

pub fn cycle_block<'a>(
    expr: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    pad_char('[')
        .ignore_then(expr.padded_by(padding()).repeated())
        .then_ignore(pad_char(']'))
        .map(|mut seq| if seq.len() == 1 { seq.remove(0) } else { Node::Sequence(seq) })
        .then(
            pad_char('*').ignore_then(int_usize().padded_by(padding())).or_not()
        )
        .map(|(node, multiplier)| {
            let count = multiplier.unwrap_or(1);
            if count == 1 {
                node
            } else {
                Node::Macro(vec![node; count])
            }
        })
}

pub fn seq_group<'a>(
    expr: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    kw("seq")
        .ignore_then(pad_char('('))
        .ignore_then(expr.padded_by(padding()).repeated())
        .then_ignore(pad_char(')'))
        .map(|mut seq| if seq.len() == 1 { seq.remove(0) } else { Node::Sequence(seq) })
}

pub fn alt_group<'a>(
    expr: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    kw("alt")
        .ignore_then(pad_char('('))
        .ignore_then(expr.padded_by(padding()).repeated())
        .then_ignore(pad_char(')'))
        .map(Node::Alternator)
}

pub fn rnd_group<'a>(
    expr: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    let rnd_branch = int_u8()
        .padded_by(padding())
        .then_ignore(pad_char(':'))
        .or_not()
        .then(expr.padded_by(padding()).repeated().map(|mut seq| {
            if seq.len() == 1 { seq.remove(0) } else { Node::Sequence(seq) }
        }));

    kw("rnd")
        .ignore_then(pad_char('('))
        .ignore_then(rnd_branch.separated_by(pad_char(',')))
        .then_ignore(pad_char(')'))
        .map(|choices| {
            Node::RandomChoice(choices.into_iter().map(|(w, n)| (w.unwrap_or(1) as u32, n)).collect())
        })
}

pub fn par_group<'a>(
    expr: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    let layer = expr.padded_by(padding()).repeated().at_least(1).map(|mut seq| {
        if seq.len() == 1 { seq.remove(0) } else { Node::Macro(seq) }
    });

    kw("par")
        .ignore_then(pad_char('('))
        .ignore_then(layer.separated_by(pad_char(',')))
        .then_ignore(pad_char(')'))
        .map(|layers| Node::Parallel(layers.into_iter().map(|l| vec![l]).collect()))
}

pub fn poly_group<'a>(
    expr: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    let layer = expr.padded_by(padding()).repeated().at_least(1).map(|mut seq| {
        if seq.len() == 1 { seq.remove(0) } else { Node::Macro(seq) }
    });

    kw("poly")
        .ignore_then(pad_char('('))
        .ignore_then(layer.separated_by(pad_char(',')))
        .then_ignore(pad_char(')'))
        .map(|layers| Node::Polymeter(layers.into_iter().map(|l| vec![l]).collect()))
}

pub fn shuf_group<'a>(
    expr: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    kw("shuf")
        .ignore_then(pad_char('('))
        .ignore_then(expr.padded_by(padding()).repeated())
        .then_ignore(pad_char(')'))
        .map(Node::ShuffledSequence)
}

pub fn struct_group<'a>(
    expr: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    kw("struct")
        .ignore_then(pad_char('('))
        .ignore_then(expr.clone())
        .then_ignore(pad_char(','))
        .then(expr)
        .then_ignore(pad_char(')'))
        .map(|(mask, content)| Node::Struct(Box::new(mask), Box::new(content)))
}
