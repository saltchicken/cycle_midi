pub mod basic;
pub mod combinators;
pub mod helpers;
pub mod modifiers;

use crate::ast::Node;
use crate::engine::render::{RenderContext, ScheduledEvent};

use basic::*;
use combinators::*;
use modifiers::*;

pub fn traverse_ast(
    node: &Node,
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
) {
    if out_events.len() >= ctx.max_events { return; }

    match node {
        Node::Note { pitch, velocity, gate } => render_note(pitch, *velocity, *gate, ctx, out_events),
        Node::CC { controller, value } => render_cc(*controller, value, ctx, out_events),
        Node::Rest => { ctx.active_chord_indices.clear(); },
        Node::Hold => render_hold(ctx, out_events),
        Node::Ref(_) => { ctx.active_chord_indices.clear(); },
        Node::Chord(elements) => render_chord(elements, ctx, out_events),
        Node::Sequence(elements) => render_sequence(elements, ctx, out_events),
        Node::Macro(elements) => render_macro(elements, ctx, out_events),
        Node::Parallel(layers) => render_parallel(layers, ctx, out_events),
        Node::Repeat(child, count) => render_repeat(child, *count, ctx, out_events),
        Node::Modified(child, modifiers) => render_modified(child, modifiers, ctx, out_events),
        Node::MidiImport(_) => { /* Ignored. Resolved upstream in the watcher */ }
    }
}
