pub mod basic;
pub mod combinators;
pub mod helpers;
pub mod modifiers;

use crate::ast::Node;
use crate::engine::render::{RenderContext, ScheduledEvent};
use rand::rngs::StdRng;

use basic::*;
use combinators::*;
use modifiers::*;

// ---------------------------------------------------------
// TRAVERSAL ROOT
// ---------------------------------------------------------

pub fn traverse_ast(
    node: &Node,
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
    rng: &mut StdRng,
) {
    match node {
        Node::Note { pitch, velocity, gate } => render_note(pitch, *velocity, *gate, ctx, out_events, rng),
        Node::CC { controller, value } => render_cc(*controller, value, ctx, out_events, rng),
        Node::Rest => { ctx.active_chord_indices.clear(); },
        Node::Hold => render_hold(ctx, out_events),
        Node::Ref(_) => { ctx.active_chord_indices.clear(); },
        Node::Chord(elements) => render_chord(elements, ctx, out_events, rng),
        Node::Sequence(elements) => render_sequence(elements, ctx, out_events, rng),
        Node::ShuffledSequence(elements) => render_shuffled_sequence(elements, ctx, out_events, rng),
        Node::Parallel(layers) => render_parallel(layers, ctx, out_events, rng),
        Node::Polymeter(layers) => render_polymeter(layers, ctx, out_events, rng),
        Node::Arrange(segments) => render_arrange(segments, ctx, out_events, rng),
        Node::Alternator(elements) => render_alternator(elements, ctx, out_events, rng),
        Node::RandomChoice(elements) => render_random_choice(elements, ctx, out_events, rng),
        Node::WithScale(scale, child) => render_with_scale(scale, child, ctx, out_events, rng),
        Node::Struct(structure, content) => render_struct(structure, content, ctx, out_events, rng),
        Node::Modified(child, modifiers) => render_modified(child, modifiers, ctx, out_events, rng),
    }
}
