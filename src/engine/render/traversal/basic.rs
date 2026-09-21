use super::helpers::calculate_lfo_phase;
use crate::ast::{DynamicValue, Pitch};
use crate::engine::render::math::resolve_pitch;
use crate::engine::render::{RenderContext, ScheduledEvent};
use rand::RngExt;
use rand::rngs::StdRng;

pub(super) fn render_note(
    pitch: &Pitch,
    velocity: u8,
    gate: u8,
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
    rng: &mut StdRng,
) {
    if ctx.start_ms >= ctx.window_start_ms - 0.1 && ctx.start_ms < ctx.window_end_ms - 0.1 {
        let actual_pitch = resolve_pitch(pitch, &ctx.scale, ctx.octave_offset);
        let splits = ctx.ratchet_splits.max(1);
        let sub_step = ctx.duration_ms / splits as f64;
        let actual_duration = sub_step * (gate as f64 / 100.0);
        let base_vel = ctx.override_velocity.unwrap_or(velocity);
        let mut final_vel = (base_vel as f32 * ctx.velocity_modifier).clamp(0.0, 127.0) as u8;
        let mut play_note = true;

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
                    jitter = rng.random_range(
                        -ctx.humanize_timing_range_ms..=ctx.humanize_timing_range_ms,
                    );
                }

                let mut split_vel = final_vel;
                if ctx.humanize_velocity_range > 0 {
                    let offset = rng.random_range(
                        -(ctx.humanize_velocity_range as i32)
                            ..=(ctx.humanize_velocity_range as i32),
                    );
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

pub(super) fn render_cc(
    controller: u8,
    value: &DynamicValue,
    ctx: &mut RenderContext,
    out_events: &mut Vec<ScheduledEvent>,
    rng: &mut StdRng,
) {
    if ctx.start_ms >= ctx.window_start_ms - 0.1 && ctx.start_ms < ctx.window_end_ms - 0.1 {
        let splits = ctx.ratchet_splits.max(1);
        let sub_step = ctx.duration_ms / splits as f64;
        ctx.active_chord_indices.clear();

        for i in 0..splits {
            let mut jitter = 0.0;
            if ctx.humanize_timing_range_ms > 0.0 {
                jitter = rng.random_range(
                    -ctx.humanize_timing_range_ms..=ctx.humanize_timing_range_ms,
                );
            }
            let note_start = ctx.start_ms + (i as f64 * sub_step) + jitter;

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
                controller,
                value: actual_value,
                start_ms: note_start,
            });
        }
    } else {
        ctx.active_chord_indices.clear();
    }
}

pub(super) fn render_hold(ctx: &mut RenderContext, out_events: &mut Vec<ScheduledEvent>) {
    for &idx in &ctx.active_chord_indices {
        if let Some(event) = out_events.get_mut(idx) {
            if let ScheduledEvent::Note { duration_ms, .. } = event {
                *duration_ms += ctx.duration_ms;
            }
        }
    }
}
