use crate::ast::{Node, Pitch, Program, RenderContext, ScheduledNote};

pub fn traverse_ast(node: &Node, ctx: RenderContext, out_notes: &mut Vec<ScheduledNote>) -> Vec<usize> {
    match node {
        Node::Note { pitch, velocity, gate, .. } => {
            if ctx.start_ms >= ctx.window_start_ms - 0.1 && ctx.start_ms < ctx.window_end_ms - 0.1 {
                let actual_pitch = match pitch {
                    Pitch::Absolute(p) => *p,
                    Pitch::Numeric(val) => {
                        let val = *val; 
                        if let Some(scale) = &ctx.scale {
                            let scale_len = scale.intervals.len() as i32;
                            let octave = val.div_euclid(scale_len);
                            let degree = val.rem_euclid(scale_len) as usize;
                            let note = scale.root_pitch as i32 + (octave * 12) + scale.intervals[degree] as i32;
                            note.clamp(0, 127) as u8
                        } else {
                            val.clamp(0, 127) as u8
                        }
                    }
                };

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
            // A rest breaks the chain of indices, meaning subsequent holds do nothing
            vec![]
        }
        Node::Hold => {
            if ctx.start_ms >= ctx.window_start_ms - 0.1 && ctx.start_ms < ctx.window_end_ms - 0.1 {
                for &idx in &ctx.active_chord_indices {
                    if let Some(note) = out_notes.get_mut(idx) {
                        note.duration_ms += ctx.duration_ms;
                    }
                }
                // Return the same indices so multiple holds (`_ _ _`) keep extending the same notes
                return ctx.active_chord_indices.clone();
            }
            vec![]
        }
        Node::Chord(elements) => {
            let mut indices = vec![];
            for el in elements {
                indices.extend(traverse_ast(el, ctx.clone(), out_notes));
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
                last_indices = traverse_ast(el, step_ctx, out_notes);
            }
            last_indices
        }
        Node::Parallel(layers) => {
            let mut all_indices = vec![];
            for layer in layers {
                all_indices.extend(traverse_ast(&Node::Sequence(layer.clone()), ctx.clone(), out_notes));
            }
            all_indices
        }
        Node::Alternator(elements) => {
            if elements.is_empty() { return vec![]; }
            let index = ctx.cycle_count % elements.len();
            traverse_ast(&elements[index], ctx, out_notes)
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
                    last_indices = traverse_ast(child, step_ctx, out_notes);
                } else {
                    last_indices = vec![]; // Rests interrupt the hold chain
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
                last_indices = traverse_ast(child, step_ctx, out_notes);
            }
            last_indices
        }
    }
}

pub fn generate_next_cycle(
    program: &Program, 
    bpm: f64, 
    cycle_start_time_ms: f64, 
    cycle_count: usize
) -> Vec<ScheduledNote> {
    if program.global_silence {
        return Vec::new();
    }

    let master_duration_ms = (60_000.0 / bpm) * 4.0; 
    let mut notes = Vec::new();

    for track in &program.tracks {
        if track.is_muted { continue; }
        
        let active_scale = if track.channel == 9 {
            track.scale.clone()
        } else {
            track.scale.clone().or(program.scale.clone())
        };

        let ctx = RenderContext {
            channel: track.channel,
            start_ms: cycle_start_time_ms,
            duration_ms: master_duration_ms,
            window_start_ms: cycle_start_time_ms,
            window_end_ms: cycle_start_time_ms + master_duration_ms,
            cycle_count,
            scale: active_scale, 
            active_chord_indices: vec![],
        };
        traverse_ast(&track.root_node, ctx, &mut notes);
    }
    
    notes
}
