use super::math::resolve_pitch;
use crate::ast::{ArpStyle, DynamicValue, Node, Pitch};
use super::{RenderContext, ScheduledEvent};
use rand::RngExt;
use rand::rngs::StdRng;

fn calculate_lfo_phase(ctx: &RenderContext, speed: f64) -> f64 {
    let lfo_duration = ctx.master_duration_ms / speed;
    let theoretical_cycle_start = ctx.cycle_count as f64 * ctx.master_duration_ms;
    let offset = ctx.start_ms - ctx.cycle_start_ms;
    let virtual_time = theoretical_cycle_start + offset;
    (virtual_time % lfo_duration) / lfo_duration
}

pub fn flatten_notes(
    node: &Node,
    cycle_count: usize,
    macro_cycle_length: usize,
    alternator_stride: usize,
    rng: &mut StdRng,
) -> Vec<(Pitch, u8, u8)> {
    match node {
        Node::Note {
            pitch,
            velocity,
            gate,
        } => vec![(pitch.clone(), *velocity, *gate)],
        Node::CC { .. } => vec![],
        Node::Chord(elements) | Node::Sequence(elements) => {
            let mut res = Vec::new();
            for n in elements {
                res.extend(flatten_notes(
                    n,
                    cycle_count,
                    macro_cycle_length,
                    alternator_stride,
                    rng,
                ));
            }
            res
        }
        Node::Alternator(elements) => {
            if elements.is_empty() {
                return vec![];
            }
            let index = (cycle_count / alternator_stride) % elements.len();
            flatten_notes(
                &elements[index],
                cycle_count,
                macro_cycle_length,
                alternator_stride * elements.len(),
                rng,
            )
        }
        Node::RandomChoice(elements) => {
            if elements.is_empty() {
                return vec![];
            }
            let index = rng.random_range(0..elements.len());
            flatten_notes(
                &elements[index],
                cycle_count,
                macro_cycle_length,
                alternator_stride,
                rng,
            )
        }
        Node::Parallel(layers) => {
            let mut res = Vec::new();
            for l in layers {
                for n in l {
                    res.extend(flatten_notes(
                        n,
                        cycle_count,
                        macro_cycle_length,
                        alternator_stride,
                        rng,
                    ));
                }
            }
            res
        }
        Node::Polymeter(layers) => {
            let mut res = Vec::new();
            for l in layers {
                for n in l {
                    res.extend(flatten_notes(
                        n,
                        cycle_count,
                        macro_cycle_length,
                        alternator_stride,
                        rng,
                    ));
                }
            }
            res
        }
        Node::Euclidean(child, _, _) | Node::SpeedModifier(child, _) | Node::Arp(child, _) | Node::PhaseShift(child, _) => {
            flatten_notes(
                child,
                cycle_count,
                macro_cycle_length,
                alternator_stride,
                rng,
            )
        }
        Node::Condition {
            interval,
            offset,
            true_branch,
            false_branch,
        } => {
            let target = if cycle_count % interval == *offset { true_branch } else { false_branch };
            flatten_notes(target, cycle_count, macro_cycle_length, alternator_stride, rng)
        }
        Node::MacroCondition {
            interval,
            offset,
            is_gate,
            true_branch,
            false_branch,
        } => {
            let m_len = macro_cycle_length.max(1);
            let macro_cycle = cycle_count / m_len;
            let is_active_macro = macro_cycle % interval == *offset;

            let condition = is_active_macro && (!*is_gate || (cycle_count % m_len == 0));
            let target = if condition { true_branch } else { false_branch };
            flatten_notes(target, cycle_count, macro_cycle_length, alternator_stride, rng)
        }
        Node::Probability(child, prob) => {
            if *prob < 100 && rng.random_range(0..100) >= *prob {
                vec![]
            } else {
                flatten_notes(
                    child,
                    cycle_count,
                    macro_cycle_length,
                    alternator_stride,
                    rng,
                )
            }
        }
        Node::Rest | Node::Hold => vec![],
    }
}

pub fn traverse_ast(
    node: &Node,
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
    rng: &mut StdRng,
) {
    match node {
        Node::Note {
            pitch,
            velocity,
            gate,
        } => {
            if ctx.start_ms >= ctx.window_start_ms - 0.1 && ctx.start_ms < ctx.window_end_ms - 0.1 {
                let actual_pitch = resolve_pitch(pitch, &ctx.scale, ctx.octave_offset);
                let actual_duration = ctx.duration_ms * (*gate as f64 / 100.0);

                let mut final_vel = *velocity;
                let mut play_note = true;

                // TRANSITION DISSOLVE EFFECT
                if let Some(fade) = ctx.transition_fade {
                    final_vel = (final_vel as f64 * fade) as u8;
                    if rng.random_range(0.0..1.0) > (fade + 0.2) {
                        play_note = false;
                    }
                }

                if play_note && final_vel > 0 {
                    out_events.push(ScheduledEvent::Note {
                        channel: ctx.channel,
                        pitch: actual_pitch,
                        velocity: final_vel,
                        start_ms: ctx.start_ms,
                        duration_ms: actual_duration,
                    });

                    ctx.active_chord_indices.clear();
                    ctx.active_chord_indices.push(out_events.len() - 1);
                } else {
                    ctx.active_chord_indices.clear();
                }
            } else {
                ctx.active_chord_indices.clear();
            }
        }
        Node::CC { controller, value } => {
            if ctx.start_ms >= ctx.window_start_ms - 0.1 && ctx.start_ms < ctx.window_end_ms - 0.1 {
                let actual_value = match value {
                    DynamicValue::Static(v) => *v,
                    DynamicValue::Sine(min, max, speed) => {
                        let phase = calculate_lfo_phase(ctx, *speed);
                        let normalized = (phase * std::f64::consts::TAU).sin() * 0.5 + 0.5;
                        let range = *max as f64 - *min as f64;
                        (*min as f64 + normalized * range).clamp(0.0, 127.0) as u8
                    }
                    DynamicValue::Saw(min, max, speed) => {
                        let phase = calculate_lfo_phase(ctx, *speed);
                        let range = *max as f64 - *min as f64;
                        (*min as f64 + phase * range).clamp(0.0, 127.0) as u8
                    }
                    DynamicValue::Tri(min, max, speed) => {
                        let phase = calculate_lfo_phase(ctx, *speed);
                        let tri = if phase < 0.5 {
                            phase * 2.0
                        } else {
                            2.0 - phase * 2.0
                        };
                        let range = *max as f64 - *min as f64;
                        (*min as f64 + tri * range).clamp(0.0, 127.0) as u8
                    }
                };

                out_events.push(ScheduledEvent::CC {
                    channel: ctx.channel,
                    controller: *controller,
                    value: actual_value,
                    start_ms: ctx.start_ms,
                });
            }
            ctx.active_chord_indices.clear();
        }
        Node::Rest => {
            ctx.active_chord_indices.clear();
        }
        Node::Hold => {
            for &idx in &ctx.active_chord_indices {
                if let Some(event) = out_events.get_mut(idx) {
                    if let ScheduledEvent::Note { duration_ms, .. } = event {
                        *duration_ms += ctx.duration_ms;
                    }
                }
            }
        }
        Node::Chord(elements) => {
            let mut chord_indices = Vec::new();
            let orig_indices = ctx.active_chord_indices.clone();
            
            for el in elements {
                ctx.active_chord_indices = orig_indices.clone();
                traverse_ast(el, ctx, out_events, rng);
                chord_indices.extend_from_slice(&ctx.active_chord_indices);
            }
            ctx.active_chord_indices = chord_indices;
        }
        Node::Sequence(elements) => {
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
        Node::Parallel(layers) => {
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
        Node::Polymeter(layers) => {
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

                if layer.is_empty() {
                    sub_ctx.active_chord_indices.clear();
                } else {
                    let li = layer.len() as f64;
                    let speed = l0 / li;
                    let local_duration = sub_ctx.duration_ms / speed;
                    
                    let theoretical_cycle_start = sub_ctx.cycle_count as f64 * sub_ctx.master_duration_ms;
                    let offset_in_cycle = sub_ctx.start_ms - sub_ctx.cycle_start_ms;
                    let virtual_start_ms = theoretical_cycle_start + offset_in_cycle;

                    let phase_offset = (virtual_start_ms + 1e-9).rem_euclid(local_duration);
                    let chunk_start_ms = sub_ctx.start_ms - phase_offset;
                    let chunks_to_render = (sub_ctx.duration_ms / local_duration).ceil() as usize + 2;

                    for i in 0..chunks_to_render {
                        let absolute_chunk_start = chunk_start_ms + (i as f64 * local_duration);
                        let mut chunk_ctx = sub_ctx.clone();
                        chunk_ctx.start_ms = absolute_chunk_start;
                        chunk_ctx.duration_ms = local_duration;
                        
                        let virtual_chunk_start = virtual_start_ms - phase_offset + (i as f64 * local_duration);
                        chunk_ctx.cycle_count = (virtual_chunk_start / chunk_ctx.master_duration_ms).floor().max(0.0) as usize;

                        let step_duration = local_duration / li;
                        for (step_idx, el) in layer.iter().enumerate() {
                            let mut step_ctx = chunk_ctx.clone();
                            step_ctx.start_ms = chunk_ctx.start_ms + (step_idx as f64 * step_duration);
                            step_ctx.duration_ms = step_duration;
                            step_ctx.window_start_ms = sub_ctx.window_start_ms.max(step_ctx.start_ms);
                            step_ctx.window_end_ms = sub_ctx.window_end_ms.min(step_ctx.start_ms + step_duration);

                            traverse_ast(el, &mut step_ctx, out_events, rng);
                            chunk_ctx.active_chord_indices = step_ctx.active_chord_indices;
                        }
                        
                        // FIX: Moved this line INSIDE the chunks_to_render loop!
                        sub_ctx.active_chord_indices = chunk_ctx.active_chord_indices;
                    }
                }
                all_indices.extend_from_slice(&sub_ctx.active_chord_indices);
            }
            ctx.active_chord_indices = all_indices;
        }
        Node::Alternator(elements) => {
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
        Node::RandomChoice(elements) => {
            if elements.is_empty() {
                ctx.active_chord_indices.clear();
                return;
            }
            let index = rng.random_range(0..elements.len());
            traverse_ast(&elements[index], ctx, out_events, rng);
        }
        Node::Condition {
            interval,
            offset,
            true_branch,
            false_branch,
        } => {
            let target = if ctx.cycle_count % interval == *offset { true_branch } else { false_branch };
            traverse_ast(target, ctx, out_events, rng);
        }
        Node::MacroCondition {
            interval,
            offset,
            is_gate,
            true_branch,
            false_branch,
        } => {
            let m_len = ctx.macro_cycle_length.max(1);
            let macro_cycle = ctx.cycle_count / m_len;
            let is_active_macro = macro_cycle % interval == *offset;

            let condition = is_active_macro && (!*is_gate || (ctx.cycle_count % m_len == 0));
            let target = if condition { true_branch } else { false_branch };
            traverse_ast(target, ctx, out_events, rng);
        }
        Node::Probability(child, prob) => {
            if *prob < 100 && rng.random_range(0..100) >= *prob {
                ctx.active_chord_indices.clear();
            } else {
                traverse_ast(child, ctx, out_events, rng);
            }
        }
        Node::Euclidean(child, pulses, steps) => {
            if *steps == 0 || *pulses == 0 {
                ctx.active_chord_indices.clear();
                return;
            }
            let step_duration = ctx.duration_ms / *steps as f64;

            for i in 0..*steps {
                let is_hit =
                    ((i as usize * *pulses as usize) % (*steps as usize)) < (*pulses as usize);
                
                if is_hit {
                    let mut sub_ctx = ctx.clone();
                    sub_ctx.start_ms = ctx.start_ms + (i as f64 * step_duration);
                    sub_ctx.duration_ms = step_duration;
                    sub_ctx.window_start_ms = ctx.window_start_ms.max(sub_ctx.start_ms);
                    sub_ctx.window_end_ms = ctx.window_end_ms.min(sub_ctx.start_ms + step_duration);

                    traverse_ast(child, &mut sub_ctx, out_events, rng);
                    ctx.active_chord_indices = sub_ctx.active_chord_indices;
                } else {
                    ctx.active_chord_indices.clear();
                }
            }
        }
        Node::SpeedModifier(child, multiplier) => {
            let m = *multiplier as f64;
            let local_duration = ctx.duration_ms / m;
            
            // Reconstruct a theoretical time grid immune to BPM changes
            let theoretical_cycle_start = ctx.cycle_count as f64 * ctx.master_duration_ms;
            let offset_in_cycle = ctx.start_ms - ctx.cycle_start_ms;
            let virtual_start_ms = theoretical_cycle_start + offset_in_cycle;

            // Add a tiny epsilon to prevent floating point modulo rounding errors
            let phase_offset = (virtual_start_ms + 1e-9).rem_euclid(local_duration);
            let chunk_start_ms = ctx.start_ms - phase_offset;
            let chunks_to_render = (ctx.duration_ms / local_duration).ceil() as usize + 2;

            for i in 0..chunks_to_render {
                let absolute_chunk_start = chunk_start_ms + (i as f64 * local_duration);
                let mut sub_ctx = ctx.clone();
                sub_ctx.start_ms = absolute_chunk_start;
                sub_ctx.duration_ms = local_duration;
                
                let virtual_chunk_start = virtual_start_ms - phase_offset + (i as f64 * local_duration);
                sub_ctx.cycle_count = (virtual_chunk_start / ctx.master_duration_ms).floor().max(0.0) as usize;

                traverse_ast(child, &mut sub_ctx, out_events, rng);
                ctx.active_chord_indices = sub_ctx.active_chord_indices;
            }
        }
        Node::PhaseShift(child, shift_amount) => {
            // How many milliseconds to offset the child timeline by
            let shift_ms = *shift_amount as f64 * ctx.duration_ms;
            
            // Reconstruct the master time grid
            let theoretical_cycle_start = ctx.cycle_count as f64 * ctx.master_duration_ms;
            let offset_in_cycle = ctx.start_ms - ctx.cycle_start_ms;
            
            // "virtual_start_ms" is the time from the perspective of the child sequence.
            // A positive shift means the child happens later, so we look backwards in time 
            // relative to the child's perspective to see what should be playing now.
            let virtual_start_ms = theoretical_cycle_start + offset_in_cycle - shift_ms;

            // Calculate the modulo phase offset 
            let phase_offset = (virtual_start_ms + 1e-9).rem_euclid(ctx.duration_ms);
            let chunk_start_ms = ctx.start_ms - phase_offset;
            
            // 2 chunks of context are mathematically guaranteed to cover our sliding window
            let chunks_to_render = 2; 
            let mut all_indices = Vec::new();

            for i in 0..chunks_to_render {
                let absolute_chunk_start = chunk_start_ms + (i as f64 * ctx.duration_ms);
                let mut sub_ctx = ctx.clone();
                sub_ctx.start_ms = absolute_chunk_start;
                
                let virtual_chunk_start = virtual_start_ms - phase_offset + (i as f64 * ctx.duration_ms);
                sub_ctx.cycle_count = (virtual_chunk_start / ctx.master_duration_ms).floor().max(0.0) as usize;

                traverse_ast(child, &mut sub_ctx, out_events, rng);
                all_indices.extend_from_slice(&sub_ctx.active_chord_indices);
            }
            ctx.active_chord_indices = all_indices;
        }
        Node::Arp(child, style) => {
            let raw_notes = flatten_notes(
                child,
                ctx.cycle_count,
                ctx.macro_cycle_length,
                ctx.alternator_stride,
                rng,
            );
            
            if raw_notes.is_empty() {
                ctx.active_chord_indices.clear();
                return;
            }

            let mut resolved: Vec<(u8, u8, u8)> = raw_notes
                .into_iter()
                .map(|(pitch, vel, gate)| {
                    (
                        resolve_pitch(&pitch, &ctx.scale, ctx.octave_offset),
                        vel,
                        gate,
                    )
                })
                .collect();

            resolved.sort_by_key(|n| n.0);

            let mut pattern = Vec::new();
            match style {
                ArpStyle::Up => {
                    pattern = resolved;
                }
                ArpStyle::Down => {
                    pattern = resolved;
                    pattern.reverse();
                }
                ArpStyle::UpDown => {
                    pattern = resolved.clone();
                    let mut rev = resolved.clone();
                    rev.reverse();
                    if rev.len() > 2 {
                        pattern.extend(rev[1..rev.len() - 1].iter().cloned());
                    }
                }
                ArpStyle::DownUp => {
                    pattern = resolved.clone();
                    pattern.reverse();
                    let up = resolved.clone();
                    if up.len() > 2 {
                        pattern.extend(up[1..up.len() - 1].iter().cloned());
                    }
                }
                ArpStyle::Converge => {
                    let mut left = 0;
                    let mut right = resolved.len().saturating_sub(1);
                    while left <= right {
                        pattern.push(resolved[left].clone());
                        if left != right {
                            pattern.push(resolved[right].clone());
                        }
                        left += 1;
                        if right == 0 {
                            break;
                        }
                        right -= 1;
                    }
                }
                ArpStyle::Diverge => {
                    let mid = (resolved.len() - 1) / 2;
                    let mut left = mid as i32;
                    let mut right = (mid + 1) as i32;
                    if resolved.len() % 2 != 0 {
                        pattern.push(resolved[mid].clone());
                        left -= 1;
                    }
                    while left >= 0 || right < resolved.len() as i32 {
                        if left >= 0 {
                            pattern.push(resolved[left as usize].clone());
                            left -= 1;
                        }
                        if right < resolved.len() as i32 {
                            pattern.push(resolved[right as usize].clone());
                            right += 1;
                        }
                    }
                }
                ArpStyle::PinkyUp => {
                    if resolved.len() > 1 {
                        let pinky = resolved.last().unwrap().clone();
                        for i in 0..resolved.len() - 1 {
                            pattern.push(resolved[i].clone());
                            pattern.push(pinky.clone());
                        }
                    } else {
                        pattern = resolved;
                    }
                }
                ArpStyle::PinkyUpDown => {
                    if resolved.len() > 1 {
                        let pinky = resolved.last().unwrap().clone();
                        for i in 0..resolved.len() - 1 {
                            pattern.push(resolved[i].clone());
                            pattern.push(pinky.clone());
                        }
                        for i in (1..resolved.len().saturating_sub(2)).rev() {
                            pattern.push(resolved[i].clone());
                            pattern.push(pinky.clone());
                        }
                    } else {
                        pattern = resolved;
                    }
                }
            }

            if pattern.is_empty() {
                ctx.active_chord_indices.clear();
                return;
            }
            
            let step_duration = ctx.duration_ms / pattern.len() as f64;

            for (i, (pitch, vel, gate)) in pattern.into_iter().enumerate() {
                let mut sub_ctx = ctx.clone();
                sub_ctx.start_ms = ctx.start_ms + (i as f64 * step_duration);
                sub_ctx.duration_ms = step_duration;
                sub_ctx.window_start_ms = ctx.window_start_ms.max(sub_ctx.start_ms);
                sub_ctx.window_end_ms = ctx.window_end_ms.min(sub_ctx.start_ms + step_duration);

                if sub_ctx.start_ms >= sub_ctx.window_start_ms - 0.1
                    && sub_ctx.start_ms < sub_ctx.window_end_ms - 0.1
                {
                    let actual_duration = step_duration * (gate as f64 / 100.0);
                    
                    let mut final_vel = vel;
                    let mut play_note = true;

                    // TRANSITION DISSOLVE EFFECT ON ARPS
                    if let Some(fade) = sub_ctx.transition_fade {
                        final_vel = (final_vel as f64 * fade) as u8;
                        if rng.random_range(0.0..1.0) > (fade + 0.2) {
                            play_note = false;
                        }
                    }

                    if play_note && final_vel > 0 {
                        out_events.push(ScheduledEvent::Note {
                            channel: sub_ctx.channel,
                            pitch,
                            velocity: final_vel,
                            start_ms: sub_ctx.start_ms,
                            duration_ms: actual_duration,
                        });
                        
                        ctx.active_chord_indices.clear();
                        ctx.active_chord_indices.push(out_events.len() - 1);
                    } else {
                        ctx.active_chord_indices.clear();
                    }
                } else {
                    ctx.active_chord_indices.clear();
                }
            }
        }
    }
}
