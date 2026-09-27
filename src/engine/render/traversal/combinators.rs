use super::traverse_ast;
use crate::ast::Node;
use crate::engine::render::{RenderContext, ScheduledEvent};

pub(super) fn render_chord(
    elements: &[Node],
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
) {
    if out_events.len() >= ctx.max_events { return; }
    let mut chord_indices = Vec::new();
    let orig_indices = ctx.active_chord_indices.clone();
    for el in elements {
        ctx.active_chord_indices = orig_indices.clone();
        traverse_ast(el, ctx, out_events);
        chord_indices.extend_from_slice(&ctx.active_chord_indices);
    }
    ctx.active_chord_indices = chord_indices;
}

pub(super) fn render_sequence(
    elements: &[Node],
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
) {
    if elements.is_empty() {
        ctx.active_chord_indices.clear();
        return;
    }
    let step_duration = ctx.duration_ms / elements.len() as f64;
    for (i, el) in elements.iter().enumerate() {
        let mut sub_ctx = ctx.derive_step(i, step_duration);
        traverse_ast(el, &mut sub_ctx, out_events);
        ctx.active_chord_indices = sub_ctx.active_chord_indices;
    }
}

pub(super) fn render_macro(
    elements: &[Node],
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
) {
    if elements.is_empty() {
        ctx.active_chord_indices.clear();
        return;
    }
    
    let total_cycles: usize = elements.iter().map(|n| n.cycle_length()).sum();
    if total_cycles == 0 {
        ctx.active_chord_indices.clear();
        return;
    }
    
    let target_cycle = ctx.cycle_count % total_cycles;
    let mut current_cycle_accum = 0;
    
    for el in elements {
        let len = el.cycle_length();
        if target_cycle >= current_cycle_accum && target_cycle < current_cycle_accum + len {
            let mut sub_ctx = ctx.clone();
            sub_ctx.cycle_count = target_cycle - current_cycle_accum;
            traverse_ast(el, &mut sub_ctx, out_events);
            ctx.active_chord_indices = sub_ctx.active_chord_indices;
            break;
        }
        current_cycle_accum += len;
    }
}

pub(super) fn render_parallel(
    layers: &[Node],
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
) {
    let orig_indices = ctx.active_chord_indices.clone();
    let mut all_indices = Vec::new();

    for layer in layers {
        let mut sub_ctx = ctx.clone();
        sub_ctx.active_chord_indices = orig_indices.clone();
        traverse_ast(layer, &mut sub_ctx, out_events);
        all_indices.extend_from_slice(&sub_ctx.active_chord_indices);
    }
    ctx.active_chord_indices = all_indices;
}

pub(super) fn render_repeat(
    child: &Node,
    count: usize,
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
) {
    if count == 0 {
        ctx.active_chord_indices.clear();
        return;
    }
    let total_cycles = child.cycle_length() * count;
    let target_cycle = ctx.cycle_count % total_cycles.max(1);
    let child_len = child.cycle_length().max(1);
    
    let mut sub_ctx = ctx.clone();
    sub_ctx.cycle_count = target_cycle % child_len;
    traverse_ast(child, &mut sub_ctx, out_events);
    ctx.active_chord_indices = sub_ctx.active_chord_indices;
}
