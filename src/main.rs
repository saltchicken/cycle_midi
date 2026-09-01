use chumsky::prelude::*;

// 1. The Abstract Syntax Tree from your Design Doc
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Note { pitch: u8, velocity: u8, gate: u8, prob: u8 },
    Chord(Vec<Node>),
    Rest,
    Hold,
    Sequence(Vec<Node>),
    Parallel(Vec<Vec<Node>>),
    Euclidean(Box<Node>, u8, u8),
    Alternator(Vec<Node>),
    SpeedModifier(Box<Node>, f32),
}

// 2. The Chumsky Parser
pub fn mmn_parser() -> impl Parser<char, Node, Error = Simple<char>> {
    recursive(|expr| {
        // --- Primitives ---
        let rest = just('.').to(Node::Rest);
        let hold = just('_').to(Node::Hold);

        // Safe integer and float parsers
        let int_u8 = text::int::<char, Simple<char>>(10)
            .map(|s: String| s.parse::<u8>().unwrap());
        
        let float = text::int(10)
            .chain::<char, _, _>(just('.').chain(text::digits(10)).or_not().flatten())
            .collect::<String>()
            .map(|s| s.parse::<f32>().unwrap());

        // --- Pitch Parsing ---
        // Parses note names (C4, D#3) and converts to MIDI integer
        let note_name = choice((
            just("C#"), just("Db"), just("D#"), just("Eb"),
            just("F#"), just("Gb"), just("G#"), just("Ab"),
            just("A#"), just("Bb"),
            just("C"), just("D"), just("E"), just("F"),
            just("G"), just("A"), just("B")
        ));

        let pitch_str = note_name
            .then(text::int(10).map(|s: String| s.parse::<i32>().unwrap()))
            .map(|(n, oct)| {
                let base = match n {
                    "C" => 0, "C#" | "Db" => 1, "D" => 2, "D#" | "Eb" => 3,
                    "E" => 4, "F" => 5, "F#" | "Gb" => 6, "G" => 7,
                    "G#" | "Ab" => 8, "A" => 9, "A#" | "Bb" => 10, "B" => 11,
                    _ => 0,
                };
                ((oct + 1) * 12 + base).clamp(0, 127) as u8
            });

        // A pitch can be either a string (C4) or raw integer (60)
        let pitch = pitch_str.or(int_u8);

        // --- Inline Modifiers ---
        let velocity = just('@').ignore_then(int_u8);
        let gate = just('%').ignore_then(int_u8);
        let prob = just('?').ignore_then(int_u8);

        // --- Chords & Notes ---
        // Handles "C4", or "C4+E4", applying modifiers to the underlying Notes
        let chord_or_note = pitch
            .separated_by(just('+'))
            .at_least(1)
            .then(velocity.or_not())
            .then(gate.or_not())
            .then(prob.or_not())
            .map(|(((pitches, v), g), pr)| {
                let notes: Vec<Node> = pitches.into_iter().map(|p| Node::Note {
                    pitch: p,
                    velocity: v.unwrap_or(100),
                    gate: g.unwrap_or(100),
                    prob: pr.unwrap_or(100),
                }).collect();
                
                if notes.len() == 1 {
                    notes.into_iter().next().unwrap()
                } else {
                    Node::Chord(notes)
                }
            });

        // --- Groupings ---
        let seq_group = expr.clone()
            .padded()
            .repeated()
            .delimited_by(just('['), just(']'))
            .map(Node::Sequence);

        let alt_group = expr.clone()
            .padded()
            .repeated()
            .delimited_by(just('<'), just('>'))
            .map(Node::Alternator);

        let parallel_layer = expr.clone()
            .padded()
            .repeated();

        let parallel_group = parallel_layer
            .separated_by(just('|'))
            .delimited_by(just('{'), just('}'))
            .map(Node::Parallel);

        // An atom is any base element before algorithmic postfixes are applied
        let atom = choice((
            rest,
            hold,
            seq_group,
            alt_group,
            parallel_group,
            chord_or_note,
        ));

        // --- Algorithmic Postfixes ---
        enum Postfix {
            Euclidean(u8, u8),
            Mul(f32),
            Div(f32),
        }

        let euclidean = just('(')
            .ignore_then(int_u8)
            .then_ignore(just(','))
            .then(int_u8)
            .then_ignore(just(')'))
            .map(|(p, s)| Postfix::Euclidean(p, s));

        let speed_mul = just('*').ignore_then(float).map(Postfix::Mul);
        let speed_div = just('/').ignore_then(float).map(Postfix::Div);

        let postfix = choice((euclidean, speed_mul, speed_div));

        // Fold postfixes over the atom. This allows chaining, e.g., `C4(3,8)*2`
        atom.then(postfix.repeated()).map(|(base, postfixes)| {
            postfixes.into_iter().fold(base, |acc, post| match post {
                Postfix::Euclidean(p, s) => Node::Euclidean(Box::new(acc), p, s),
                Postfix::Mul(val) => Node::SpeedModifier(Box::new(acc), val),
                Postfix::Div(val) => Node::SpeedModifier(Box::new(acc), 1.0 / val),
            })
        })
    })
    // 3. The Root parser treats the entire file as an implicit Sequence
    .padded()
    .repeated()
    .map(Node::Sequence)
    .then_ignore(end())
}

fn main() {
    // A complex test pattern demonstrating all features of the design doc
    let notation = r#"
        { C3 [. C3] . C3*2 | 64(3,8)@80 } 
        <C4+E4+G4@80%50?75 .>
    "#;

    println!("Parsing Notation:\n{}\n", notation.trim());

    match mmn_parser().parse(notation) {
        Ok(ast) => println!("AST Output:\n{:#?}", ast),
        Err(errs) => {
            for e in errs {
                eprintln!("Parse error at character {}: expected {:?}", e.span().start, e.expected());
            }
        }
    }
}
