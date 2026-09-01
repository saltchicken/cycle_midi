use crate::ast::{Node, RenderContext, ScheduledNote, Program};

pub fn traverse_ast(node: &Node, ctx: RenderContext, out_notes: &mut Vec<ScheduledNote>) {
    match node {
        Node::Note { pitch, velocity, gate, .. } => {
            let actual_duration = ctx.duration_ms * (*gate as f64 / 100.0);
            out_notes.push(ScheduledNote {
                channel: ctx.channel,
                pitch: *pitch,
                velocity: *velocity,
                start_ms: ctx.start_ms,
                duration_ms: actual_duration,
            });
        }
        Node::Rest => {}
        Node::Hold => {
            if let Some(last_note) = out_notes.last_mut() {
                last_note.duration_ms += ctx.duration_ms;
            }
        }
        Node::Chord(elements) => {
            for el in elements {
                traverse_ast(el, ctx.clone(), out_notes);
            }
        }
        Node::Sequence(elements) => {
            if elements.is_empty() { return; }
            let step_duration = ctx.duration_ms / elements.len() as f64;
            for (i, el) in elements.iter().enumerate() {
                let mut step_ctx = ctx.clone();
                step_ctx.start_ms = ctx.start_ms + (i as f64 * step_duration);
                step_ctx.duration_ms = step_duration;
                traverse_ast(el, step_ctx, out_notes);
            }
        }
        Node::Parallel(layers) => {
            for layer in layers {
                traverse_ast(&Node::Sequence(layer.clone()), ctx.clone(), out_notes);
            }
        }
        Node::Alternator(elements) => {
            if elements.is_empty() { return; }
            let index = ctx.cycle_count % elements.len();
            traverse_ast(&elements[index], ctx, out_notes);
        }
        Node::Euclidean(child, pulses, steps) => {
            if *steps == 0 || *pulses == 0 { return; }
            let step_duration = ctx.duration_ms / *steps as f64;
            for i in 0..*steps {
                let is_hit = ((i as usize * *pulses as usize) % (*steps as usize)) < (*pulses as usize);
                if is_hit {
                    let mut step_ctx = ctx.clone();
                    step_ctx.start_ms = ctx.start_ms + (i as f64 * step_duration);
                    step_ctx.duration_ms = step_duration;
                    traverse_ast(child, step_ctx, out_notes);
                }
            }
        }
        Node::SpeedModifier(child, multiplier) => {
            let repeats = multiplier.max(1.0) as usize; 
            let step_duration = ctx.duration_ms / *multiplier as f64;
            for i in 0..repeats {
                let mut step_ctx = ctx.clone();
                step_ctx.start_ms = ctx.start_ms + (i as f64 * step_duration);
                step_ctx.duration_ms = step_duration;
                traverse_ast(child, step_ctx, out_notes);
            }
        }
    }
}

pub fn generate_next_cycle(
    program: &Program, 
    bpm: f64, 
    cycle_start_time_ms: f64, 
    cycle_count: usize
) -> Vec<ScheduledNote> {
    let master_duration_ms = (60_000.0 / bpm) * 4.0; // 1 Bar in 4/4
    
    let mut notes = Vec::new();

    for track in &program.tracks {
        let ctx = RenderContext {
            channel: track.channel,
            start_ms: cycle_start_time_ms,
            duration_ms: master_duration_ms,
            cycle_count,
        };
        traverse_ast(&track.root_node, ctx, &mut notes);
    }
    
    notes
}
