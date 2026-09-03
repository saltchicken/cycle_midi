use super::math::resolve_pitch;
use crate::ast::{ArpStyle, DynamicValue, Node, Pitch};
use super::{RenderContext, ScheduledEvent};
use rand::RngExt;
use rand::rngs::StdRng;

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
        Node::CC { .. } => vec![], // CCs are ignored in Arp evaluation
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
        Node::Euclidean(child, _, _) | Node::SpeedModifier(child, _) | Node::Arp(child, _) => {
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
            if cycle_count % interval == *offset {
                flatten_notes(
                    true_branch,
                    cycle_count,
                    macro_cycle_length,
                    alternator_stride,
                    rng,
                )
            } else {
                flatten_notes(
                    false_branch,
                    cycle_count,
                    macro_cycle_length,
                    alternator_stride,
                    rng,
                )
            }
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

            if is_active_macro && (!*is_gate || (cycle_count % m_len == 0)) {
                flatten_notes(
                    true_branch,
                    cycle_count,
                    macro_cycle_length,
                    alternator_stride,
                    rng,
                )
            } else {
                flatten_notes(
                    false_branch,
                    cycle_count,
                    macro_cycle_length,
                    alternator_stride,
                    rng,
                )
            }
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
    ctx: RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
    rng: &mut StdRng,
) -> Vec<usize> {
    match node {
        Node::Note {
            pitch,
            velocity,
            gate,
        } => {
            if ctx.start_ms >= ctx.window_start_ms - 0.1 && ctx.start_ms < ctx.window_end_ms - 0.1 {
                let actual_pitch = resolve_pitch(pitch, &ctx.scale, ctx.octave_offset);
                let actual_duration = ctx.duration_ms * (*gate as f64 / 100.0);

                out_events.push(ScheduledEvent::Note {
                    channel: ctx.channel,
                    pitch: actual_pitch,
                    velocity: *velocity,
                    start_ms: ctx.start_ms,
                    duration_ms: actual_duration,
                });

                return vec![out_events.len() - 1];
            }
            vec![]
        }
        Node::CC { controller, value } => {
            if ctx.start_ms >= ctx.window_start_ms - 0.1 && ctx.start_ms < ctx.window_end_ms - 0.1 {
                let actual_value = match value {
                    DynamicValue::Static(v) => *v,
                    DynamicValue::Sine(min, max, speed) => {
                        let lfo_duration = ctx.master_duration_ms / speed;
                        let phase = (ctx.start_ms % lfo_duration) / lfo_duration;
                        let normalized = (phase * std::f64::consts::TAU).sin() * 0.5 + 0.5;
                        let range = *max as f64 - *min as f64;
                        (*min as f64 + normalized * range).clamp(0.0, 127.0) as u8
                    }
                    DynamicValue::Saw(min, max, speed) => {
                        let lfo_duration = ctx.master_duration_ms / speed;
                        let phase = (ctx.start_ms % lfo_duration) / lfo_duration;
                        let range = *max as f64 - *min as f64;
                        (*min as f64 + phase * range).clamp(0.0, 127.0) as u8
                    }
                    DynamicValue::Tri(min, max, speed) => {
                        let lfo_duration = ctx.master_duration_ms / speed;
                        let phase = (ctx.start_ms % lfo_duration) / lfo_duration;
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
            vec![]
        }
        Node::Rest => {
            vec![]
        }
        Node::Hold => {
            if ctx.start_ms >= ctx.window_start_ms - 0.1 && ctx.start_ms < ctx.window_end_ms - 0.1 {
                for &idx in &ctx.active_chord_indices {
                    if let Some(event) = out_events.get_mut(idx) {
                        if let ScheduledEvent::Note { duration_ms, .. } = event {
                            *duration_ms += ctx.duration_ms;
                        }
                    }
                }
                return ctx.active_chord_indices.clone();
            }
            vec![]
        }
        Node::Chord(elements) => {
            let mut indices = vec![];
            for el in elements {
                indices.extend(traverse_ast(el, ctx.clone(), out_events, rng));
            }
            indices
        }
        Node::Sequence(elements) => {
            if elements.is_empty() {
                return vec![];
            }
            let step_duration = ctx.duration_ms / elements.len() as f64;

            let mut last_indices = ctx.active_chord_indices.clone();

            for (i, el) in elements.iter().enumerate() {
                let mut step_ctx = ctx.clone();
                step_ctx.start_ms = ctx.start_ms + (i as f64 * step_duration);
                step_ctx.duration_ms = step_duration;
                step_ctx.window_start_ms = step_ctx.window_start_ms.max(step_ctx.start_ms);
                step_ctx.window_end_ms = step_ctx
                    .window_end_ms
                    .min(step_ctx.start_ms + step_duration);

                step_ctx.active_chord_indices = last_indices;
                last_indices = traverse_ast(el, step_ctx, out_events, rng);
            }
            last_indices
        }
        Node::Parallel(layers) => {
            let mut all_indices = vec![];
            for layer in layers {
                all_indices.extend(traverse_ast(
                    &Node::Sequence(layer.clone()),
                    ctx.clone(),
                    out_events,
                    rng,
                ));
            }
            all_indices
        }
        Node::Alternator(elements) => {
            if elements.is_empty() {
                return vec![];
            }

            let index = (ctx.cycle_count / ctx.alternator_stride) % elements.len();

            let mut step_ctx = ctx.clone();
            step_ctx.alternator_stride = ctx.alternator_stride * elements.len();

            traverse_ast(&elements[index], step_ctx, out_events, rng)
        }
        Node::RandomChoice(elements) => {
            if elements.is_empty() {
                return vec![];
            }
            let index = rng.random_range(0..elements.len());
            traverse_ast(&elements[index], ctx, out_events, rng)
        }
        Node::Condition {
            interval,
            offset,
            true_branch,
            false_branch,
        } => {
            if ctx.cycle_count % interval == *offset {
                traverse_ast(true_branch, ctx, out_events, rng)
            } else {
                traverse_ast(false_branch, ctx, out_events, rng)
            }
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

            if is_active_macro && (!*is_gate || (ctx.cycle_count % m_len == 0)) {
                traverse_ast(true_branch, ctx, out_events, rng)
            } else {
                traverse_ast(false_branch, ctx, out_events, rng)
            }
        }
        Node::Probability(child, prob) => {
            if *prob < 100 && rng.random_range(0..100) >= *prob {
                vec![]
            } else {
                traverse_ast(child, ctx, out_events, rng)
            }
        }
        Node::Euclidean(child, pulses, steps) => {
            if *steps == 0 || *pulses == 0 {
                return vec![];
            }
            let step_duration = ctx.duration_ms / *steps as f64;
            let mut last_indices = ctx.active_chord_indices.clone();

            for i in 0..*steps {
                let is_hit =
                    ((i as usize * *pulses as usize) % (*steps as usize)) < (*pulses as usize);
                if is_hit {
                    let mut step_ctx = ctx.clone();
                    step_ctx.start_ms = ctx.start_ms + (i as f64 * step_duration);
                    step_ctx.duration_ms = step_duration;
                    step_ctx.window_start_ms = step_ctx.window_start_ms.max(step_ctx.start_ms);
                    step_ctx.window_end_ms = step_ctx
                        .window_end_ms
                        .min(step_ctx.start_ms + step_duration);

                    step_ctx.active_chord_indices = last_indices;
                    last_indices = traverse_ast(child, step_ctx, out_events, rng);
                } else {
                    last_indices = vec![];
                }
            }
            last_indices
        }
        Node::SpeedModifier(child, multiplier) => {
            let m = *multiplier as f64;
            let local_duration = ctx.duration_ms / m;
            let phase_offset = ctx.start_ms.rem_euclid(local_duration);
            let chunk_start_ms = ctx.start_ms - phase_offset;
            let chunks_to_render = (ctx.duration_ms / local_duration).ceil() as usize + 2;

            let mut last_indices = ctx.active_chord_indices.clone();

            for i in 0..chunks_to_render {
                let mut step_ctx = ctx.clone();
                let absolute_chunk_start = chunk_start_ms + (i as f64 * local_duration);

                step_ctx.start_ms = absolute_chunk_start;
                step_ctx.duration_ms = local_duration;
                step_ctx.cycle_count = (absolute_chunk_start / local_duration).round() as usize;

                step_ctx.active_chord_indices = last_indices;
                last_indices = traverse_ast(child, step_ctx, out_events, rng);
            }
            last_indices
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
                return vec![];
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
                return vec![];
            }
            let step_duration = ctx.duration_ms / pattern.len() as f64;
            let mut last_indices = ctx.active_chord_indices.clone();

            for (i, (pitch, vel, gate)) in pattern.into_iter().enumerate() {
                let mut step_ctx = ctx.clone();
                step_ctx.start_ms = ctx.start_ms + (i as f64 * step_duration);
                step_ctx.duration_ms = step_duration;
                step_ctx.window_start_ms = step_ctx.window_start_ms.max(step_ctx.start_ms);
                step_ctx.window_end_ms = step_ctx
                    .window_end_ms
                    .min(step_ctx.start_ms + step_duration);

                if step_ctx.start_ms >= step_ctx.window_start_ms - 0.1
                    && step_ctx.start_ms < step_ctx.window_end_ms - 0.1
                {
                    let actual_duration = step_duration * (gate as f64 / 100.0);
                    out_events.push(ScheduledEvent::Note {
                        channel: ctx.channel,
                        pitch,
                        velocity: vel,
                        start_ms: step_ctx.start_ms,
                        duration_ms: actual_duration,
                    });
                    last_indices = vec![out_events.len() - 1];
                } else {
                    last_indices = vec![];
                }
            }
            last_indices
        }
    }
}
