pub mod math;
pub mod traversal;

use crate::ast::{Program, RenderContext, ScheduledEvent, SeedInterval};
use rand::SeedableRng;
use rand::rngs::StdRng;
use traversal::traverse_ast;

pub fn generate_next_cycle(
    program: &Program, 
    bpm: f64, 
    cycle_start_time_ms: f64, 
    cycle_count: usize,
    macro_cycle_length: usize,
) -> Vec<ScheduledEvent> {
    if program.global_silence {
        return Vec::new();
    }

    let master_duration_ms = (60_000.0 / bpm) * 4.0; 
    let mut events = Vec::new();
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
            master_duration_ms,
            scale: active_scale, 
            active_chord_indices: vec![],
            octave_offset: track.octave_offset,
            alternator_stride: 1,
        };
        traverse_ast(&track.root_node, ctx, &mut events, &mut rng);
    }
    
    events
}
