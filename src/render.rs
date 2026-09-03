use crate::ast::{Node, Pitch, Program, RenderContext, ScheduledNote, ArpStyle, ScaleDef, SeedInterval};
use rand::{RngExt, SeedableRng};
use rand::rngs::StdRng;

pub fn resolve_pitch(pitch: &Pitch, scale: &Option<ScaleDef>, octave_offset: i32) -> u8 {
    let shift = octave_offset * 12;
    match pitch {
        Pitch::Absolute(p) => (*p as i32 + shift).clamp(0, 127) as u8,
        Pitch::Numeric(val) => {
            let val = *val;
            if let Some(scale) = scale {
                let scale_len = scale.intervals.len() as i32;
                let octave = val.div_euclid(scale_len);
                let degree = val.rem_euclid(scale_len) as usize;
                let note = scale.root_pitch as i32 + (octave * 12) + scale.intervals[degree] as i32 + shift;
                note.clamp(0, 127) as u8
            } else {
                (val + shift).clamp(0, 127) as u8
            }
        }
    }
}

fn flatten_notes(node: &Node, cycle_count: usize, macro_cycle_length: usize, alternator_stride: usize, rng: &mut StdRng) -> Vec<(Pitch, u8, u8, u8)> {
    match node {
        Node::Note { pitch, velocity, gate, prob } => vec![(pitch.clone(), *velocity, *gate, *prob)],
        Node::Chord(elements) | Node::Sequence(elements) => {
            let mut res = Vec::new();
            for n in elements {
                res.extend(flatten_notes(n, cycle_count, macro_cycle_length, alternator_stride, rng));
            }
            res
        }
        Node::Alternator(elements) => {
            if elements.is_empty() { return vec![]; }
            let index = (cycle_count / alternator_stride) % elements.len();
            flatten_notes(&elements[index], cycle_count, macro_cycle_length, alternator_stride * elements.len(), rng)
        }
        Node::RandomChoice(elements) => {
            if elements.is_empty() { return vec![]; }
            let index = rng.random_range(0..elements.len());
            flatten_notes(&elements[index], cycle_count, macro_cycle_length, alternator_stride, rng)
        }
        Node::Parallel(layers) => {
            let mut res = Vec::new();
            for l in layers {
                for n in l {
                    res.extend(flatten_notes(n, cycle_count, macro_cycle_length, alternator_stride, rng));
                }
            }
            res
        }
        Node::Euclidean(child, _, _) | Node::SpeedModifier(child, _) | Node::Arp(child, _) => flatten_notes(child, cycle_count, macro_cycle_length, alternator_stride, rng),
        Node::Condition { interval, offset, true_branch, false_branch } => {
            if cycle_count % interval == *offset {
                flatten_notes(true_branch, cycle_count, macro_cycle_length, alternator_stride, rng)
            } else {
                flatten_notes(false_branch, cycle_count, macro_cycle_length, alternator_stride, rng)
            }
        }
        Node::MacroCondition { interval, offset, is_gate, true_branch, false_branch } => {
            let m_len = macro_cycle_length.max(1);
            let macro_cycle = cycle_count / m_len;
            let is_active_macro = macro_cycle % interval == *offset;
            
            if is_active_macro && (!*is_gate || (cycle_count % m_len == 0)) {
                flatten_notes(true_branch, cycle_count, macro_cycle_length, alternator_stride, rng)
            } else {
                flatten_notes(false_branch, cycle_count, macro_cycle_length, alternator_stride, rng)
            }
        }
        Node::Rest | Node::Hold => vec![],
    }
}

pub fn traverse_ast(node: &Node, ctx: RenderContext, out_notes: &mut Vec<ScheduledNote>, rng: &mut StdRng) -> Vec<usize> {
    match node {
        Node::Note { pitch, velocity, gate, prob } => {
            if *prob < 100 && rng.random_range(0..100) >= *prob {
                return vec![]; 
            }

            if ctx.start_ms >= ctx.window_start_ms - 0.1 && ctx.start_ms < ctx.window_end_ms - 0.1 {
                let actual_pitch = resolve_pitch(pitch, &ctx.scale, ctx.octave_offset);
                let actual_duration = ctx.duration_ms * (*gate as f64 / 100.0);
                
                out_notes.push(ScheduledNote {
                    channel: ctx.channel,
                    pitch: actual_pitch,
                    velocity: *velocity,
                    start_ms: ctx.start_ms,
                    duration_ms: actual_duration,
                });
                
                return vec![out_notes.len() - 1];
            }
            vec![]
        }
        Node::Rest => {
            vec![]
        }
        Node::Hold => {
            if ctx.start_ms >= ctx.window_start_ms - 0.1 && ctx.start_ms < ctx.window_end_ms - 0.1 {
                for &idx in &ctx.active_chord_indices {
                    if let Some(note) = out_notes.get_mut(idx) {
                        note.duration_ms += ctx.duration_ms;
                    }
                }
                return ctx.active_chord_indices.clone();
            }
            vec![]
        }
        Node::Chord(elements) => {
            let mut indices = vec![];
            for el in elements {
                indices.extend(traverse_ast(el, ctx.clone(), out_notes, rng));
            }
            indices
        }
        Node::Sequence(elements) => {
            if elements.is_empty() { return vec![]; }
            let step_duration = ctx.duration_ms / elements.len() as f64;
            
            let mut last_indices = ctx.active_chord_indices.clone();
            
            for (i, el) in elements.iter().enumerate() {
                let mut step_ctx = ctx.clone();
                step_ctx.start_ms = ctx.start_ms + (i as f64 * step_duration);
                step_ctx.duration_ms = step_duration;
                step_ctx.window_start_ms = step_ctx.window_start_ms.max(step_ctx.start_ms);
                step_ctx.window_end_ms = step_ctx.window_end_ms.min(step_ctx.start_ms + step_duration);
                
                step_ctx.active_chord_indices = last_indices;
                last_indices = traverse_ast(el, step_ctx, out_notes, rng);
            }
            last_indices
        }
        Node::Parallel(layers) => {
            let mut all_indices = vec![];
            for layer in layers {
                all_indices.extend(traverse_ast(&Node::Sequence(layer.clone()), ctx.clone(), out_notes, rng));
            }
            all_indices
        }
        Node::Alternator(elements) => {
            if elements.is_empty() { return vec![]; }
            
            let index = (ctx.cycle_count / ctx.alternator_stride) % elements.len();
            
            let mut step_ctx = ctx.clone();
            step_ctx.alternator_stride = ctx.alternator_stride * elements.len();
            
            traverse_ast(&elements[index], step_ctx, out_notes, rng)
        }
        Node::RandomChoice(elements) => {
            if elements.is_empty() { return vec![]; }
            let index = rng.random_range(0..elements.len());
            traverse_ast(&elements[index], ctx, out_notes, rng)
        }
        Node::Condition { interval, offset, true_branch, false_branch } => {
            if ctx.cycle_count % interval == *offset {
                traverse_ast(true_branch, ctx, out_notes, rng)
            } else {
                traverse_ast(false_branch, ctx, out_notes, rng)
            }
        }
        Node::MacroCondition { interval, offset, is_gate, true_branch, false_branch } => {
            let m_len = ctx.macro_cycle_length.max(1);
            let macro_cycle = ctx.cycle_count / m_len;
            let is_active_macro = macro_cycle % interval == *offset;
            
            if is_active_macro && (!*is_gate || (ctx.cycle_count % m_len == 0)) {
                traverse_ast(true_branch, ctx, out_notes, rng)
            } else {
                traverse_ast(false_branch, ctx, out_notes, rng)
            }
        }
        Node::Euclidean(child, pulses, steps) => {
            if *steps == 0 || *pulses == 0 { return vec![]; }
            let step_duration = ctx.duration_ms / *steps as f64;
            let mut last_indices = ctx.active_chord_indices.clone();
            
            for i in 0..*steps {
                let is_hit = ((i as usize * *pulses as usize) % (*steps as usize)) < (*pulses as usize);
                if is_hit {
                    let mut step_ctx = ctx.clone();
                    step_ctx.start_ms = ctx.start_ms + (i as f64 * step_duration);
                    step_ctx.duration_ms = step_duration;
                    step_ctx.window_start_ms = step_ctx.window_start_ms.max(step_ctx.start_ms);
                    step_ctx.window_end_ms = step_ctx.window_end_ms.min(step_ctx.start_ms + step_duration);
                    
                    step_ctx.active_chord_indices = last_indices;
                    last_indices = traverse_ast(child, step_ctx, out_notes, rng);
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
                last_indices = traverse_ast(child, step_ctx, out_notes, rng);
            }
            last_indices
        }
        Node::Arp(child, style) => {
            let raw_notes = flatten_notes(child, ctx.cycle_count, ctx.macro_cycle_length, ctx.alternator_stride, rng);
            if raw_notes.is_empty() { return vec![]; }
            
            let mut resolved: Vec<(u8, u8, u8, u8)> = raw_notes.into_iter().map(|(pitch, vel, gate, prob)| {
                (resolve_pitch(&pitch, &ctx.scale, ctx.octave_offset), vel, gate, prob)
            }).collect();
            
            resolved.sort_by_key(|n| n.0);
            
            let mut pattern = Vec::new();
            match style {
                ArpStyle::Up => { pattern = resolved; }
                ArpStyle::Down => { pattern = resolved; pattern.reverse(); }
                ArpStyle::UpDown => {
                    pattern = resolved.clone();
                    let mut rev = resolved.clone();
                    rev.reverse();
                    if rev.len() > 2 {
                        pattern.extend(rev[1..rev.len()-1].iter().cloned());
                    }
                }
                ArpStyle::DownUp => {
                    pattern = resolved.clone();
                    pattern.reverse();
                    let up = resolved.clone();
                    if up.len() > 2 {
                        pattern.extend(up[1..up.len()-1].iter().cloned());
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
                        if right == 0 { break; }
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
                        for i in 0..resolved.len()-1 {
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
                        for i in 0..resolved.len()-1 {
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

            if pattern.is_empty() { return vec![]; }
            let step_duration = ctx.duration_ms / pattern.len() as f64;
            let mut last_indices = ctx.active_chord_indices.clone();

            for (i, (pitch, vel, gate, prob)) in pattern.into_iter().enumerate() {
                if prob < 100 && rng.random_range(0..100) >= prob {
                    last_indices = vec![];
                    continue;
                }
                
                let mut step_ctx = ctx.clone();
                step_ctx.start_ms = ctx.start_ms + (i as f64 * step_duration);
                step_ctx.duration_ms = step_duration;
                step_ctx.window_start_ms = step_ctx.window_start_ms.max(step_ctx.start_ms);
                step_ctx.window_end_ms = step_ctx.window_end_ms.min(step_ctx.start_ms + step_duration);

                if step_ctx.start_ms >= step_ctx.window_start_ms - 0.1 && step_ctx.start_ms < step_ctx.window_end_ms - 0.1 {
                    let actual_duration = step_duration * (gate as f64 / 100.0);
                    out_notes.push(ScheduledNote {
                        channel: ctx.channel,
                        pitch,
                        velocity: vel,
                        start_ms: step_ctx.start_ms,
                        duration_ms: actual_duration,
                    });
                    last_indices = vec![out_notes.len() - 1];
                } else {
                    last_indices = vec![];
                }
            }
            last_indices
        }
    }
}

pub fn generate_next_cycle(
    program: &Program, 
    bpm: f64, 
    cycle_start_time_ms: f64, 
    cycle_count: usize,
    macro_cycle_length: usize,
) -> Vec<ScheduledNote> {
    if program.global_silence {
        return Vec::new();
    }

    let master_duration_ms = (60_000.0 / bpm) * 4.0; 
    let mut notes = Vec::new();
    let macro_cycle_count = cycle_count / macro_cycle_length.max(1);

    for track in &program.tracks {
        if track.is_muted { continue; }
        
        let active_scale = if track.channel == 9 {
            track.scale.clone()
        } else {
            track.scale.clone().or(program.scale.clone())
        };

        let mut rng = if let Some(seed_def) = &track.seed {
            let mut final_seed = seed_def.base;
            if let Some(interval) = &seed_def.interval {
                let seed_bump = match interval {
                    SeedInterval::Macro(m) => (macro_cycle_count / *m) as u64,
                    SeedInterval::Micro(m) => (cycle_count / *m) as u64,
                };
                final_seed = final_seed.wrapping_add(seed_bump);
            }
            StdRng::seed_from_u64(final_seed)
        } else {
            StdRng::seed_from_u64(rand::random::<u64>())
        };

        let ctx = RenderContext {
            channel: track.channel,
            start_ms: cycle_start_time_ms,
            duration_ms: master_duration_ms,
            window_start_ms: cycle_start_time_ms,
            window_end_ms: cycle_start_time_ms + master_duration_ms,
            cycle_count,
            macro_cycle_length,
            scale: active_scale, 
            active_chord_indices: vec![],
            octave_offset: track.octave_offset,
            alternator_stride: 1, // <-- Init at root
        };
        traverse_ast(&track.root_node, ctx, &mut notes, &mut rng);
    }
    
    notes
}
