use super::math::resolve_pitch;
use crate::ast::{ArpStyle, DynamicValue, Node};
use super::{RenderContext, ScheduledEvent};
use rand::RngExt;
use rand::rngs::StdRng;
use rand::distr::Distribution;
use rand::distr::weighted::WeightedIndex;

fn calculate_lfo_phase(ctx: &RenderContext, speed: f64) -> f64 {
    let lfo_duration = ctx.master_duration_ms / speed;
    let theoretical_cycle_start = ctx.cycle_count as f64 * ctx.master_duration_ms;
    let offset = ctx.start_ms - ctx.cycle_start_ms;
    let virtual_time = theoretical_cycle_start + offset;
    (virtual_time % lfo_duration) / lfo_duration
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
                
                // NEW: Ratchet division math applied at note-render time
                let splits = ctx.ratchet_splits.max(1);
                let sub_step = ctx.duration_ms / splits as f64;
                let actual_duration = sub_step * (*gate as f64 / 100.0);

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
                    ctx.active_chord_indices.clear();
                    for i in 0..splits {
                        let mut jitter = 0.0;
                        if ctx.humanize_timing_range_ms > 0.0 {
                            jitter = rng.random_range(-ctx.humanize_timing_range_ms..=ctx.humanize_timing_range_ms);
                        }

                        let mut split_vel = final_vel;
                        if ctx.humanize_velocity_range > 0 {
                            let offset = rng.random_range(-(ctx.humanize_velocity_range as i32)..=(ctx.humanize_velocity_range as i32));
                            split_vel = (split_vel as i32 + offset).clamp(1, 127) as u8;
                        }

                        out_events.push(ScheduledEvent::Note {
                            channel: ctx.channel,
                            pitch: actual_pitch,
                            velocity: split_vel,
                            start_ms: ctx.start_ms + (i as f64 * sub_step) + jitter,
                            duration_ms: actual_duration,
                        });
                        ctx.active_chord_indices.push(out_events.len() - 1);
                    }
                } else {
                    ctx.active_chord_indices.clear();
                }
            } else {
                ctx.active_chord_indices.clear();
            }
        }
        Node::CC { controller, value } => {
            if ctx.start_ms >= ctx.window_start_ms - 0.1 && ctx.start_ms < ctx.window_end_ms - 0.1 {
                let splits = ctx.ratchet_splits.max(1);
                let sub_step = ctx.duration_ms / splits as f64;

                ctx.active_chord_indices.clear(); // Ensure clear before loop

                for i in 0..splits {
                    let mut jitter = 0.0;
                    if ctx.humanize_timing_range_ms > 0.0 {
                        jitter = rng.random_range(-ctx.humanize_timing_range_ms..=ctx.humanize_timing_range_ms);
                    }
                    let note_start = ctx.start_ms + (i as f64 * sub_step) + jitter;

                    // Re-calculate the phase precisely for the sub-step time
                    let mut lfo_ctx = ctx.clone();
                    lfo_ctx.start_ms = note_start;

                    let actual_value = match value {
                        DynamicValue::Static(v) => *v,
                        DynamicValue::Sine(min, max, speed) => {
                            let phase = calculate_lfo_phase(&lfo_ctx, *speed);
                            let normalized = (phase * std::f64::consts::TAU).sin() * 0.5 + 0.5;
                            let range = *max as f64 - *min as f64;
                            (*min as f64 + normalized * range).clamp(0.0, 127.0) as u8
                        }
                        DynamicValue::Saw(min, max, speed) => {
                            let phase = calculate_lfo_phase(&lfo_ctx, *speed);
                            let range = *max as f64 - *min as f64;
                            (*min as f64 + phase * range).clamp(0.0, 127.0) as u8
                        }
                        DynamicValue::Tri(min, max, speed) => {
                            let phase = calculate_lfo_phase(&lfo_ctx, *speed);
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
                        start_ms: note_start,
                    });
                }
            } else {
                ctx.active_chord_indices.clear();
            }
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
        Node::Ref(_) => {
            ctx.active_chord_indices.clear();
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
                        
                        chunk_ctx.master_duration_ms = local_duration;
                        chunk_ctx.cycle_start_ms = absolute_chunk_start;
                        
                        let virtual_chunk_start = virtual_start_ms - phase_offset + (i as f64 * local_duration);
                        chunk_ctx.cycle_count = (virtual_chunk_start / local_duration).floor().max(0.0) as usize;

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
            
            let weights: Vec<u32> = elements.iter().map(|(w, _)| *w).collect();
            
            if let Ok(dist) = WeightedIndex::new(&weights) {
                let index = dist.sample(rng);
                traverse_ast(&elements[index].1, ctx, out_events, rng);
            } else {
                let index = rng.random_range(0..elements.len());
                traverse_ast(&elements[index].1, ctx, out_events, rng);
            }
        }
        Node::Ratchet(child, splits) => {
            // Ratchet passes the multiplier down instead of re-evaluating the AST
            let mut sub_ctx = ctx.clone();
            sub_ctx.ratchet_splits *= *splits as usize;
            traverse_ast(child, &mut sub_ctx, out_events, rng);
            ctx.active_chord_indices = sub_ctx.active_chord_indices;
        }
        Node::HumanizeVelocity(child, amount) => {
            let mut sub_ctx = ctx.clone();
            sub_ctx.humanize_velocity_range = *amount;
            traverse_ast(child, &mut sub_ctx, out_events, rng);
            ctx.active_chord_indices = sub_ctx.active_chord_indices;
        }
        Node::HumanizeTiming(child, amount) => {
            let mut sub_ctx = ctx.clone();
            sub_ctx.humanize_timing_range_ms = amount.abs();
            traverse_ast(child, &mut sub_ctx, out_events, rng);
            ctx.active_chord_indices = sub_ctx.active_chord_indices;
        }
        Node::SeqP(segments, is_loop) => {
            let max_end = segments.iter().map(|s| s.1).max().unwrap_or(1).max(1);
            let current_cycle = if *is_loop {
                ctx.cycle_count % max_end
            } else {
                ctx.cycle_count
            };

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
            
            let theoretical_cycle_start = ctx.cycle_count as f64 * ctx.master_duration_ms;
            let offset_in_cycle = ctx.start_ms - ctx.cycle_start_ms;
            let virtual_start_ms = theoretical_cycle_start + offset_in_cycle;

            let phase_offset = (virtual_start_ms + 1e-9).rem_euclid(local_duration);
            let chunk_start_ms = ctx.start_ms - phase_offset;
            let chunks_to_render = (ctx.duration_ms / local_duration).ceil() as usize + 2;

            for i in 0..chunks_to_render {
                let absolute_chunk_start = chunk_start_ms + (i as f64 * local_duration);
                let mut sub_ctx = ctx.clone();
                sub_ctx.start_ms = absolute_chunk_start;
                sub_ctx.duration_ms = local_duration;
                
                sub_ctx.master_duration_ms = local_duration;
                sub_ctx.cycle_start_ms = absolute_chunk_start;

                let virtual_chunk_start = virtual_start_ms - phase_offset + (i as f64 * local_duration);
                sub_ctx.cycle_count = (virtual_chunk_start / local_duration).floor().max(0.0) as usize;

                traverse_ast(child, &mut sub_ctx, out_events, rng);
                ctx.active_chord_indices = sub_ctx.active_chord_indices;
            }
        }
        Node::PhaseShift(child, shift_amount) => {
            let shift_ms = *shift_amount as f64 * ctx.duration_ms;
            
            let theoretical_cycle_start = ctx.cycle_count as f64 * ctx.master_duration_ms;
            let offset_in_cycle = ctx.start_ms - ctx.cycle_start_ms;
            
            let virtual_start_ms = theoretical_cycle_start + offset_in_cycle - shift_ms;

            let phase_offset = (virtual_start_ms + 1e-9).rem_euclid(ctx.duration_ms);
            let chunk_start_ms = ctx.start_ms - phase_offset;
            
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
            let mut temp_events = Vec::new();
            let mut sub_ctx = ctx.clone();
            // Disable transition dissolve for the buffer generation to avoid double-fading notes
            sub_ctx.transition_fade = None;

            // Generate notes for the child node
            traverse_ast(child, &mut sub_ctx, &mut temp_events, rng);

            let mut resolved_notes = Vec::new();
            for ev in temp_events {
                match ev {
                    ScheduledEvent::Note { pitch, velocity, .. } => {
                        resolved_notes.push((pitch, velocity));
                    }
                    cc @ ScheduledEvent::CC { .. } => {
                        // Pass CCs through unharmed
                        out_events.push(cc);
                    }
                }
            }

            if resolved_notes.is_empty() {
                ctx.active_chord_indices.clear();
                return;
            }

            // Deduplicate and sort pitches
            resolved_notes.sort_by_key(|n| n.0);
            resolved_notes.dedup_by_key(|n| n.0);

            let mut pattern = Vec::new();
            match style {
                ArpStyle::Up => {
                    pattern = resolved_notes;
                }
                ArpStyle::Down => {
                    pattern = resolved_notes;
                    pattern.reverse();
                }
                ArpStyle::UpDown => {
                    pattern = resolved_notes.clone();
                    let mut rev = resolved_notes.clone();
                    rev.reverse();
                    if rev.len() > 2 {
                        pattern.extend(rev[1..rev.len() - 1].iter().cloned());
                    }
                }
                ArpStyle::DownUp => {
                    pattern = resolved_notes.clone();
                    pattern.reverse();
                    let up = resolved_notes.clone();
                    if up.len() > 2 {
                        pattern.extend(up[1..up.len() - 1].iter().cloned());
                    }
                }
                ArpStyle::Converge => {
                    let mut left = 0;
                    let mut right = resolved_notes.len().saturating_sub(1);
                    while left <= right {
                        pattern.push(resolved_notes[left].clone());
                        if left != right {
                            pattern.push(resolved_notes[right].clone());
                        }
                        left += 1;
                        if right == 0 {
                            break;
                        }
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

            if pattern.is_empty() {
                ctx.active_chord_indices.clear();
                return;
            }

            let step_duration = ctx.duration_ms / pattern.len() as f64;
            let mut all_indices = Vec::new();

            for (i, (pitch, vel)) in pattern.into_iter().enumerate() {
                let mut step_ctx = ctx.clone();
                step_ctx.start_ms = ctx.start_ms + (i as f64 * step_duration);
                step_ctx.duration_ms = step_duration;
                step_ctx.window_start_ms = ctx.window_start_ms.max(step_ctx.start_ms);
                step_ctx.window_end_ms = ctx.window_end_ms.min(step_ctx.start_ms + step_duration);

                if step_ctx.start_ms >= step_ctx.window_start_ms - 0.1
                    && step_ctx.start_ms < step_ctx.window_end_ms - 0.1
                {
                    // If Arp itself was subjected to ratcheting, this inherits ctx.ratchet_splits
                    let splits = step_ctx.ratchet_splits.max(1);
                    let sub_step = step_ctx.duration_ms / splits as f64;
                    // Default to legato (100% gate) for arpeggiated steps
                    let actual_duration = sub_step;

                    let mut final_vel = vel;
                    let mut play_note = true;

                    // Re-apply the transition fade to the generated Arp notes
                    if let Some(fade) = ctx.transition_fade {
                        final_vel = (final_vel as f64 * fade) as u8;
                        if rng.random_range(0.0..1.0) > (fade + 0.2) {
                            play_note = false;
                        }
                    }

                    if play_note && final_vel > 0 {
                        for sub_i in 0..splits {
                            let mut jitter = 0.0;
                            if step_ctx.humanize_timing_range_ms > 0.0 {
                                jitter = rng.random_range(-step_ctx.humanize_timing_range_ms..=step_ctx.humanize_timing_range_ms);
                            }

                            out_events.push(ScheduledEvent::Note {
                                channel: step_ctx.channel,
                                pitch,
                                velocity: final_vel, // Note: Velocity humanization was already safely captured when Arp resolved its children.
                                start_ms: step_ctx.start_ms + (sub_i as f64 * sub_step) + jitter,
                                duration_ms: actual_duration,
                            });
                            all_indices.push(out_events.len() - 1);
                        }
                    }
                }
            }
            ctx.active_chord_indices = all_indices;
        }
    }
}
