use super::helpers::render_phase_chunks;
use super::traverse_ast;
use crate::ast::{Node, ScaleDef};
use crate::engine::render::{RenderContext, ScheduledEvent};
use rand::RngExt;
use rand::distr::Distribution;
use rand::distr::weighted::WeightedIndex;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;

pub(super) fn render_chord(
    elements: &[Node],
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
    rng: &mut StdRng,
) {
    let mut chord_indices = Vec::new();
    let orig_indices = ctx.active_chord_indices.clone();
    for el in elements {
        ctx.active_chord_indices = orig_indices.clone();
        traverse_ast(el, ctx, out_events, rng);
        chord_indices.extend_from_slice(&ctx.active_chord_indices);
    }
    ctx.active_chord_indices = chord_indices;
}

pub(super) fn render_sequence(
    elements: &[Node],
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
    rng: &mut StdRng,
) {
    if elements.is_empty() {
        ctx.active_chord_indices.clear();
        return;
    }
    let step_duration = ctx.duration_ms / elements.len() as f64;
    for (i, el) in elements.iter().enumerate() {
        let mut sub_ctx = ctx.clone();
        sub_ctx.start_ms = ctx.start_ms + (i as f64 * step_duration);
        sub_ctx.duration_ms = step_duration;
        sub_ctx.window_start_ms = ctx.window_start_ms.max(sub_ctx.start_ms);
        sub_ctx.window_end_ms = ctx.window_end_ms.min(sub_ctx.start_ms + step_duration);
        traverse_ast(el, &mut sub_ctx, out_events, rng);
        ctx.active_chord_indices = sub_ctx.active_chord_indices;
    }
}

pub(super) fn render_shuffled_sequence(
    elements: &[Node],
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
    rng: &mut StdRng,
) {
    if elements.is_empty() {
        ctx.active_chord_indices.clear();
        return;
    }
    let mut shuffled = elements.to_vec();
    shuffled.shuffle(rng);
    render_sequence(&shuffled, ctx, out_events, rng);
}

pub(super) fn render_parallel(
    layers: &[Vec<Node>],
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
    rng: &mut StdRng,
) {
    let orig_indices = ctx.active_chord_indices.clone();
    let mut all_indices = Vec::new();

    for layer in layers {
        let mut sub_ctx = ctx.clone();
        sub_ctx.active_chord_indices = orig_indices.clone();

        if layer.is_empty() {
            sub_ctx.active_chord_indices.clear();
        } else {
            let step_duration = sub_ctx.duration_ms / layer.len() as f64;
            for (i, el) in layer.iter().enumerate() {
                let mut step_ctx = sub_ctx.clone();
                step_ctx.start_ms = ctx.start_ms + (i as f64 * step_duration);
                step_ctx.duration_ms = step_duration;
                step_ctx.window_start_ms = ctx.window_start_ms.max(step_ctx.start_ms);
                step_ctx.window_end_ms = ctx.window_end_ms.min(step_ctx.start_ms + step_duration);

                traverse_ast(el, &mut step_ctx, out_events, rng);
                sub_ctx.active_chord_indices = step_ctx.active_chord_indices;
            }
        }
        all_indices.extend_from_slice(&sub_ctx.active_chord_indices);
    }
    ctx.active_chord_indices = all_indices;
}

pub(super) fn render_polymeter(
    layers: &[Vec<Node>],
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
    rng: &mut StdRng,
) {
    let orig_indices = ctx.active_chord_indices.clone();
    let mut all_indices = Vec::new();

    if layers.is_empty() {
        ctx.active_chord_indices.clear();
        return;
    }

    let l0 = layers[0].len().max(1) as f64;
    for layer in layers {
        let mut sub_ctx = ctx.clone();
        sub_ctx.active_chord_indices = orig_indices.clone();

        if !layer.is_empty() {
            let li = layer.len() as f64;
            let speed = l0 / li;
            let local_duration = sub_ctx.duration_ms / speed;

            let layer_indices = render_phase_chunks(
                &sub_ctx,
                local_duration,
                0.0,
                Some(local_duration),
                |chunk_ctx| {
                    let step_duration = local_duration / li;
                    for (step_idx, el) in layer.iter().enumerate() {
                        let mut step_ctx = chunk_ctx.clone();
                        step_ctx.start_ms = chunk_ctx.start_ms + (step_idx as f64 * step_duration);
                        step_ctx.duration_ms = step_duration;
                        step_ctx.window_start_ms = chunk_ctx.window_start_ms.max(step_ctx.start_ms);
                        step_ctx.window_end_ms = chunk_ctx.window_end_ms.min(step_ctx.start_ms + step_duration);

                        traverse_ast(el, &mut step_ctx, out_events, rng);
                        chunk_ctx.active_chord_indices = step_ctx.active_chord_indices;
                    }
                },
            );
            sub_ctx.active_chord_indices = layer_indices;
        }
        all_indices.extend_from_slice(&sub_ctx.active_chord_indices);
    }
    ctx.active_chord_indices = all_indices;
}

pub(super) fn render_arrange(
    segments: &[(usize, usize, Box<Node>)],
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
    rng: &mut StdRng,
) {
    let max_end = segments.iter().map(|s| s.1).max().unwrap_or(1).max(1);
    let current_cycle = ctx.cycle_count % max_end;

    let orig_indices = ctx.active_chord_indices.clone();
    let mut all_indices = Vec::new();

    for (start, end, child) in segments {
        if current_cycle >= *start && current_cycle < *end {
            let mut sub_ctx = ctx.clone();
            sub_ctx.cycle_count = current_cycle;
            sub_ctx.active_chord_indices = orig_indices.clone();
            traverse_ast(child, &mut sub_ctx, out_events, rng);
            all_indices.extend_from_slice(&sub_ctx.active_chord_indices);
        }
    }
    ctx.active_chord_indices = all_indices;
}

pub(super) fn render_alternator(
    elements: &[Node],
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
    rng: &mut StdRng,
) {
    if elements.is_empty() {
        ctx.active_chord_indices.clear();
        return;
    }
    let index = (ctx.cycle_count / ctx.alternator_stride) % elements.len();
    let mut sub_ctx = ctx.clone();
    sub_ctx.alternator_stride *= elements.len();
    traverse_ast(&elements[index], &mut sub_ctx, out_events, rng);
    ctx.active_chord_indices = sub_ctx.active_chord_indices;
}

pub(super) fn render_random_choice(
    elements: &[(u32, Node)],
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
    rng: &mut StdRng,
) {
    if elements.is_empty() {
        ctx.active_chord_indices.clear();
        return;
    }
    let weights: Vec<u32> = elements.iter().map(|(w, _)| *w).collect();
    if let Ok(dist) = WeightedIndex::new(&weights) {
        let index = dist.sample(rng);
        traverse_ast(&elements[index].1, ctx, out_events, rng);
    } else {
        let index = rng.random_range(0..elements.len());
        traverse_ast(&elements[index].1, ctx, out_events, rng);
    }
}

pub(super) fn render_with_scale(
    scale: &ScaleDef,
    child: &Node,
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
    rng: &mut StdRng,
) {
    let mut sub_ctx = ctx.clone();
    sub_ctx.scale = Some(scale.clone());
    traverse_ast(child, &mut sub_ctx, out_events, rng);
    ctx.active_chord_indices = sub_ctx.active_chord_indices;
}

pub(super) fn render_struct(
    structure: &Node,
    content: &Node,
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
    rng: &mut StdRng,
) {
    let mut struct_events = Vec::new();
    traverse_ast(structure, ctx, &mut struct_events, rng);

    if struct_events.is_empty() {
        ctx.active_chord_indices.clear();
        return;
    }

    let mut content_events = Vec::new();
    let mut content_ctx = ctx.clone();
    content_ctx.window_start_ms = f64::MIN;
    content_ctx.window_end_ms = f64::MAX;
    content_ctx.transition_fade = None;
    traverse_ast(content, &mut content_ctx, &mut content_events, rng);

    let mut all_indices = Vec::new();

    for s_ev in struct_events {
        match s_ev {
            ScheduledEvent::Note { start_ms, duration_ms, velocity: s_vel, .. } => {
                let mut matched_notes = Vec::new();
                let mut exact_match_found = false;

                for c_ev in &content_events {
                    if let ScheduledEvent::Note { start_ms: c_start, duration_ms: c_dur, pitch, velocity, channel, .. } = c_ev {
                        if start_ms >= *c_start - 0.1 && start_ms < (*c_start + *c_dur - 0.1) {
                            matched_notes.push((*pitch, *velocity, *channel));
                            exact_match_found = true;
                        }
                    }
                }

                if !exact_match_found {
                    let mut last_start_time = -1.0;
                    for c_ev in &content_events {
                        if let ScheduledEvent::Note { start_ms: c_start, pitch, velocity, channel, .. } = c_ev {
                            if *c_start <= start_ms + 0.1 {
                                if *c_start - last_start_time > 1.0 {
                                    matched_notes.clear();
                                    last_start_time = *c_start;
                                }
                                matched_notes.push((*pitch, *velocity, *channel));
                            }
                        }
                    }
                }

                for (pitch, c_vel, matched_channel) in matched_notes {
                    let mixed_vel = ((s_vel as f32 * c_vel as f32) / 127.0).clamp(1.0, 127.0) as u8;
                    let final_vel = ctx.override_velocity.unwrap_or(mixed_vel);

                    out_events.push(ScheduledEvent::Note {
                        channel: matched_channel,
                        pitch,
                        velocity: final_vel,
                        start_ms,
                        duration_ms,
                    });
                    all_indices.push(out_events.len() - 1);
                }
            }
            cc @ ScheduledEvent::CC { .. } => {
                out_events.push(cc);
            }
        }
    }
    ctx.active_chord_indices = all_indices;
}
