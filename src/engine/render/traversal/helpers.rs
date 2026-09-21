use crate::engine::render::RenderContext;

pub(super) fn calculate_lfo_phase(ctx: &RenderContext, speed: f64) -> f64 {
    let lfo_duration = ctx.master_duration_ms / speed;
    let theoretical_cycle_start = ctx.cycle_count as f64 * ctx.master_duration_ms;
    let offset = ctx.start_ms - ctx.cycle_start_ms;
    let virtual_time = theoretical_cycle_start + offset;
    (virtual_time % lfo_duration) / lfo_duration
}

pub(super) fn render_phase_chunks<F>(
    ctx: &RenderContext,
    wrap_duration: f64,
    shift_ms: f64,
    new_master_duration: Option<f64>,
    mut render_fn: F,
) -> Vec<usize>
where
    F: FnMut(&mut RenderContext),
{
    let theoretical_cycle_start = ctx.cycle_count as f64 * ctx.master_duration_ms;
    let offset_in_cycle = ctx.start_ms - ctx.cycle_start_ms;
    let virtual_start_ms = theoretical_cycle_start + offset_in_cycle - shift_ms;

    let phase_offset = (virtual_start_ms + 1e-9).rem_euclid(wrap_duration);
    let chunk_start_ms = ctx.start_ms - phase_offset;
    let chunks_to_render = (ctx.duration_ms / wrap_duration).ceil() as usize + 2;

    let master_dur = new_master_duration.unwrap_or(ctx.master_duration_ms);
    let mut all_indices = Vec::new();

    for i in 0..chunks_to_render {
        let absolute_chunk_start = chunk_start_ms + (i as f64 * wrap_duration);
        let mut chunk_ctx = ctx.clone();
        chunk_ctx.start_ms = absolute_chunk_start;
        chunk_ctx.duration_ms = wrap_duration;

        chunk_ctx.window_start_ms = ctx.window_start_ms.max(absolute_chunk_start);
        chunk_ctx.window_end_ms = ctx.window_end_ms.min(absolute_chunk_start + wrap_duration);

        chunk_ctx.master_duration_ms = master_dur;
        chunk_ctx.cycle_start_ms = absolute_chunk_start;

        let virtual_chunk_start = virtual_start_ms - phase_offset + (i as f64 * wrap_duration);
        chunk_ctx.cycle_count = (virtual_chunk_start / master_dur).floor().max(0.0) as usize;

        render_fn(&mut chunk_ctx);
        all_indices.extend_from_slice(&chunk_ctx.active_chord_indices);
    }

    all_indices
}
