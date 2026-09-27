use super::helpers::{render_phase_chunks, get_positional_rng};
use super::traverse_ast;
use crate::ast::{ArpStyle, Modifier, Node};
use crate::engine::render::{RenderContext, ScheduledEvent};
use rand::RngExt;

macro_rules! with_ctx {
    ($ctx:expr, $child:expr, $rest:expr, $out_events:expr, $override_fn:expr) => {{
        let mut sub_ctx = $ctx.clone();
        $override_fn(&mut sub_ctx);
        render_modified($child, $rest, &mut sub_ctx, $out_events);
        $ctx.active_chord_indices = sub_ctx.active_chord_indices;
    }};
}

pub(super) fn render_modified(
    child: &Node,
    modifiers: &[Modifier],
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
) {
    if out_events.len() >= ctx.max_events { return; }
    if modifiers.is_empty() {
        traverse_ast(child, ctx, out_events);
        return;
    }

    let (last_mod, rest) = modifiers.split_last().unwrap();

    match last_mod {
        Modifier::Velocity(vel) => with_ctx!(ctx, child, rest, out_events, |c: &mut RenderContext| c.override_velocity = Some(*vel)),
        Modifier::Gate(g) => with_ctx!(ctx, child, rest, out_events, |c: &mut RenderContext| c.override_gate = Some(*g)),
        Modifier::Humanize(vel, time) => with_ctx!(ctx, child, rest, out_events, |c: &mut RenderContext| {
            c.humanize_velocity_range = *vel;
            c.humanize_timing_range_ms = time.abs();
        }),
        Modifier::Probability(prob) => {
            let mut rng = get_positional_rng(ctx);
            if *prob < 100 && rng.random_range(0..100) >= *prob {
                ctx.active_chord_indices.clear();
            } else {
                render_modified(child, rest, ctx, out_events);
            }
        }
        Modifier::Span(span_cycles) => {
            let local_duration = (*span_cycles).max(1) as f64 * ctx.master_duration_ms;
            ctx.active_chord_indices = render_phase_chunks(
                ctx, local_duration, 0.0, Some(local_duration),
                |chunk_ctx| { render_modified(child, rest, chunk_ctx, out_events); }
            );
        }
        Modifier::Speed(factor) => {
            if *factor <= 0.0 { return; }
            let local_duration = ctx.duration_ms / *factor;
            let local_master = ctx.master_duration_ms / *factor;
            ctx.active_chord_indices = render_phase_chunks(
                ctx, local_duration, 0.0, Some(local_master),
                |chunk_ctx| { render_modified(child, rest, chunk_ctx, out_events); }
            );
        }
        Modifier::Euclidean(pulses, steps) => {
            if *steps == 0 || *pulses == 0 {
                ctx.active_chord_indices.clear();
                return;
            }
            let step_duration = ctx.duration_ms / *steps as f64;
            for i in 0..*steps {
                let is_hit = ((i as usize * *pulses as usize) % (*steps as usize)) < (*pulses as usize);
                if is_hit {
                    let mut sub_ctx = ctx.derive_step(i as usize, step_duration);
                    render_modified(child, rest, &mut sub_ctx, out_events);
                    ctx.active_chord_indices = sub_ctx.active_chord_indices;
                } else {
                    ctx.active_chord_indices.clear();
                }
            }
        }
        Modifier::Stut(depth, feedback, shift_amount) => {
            let orig_indices = ctx.active_chord_indices.clone();
            let mut all_indices = Vec::new();
            for i in 0..=*depth {
                let mut sub_ctx = ctx.clone();
                sub_ctx.active_chord_indices = orig_indices.clone();
                sub_ctx.velocity_modifier *= feedback.powi(i as i32);
                if i == 0 {
                    render_modified(child, rest, &mut sub_ctx, out_events);
                    all_indices.extend_from_slice(&sub_ctx.active_chord_indices);
                } else {
                    let shift_ms = (*shift_amount * i as f32) as f64 * sub_ctx.duration_ms;
                    let stut_indices = render_phase_chunks(&sub_ctx, sub_ctx.duration_ms, shift_ms, None, |chunk_ctx| {
                        render_modified(child, rest, chunk_ctx, out_events);
                    });
                    all_indices.extend_from_slice(&stut_indices);
                }
            }
            ctx.active_chord_indices = all_indices;
        }
        Modifier::Transpose(amt) => {
            let start_idx = out_events.len();
            render_modified(child, rest, ctx, out_events);
            for i in start_idx..out_events.len() {
                if let ScheduledEvent::Note { pitch, .. } = &mut out_events[i] {
                    *pitch = (*pitch as i32 + *amt).clamp(0, 127) as u8;
                }
            }
        }
        Modifier::Arp(style) => {
            let mut temp_events = Vec::new();
            let mut sub_ctx = ctx.clone();
            sub_ctx.transition_fade = None;
            sub_ctx.window_start_ms = f64::MIN;
            sub_ctx.window_end_ms = f64::MAX;

            render_modified(child, rest, &mut sub_ctx, &mut temp_events);

            let mut resolved_notes = Vec::new();
            for ev in temp_events {
                match ev {
                    ScheduledEvent::Note { pitch, velocity, .. } => resolved_notes.push((pitch, velocity)),
                    cc @ ScheduledEvent::CC { .. } => {
                        if cc.start_ms() >= ctx.window_start_ms - 0.1 && cc.start_ms() < ctx.window_end_ms - 0.1 {
                            out_events.push(cc);
                        }
                    }
                }
            }

            if resolved_notes.is_empty() {
                ctx.active_chord_indices.clear();
                return;
            }

            resolved_notes.sort_by_key(|n| n.0);
            resolved_notes.dedup_by_key(|n| n.0);

            let mut pattern = Vec::new();
            match style {
                ArpStyle::Up => pattern = resolved_notes,
                ArpStyle::Down => {
                    pattern = resolved_notes;
                    pattern.reverse();
                }
                ArpStyle::UpDown => {
                    pattern = resolved_notes.clone();
                    let mut rev = resolved_notes.clone();
                    rev.reverse();
                    if rev.len() > 2 { pattern.extend(rev[1..rev.len() - 1].iter().cloned()); }
                }
                ArpStyle::DownUp => {
                    pattern = resolved_notes.clone();
                    pattern.reverse();
                    let up = resolved_notes.clone();
                    if up.len() > 2 { pattern.extend(up[1..up.len() - 1].iter().cloned()); }
                }
                ArpStyle::Converge => {
                    let mut left = 0;
                    let mut right = resolved_notes.len().saturating_sub(1);
                    while left <= right {
                        pattern.push(resolved_notes[left].clone());
                        if left != right { pattern.push(resolved_notes[right].clone()); }
                        left += 1;
                        if right == 0 { break; }
                        right -= 1;
                    }
                }
                ArpStyle::Diverge => {
                    let mid = (resolved_notes.len() - 1) / 2;
                    let mut left = mid as i32;
                    let mut right = (mid + 1) as i32;
                    if resolved_notes.len() % 2 != 0 {
                        pattern.push(resolved_notes[mid].clone());
                        left -= 1;
                    }
                    while left >= 0 || right < resolved_notes.len() as i32 {
                        if left >= 0 {
                            pattern.push(resolved_notes[left as usize].clone());
                            left -= 1;
                        }
                        if right < resolved_notes.len() as i32 {
                            pattern.push(resolved_notes[right as usize].clone());
                            right += 1;
                        }
                    }
                }
                ArpStyle::PinkyUp => {
                    if resolved_notes.len() > 1 {
                        let pinky = resolved_notes.last().unwrap().clone();
                        for i in 0..resolved_notes.len() - 1 {
                            pattern.push(resolved_notes[i].clone());
                            pattern.push(pinky.clone());
                        }
                    } else {
                        pattern = resolved_notes;
                    }
                }
                ArpStyle::PinkyUpDown => {
                    if resolved_notes.len() > 1 {
                        let pinky = resolved_notes.last().unwrap().clone();
                        for i in 0..resolved_notes.len() - 1 {
                            pattern.push(resolved_notes[i].clone());
                            pattern.push(pinky.clone());
                        }
                        for i in (1..resolved_notes.len().saturating_sub(2)).rev() {
                            pattern.push(resolved_notes[i].clone());
                            pattern.push(pinky.clone());
                        }
                    } else {
                        pattern = resolved_notes;
                    }
                }
            }

            let mut rng = get_positional_rng(ctx);
            let step_duration = ctx.duration_ms / pattern.len() as f64;
            let mut all_indices = Vec::new();

            for (i, (pitch, vel)) in pattern.into_iter().enumerate() {
                if out_events.len() >= ctx.max_events { break; }
                let step_ctx = ctx.derive_step(i, step_duration);

                if step_ctx.is_in_window(step_ctx.start_ms) {
                    let mut final_vel = vel;
                    let mut play_note = true;

                    if let Some(fade) = ctx.transition_fade {
                        final_vel = (final_vel as f64 * fade) as u8;
                        if rng.random_range(0.0..1.0) > (fade + 0.2) { play_note = false; }
                    }

                    if play_note && final_vel > 0 {
                        let mut jitter = 0.0;
                        if step_ctx.humanize_timing_range_ms > 0.0 {
                            jitter = rng.random_range(-step_ctx.humanize_timing_range_ms..=step_ctx.humanize_timing_range_ms);
                        }
                        out_events.push(ScheduledEvent::Note {
                            channel: step_ctx.channel,
                            pitch,
                            velocity: final_vel,
                            start_ms: step_ctx.start_ms + jitter,
                            duration_ms: step_ctx.duration_ms,
                        });
                        all_indices.push(out_events.len() - 1);
                    }
                }
            }
            ctx.active_chord_indices = all_indices;
        }
    }
}
