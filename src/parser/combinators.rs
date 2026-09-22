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

pub fn implicit_seq<'a>(
    expr: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    pad_char('[')
        .ignore_then(expr.padded_by(padding()).repeated())
        .then_ignore(pad_char(']'))
        .map(|mut seq| if seq.len() == 1 { seq.remove(0) } else { Node::Sequence(seq) })
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
    kw("par")
        .ignore_then(pad_char('('))
        .ignore_then(expr.padded_by(padding()).repeated().separated_by(pad_char(',')))
        .then_ignore(pad_char(')'))
        .map(Node::Parallel)
}

pub fn poly_group<'a>(
    expr: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    kw("poly")
        .ignore_then(pad_char('('))
        .ignore_then(expr.padded_by(padding()).repeated().separated_by(pad_char(',')))
        .then_ignore(pad_char(')'))
        .map(Node::Polymeter)
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

pub fn arrange<'a>(
    expr: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    let arrange_segment = pad_char('(')
        .ignore_then(int_usize())
        .then_ignore(pad_char(','))
        .then(int_usize())
        .then_ignore(pad_char(')'))
        .then_ignore(pad_char(':'))
        .then(
            expr.padded_by(padding())
                .repeated()
                .at_least(1)
                .map(|mut seq| if seq.len() == 1 { seq.remove(0) } else { Node::Sequence(seq) }),
        )
        .map(|((start, end), node)| (start, end, Box::new(node)));

    kw("arrange")
        .ignore_then(
            arrange_segment
                .padded_by(padding())
                .then_ignore(pad_char(',').or_not())
                .repeated()
                .delimited_by(pad_char('['), pad_char(']')),
        )
        .map(|segments| Node::Arrange(segments))
}

pub fn chain_loop<'a>(
    expr: impl Parser<char, Node, Error = Simple<char>> + Clone + 'a,
) -> impl Parser<char, Node, Error = Simple<char>> + Clone + 'a {
    let chain_segment = int_usize().then_ignore(pad_char(':')).then(
        expr.padded_by(padding())
            .repeated()
            .at_least(1)
            .map(|mut seq| if seq.len() == 1 { seq.remove(0) } else { Node::Sequence(seq) }),
    );

    kw("chain")
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
        })
}
