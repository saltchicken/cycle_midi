pub mod math;
pub mod traversal;

use crate::ast::{Program, ScaleDef, SeedInterval};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use traversal::traverse_ast;

#[derive(Debug, Clone)]
pub enum ScheduledEvent {
    Note {
        channel: u8,
        pitch: u8,
        velocity: u8,
        start_ms: f64,
        duration_ms: f64,
    },
    CC {
        channel: u8,
        controller: u8,
        value: u8,
        start_ms: f64,
    },
}

impl ScheduledEvent {
    pub fn start_ms(&self) -> f64 {
        match self {
            Self::Note { start_ms, .. } => *start_ms,
            Self::CC { start_ms, .. } => *start_ms,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderContext {
    pub track_seed: u64,
    pub channel: u8,
    pub start_ms: f64,
    pub duration_ms: f64,
    pub window_start_ms: f64,
    pub window_end_ms: f64,
    pub cycle_count: usize,
    pub master_duration_ms: f64,
    pub cycle_start_ms: f64,
    pub scale: Option<ScaleDef>,
    pub active_chord_indices: Vec<usize>,
    pub octave_offset: i32,
    pub alternator_stride: usize,
    pub transition_fade: Option<f64>,
    pub velocity_modifier: f32,
    pub override_velocity: Option<u8>,
    pub override_gate: Option<u8>,
    pub ratchet_splits: usize,
    pub humanize_velocity_range: u8,
    pub humanize_timing_range_ms: f64,
    pub max_events: usize,
}

impl RenderContext {
    pub fn is_in_window(&self, time_ms: f64) -> bool {
        time_ms >= self.window_start_ms - 0.1 && time_ms < self.window_end_ms - 0.1
    }

    pub fn derive_step(&self, index: usize, step_duration: f64) -> Self {
        let mut step_ctx = self.clone();
        step_ctx.start_ms = self.start_ms + (index as f64 * step_duration);
        step_ctx.duration_ms = step_duration;
        step_ctx.window_start_ms = self.window_start_ms.max(step_ctx.start_ms);
        step_ctx.window_end_ms = self.window_end_ms.min(step_ctx.start_ms + step_duration);
        step_ctx
    }
}

pub fn generate_next_cycle(
    program: &Program,
    bpm: f64,
    cycle_start_time_ms: f64,
    cycle_count: usize,
    macro_cycle_length: usize,
    transition_fade: Option<f64>,
) -> Vec<ScheduledEvent> {
    if program.global_silence {
        return Vec::new();
    }

    let (num, den) = program.signature.unwrap_or((4, 4));
    let beats_per_cycle = num as f64 * (4.0 / den as f64);
    let master_duration_ms = (60_000.0 / bpm) * beats_per_cycle;

    let mut events = Vec::new();
    let macro_cycle_count = cycle_count / macro_cycle_length.max(1);

    let mut active_global_scale = program.scale.clone();

    if let Some(seq) = &program.scale_seq {
        let max_end = seq.iter().map(|s| s.1).max().unwrap_or(1).max(1);
        let loop_cycle = cycle_count % max_end;

        for (start, end, scale) in seq {
            if loop_cycle >= *start && loop_cycle < *end {
                active_global_scale = Some(scale.clone());
                break;
            }
        }
    }

    for track in &program.tracks {
        if track.is_muted {
            continue;
        }

        let active_scale = if track.channel == 9 {
            track.scale.clone()
        } else {
            track.scale.clone().or(active_global_scale.clone())
        };

        let final_seed = if let Some(seed_def) = &track.seed {
            let mut s = seed_def.base;
            if let Some(interval) = &seed_def.interval {
                let seed_bump = match interval {
                    SeedInterval::Macro(m) => (macro_cycle_count / *m) as u64,
                    SeedInterval::Track(t) => {
                        let track_len = track.root_node.cycle_length().max(1);
                        ((cycle_count / track_len) / *t) as u64
                    }
                    SeedInterval::Micro(m) => (cycle_count / *m) as u64,
                };
                s = s.wrapping_add(seed_bump);
            }
            s
        } else {
            // Derive a stable seed per track based on cycle and channel
            let mut hasher = DefaultHasher::new();
            track.channel.hash(&mut hasher);
            cycle_count.hash(&mut hasher);
            hasher.finish()
        };

        let mut ctx = RenderContext {
            track_seed: final_seed,
            channel: track.channel,
            start_ms: cycle_start_time_ms,
            duration_ms: master_duration_ms,
            window_start_ms: cycle_start_time_ms,
            window_end_ms: cycle_start_time_ms + master_duration_ms,
            cycle_count,
            master_duration_ms,
            cycle_start_ms: cycle_start_time_ms,
            scale: active_scale,
            active_chord_indices: vec![],
            octave_offset: track.octave_offset,
            alternator_stride: 1,
            transition_fade,
            velocity_modifier: 1.0,
            override_velocity: None,
            override_gate: None,
            ratchet_splits: 1,
            humanize_velocity_range: 0,
            humanize_timing_range_ms: 0.0,
            max_events: 1024,
        };

        traverse_ast(&track.root_node, &mut ctx, &mut events);
    }

    events
}
