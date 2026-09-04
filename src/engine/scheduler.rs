use crate::ast::{self, Program};
use super::render::{ScheduledEvent, generate_next_cycle};
use rtrb::Producer;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};
use thread_priority::*;

// MIDI Status Byte Constants
const MIDI_NOTE_OFF: u8 = 0x80;
const MIDI_NOTE_ON: u8 = 0x90;
const MIDI_CC: u8 = 0xB0;
const MIDI_CLOCK: u8 = 0xF8;
const MIDI_START: u8 = 0xFA;
const MIDI_STOP: u8 = 0xFC;
const MIDI_ALL_NOTES_OFF: u8 = 123;

macro_rules! send_midi {
    ($tx:expr, $msg:expr) => {
        let mut attempts = 0;
        while $tx.push($msg).is_err() && attempts < 10_000 {
            std::hint::spin_loop();
            attempts += 1;
        }
    };
}

pub fn run_scheduler(
    rx: Receiver<(String, Program)>,
    mut midi_tx: Producer<Vec<u8>>,
    running: Arc<AtomicBool>,
    default_quantize: ast::QuantizeMode,
    max_auto_quantize: usize,
) {
    let thread_id = thread_native_id();
    if let Err(e) = set_thread_priority_and_policy(
        thread_id,
        ThreadPriority::Max,
        ThreadSchedulePolicy::Realtime(RealtimeThreadSchedulePolicy::Fifo),
    ) {
        eprintln!(
            "Notice: Could not set SCHED_FIFO real-time policy (Requires elevated permissions on Linux). Jitter may occur: {:?}",
            e
        );
    } else {
        println!("Main timing loop elevated to SCHED_FIFO Real-Time priority!");
    }

    let mut bpm = 120.0;
    let mut cycle_duration_ms = (60_000.0 / bpm) * 4.0;
    let mut clock_interval_ms = 60_000.0 / (bpm * 24.0);

    let mut current_filename = String::new();
    let mut current_program = Program {
        bpm: None,
        quantize: None,
        scale: None,
        global_silence: false,
        tracks: vec![],
    };
    
    let mut staged_program: Option<(String, Program)> = None;
    let mut cycle_count = 0;

    println!("Waiting for initial AST compilation...");
    if let Ok((filename, initial_prog)) = rx.recv() {
        current_filename = filename;
        current_program = initial_prog;
        
        if let Some(new_bpm) = current_program.bpm {
            bpm = new_bpm;
            cycle_duration_ms = (60_000.0 / bpm) * 4.0;
            clock_interval_ms = 60_000.0 / (bpm * 24.0);
        }

        let initial_macro_len = current_program.pattern_length_cycles();
        println!(
            "Initial AST loaded ({}). Macro-cycle length is {} cycles. Sequence running...",
            current_filename, initial_macro_len
        );
    }

    let start_time = Instant::now();
    let mut next_cycle_start_ms = 0.0;
    let mut next_clock_ms = 0.0;

    let mut upcoming_events: Vec<ScheduledEvent> = Vec::new();
    let mut active_notes: Vec<(f64, u8, u8)> = Vec::new();

    println!("Starting Scheduler Loop...");
    
    // Send MIDI Start message to sync external sequencers/drum machines
    send_midi!(midi_tx, vec![MIDI_START]);

    loop {
        if !running.load(Ordering::SeqCst) {
            break;
        }

        let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;

        // Process MIDI Timing Clock (24 pulses per quarter note)
        while elapsed_ms >= next_clock_ms {
            send_midi!(midi_tx, vec![MIDI_CLOCK]);
            next_clock_ms += clock_interval_ms;
        }

        match rx.try_recv() {
            Ok((filename, new_prog)) => {
                let staged_macro_len = new_prog.pattern_length_cycles();
                println!(
                    "AST staged from {}! Macro-cycle length is {} cycles. Waiting for phrase boundary...",
                    filename, staged_macro_len
                );
                staged_program = Some((filename, new_prog));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => break,
        }

        if elapsed_ms >= next_cycle_start_ms {
            let mut transition_progress: Option<f64> = None;

            if let Some((staged_filename, staged)) = &staged_program {
                let is_hot_reload = *staged_filename == current_filename;
                let q_mode = staged.quantize.clone().unwrap_or(default_quantize.clone());

                let target_q_cycles = match q_mode {
                    ast::QuantizeMode::Fixed(n) => n,
                    ast::QuantizeMode::Auto => current_program.pattern_length_cycles().min(max_auto_quantize),
                };

                let position_in_phrase = cycle_count % target_q_cycles;

                if position_in_phrase == 0 {
                    let (filename, prog) = staged_program.take().unwrap();
                    current_program = prog;
                    current_filename = filename;

                    let calculated_len = current_program.pattern_length_cycles();
                    
                    if is_hot_reload {
                        println!(
                            "Hot reloaded {}! (Sequence loop length: {} cycles)",
                            current_filename, calculated_len
                        );
                    } else {
                        println!(
                            "Swapped to new pattern: {}! (Sequence loop length: {} cycles)",
                            current_filename, calculated_len
                        );
                        // THE DROP: Reset Expression/Volume to max on the new phrase!
                        for ch in 0..16 {
                            send_midi!(midi_tx, vec![MIDI_CC | ch, 11, 127]);
                        }
                    }

                    if let Some(new_bpm) = current_program.bpm {
                        if (new_bpm - bpm).abs() > f64::EPSILON {
                            bpm = new_bpm;
                            cycle_duration_ms = (60_000.0 / bpm) * 4.0;
                            clock_interval_ms = 60_000.0 / (bpm * 24.0);
                            println!("BPM updated to: {}", bpm);
                        }
                    }
                } else {
                    let cycles_left = target_q_cycles - position_in_phrase;

                    // ONLY fade if we are moving to a totally different file
                    if !is_hot_reload {
                        let prog = cycles_left as f64 / target_q_cycles as f64;
                        transition_progress = Some(prog);

                        let sweep_val = (prog * 127.0) as u8;
                        for ch in 0..16 {
                            send_midi!(midi_tx, vec![MIDI_CC | ch, 11, sweep_val]);
                        }

                        println!(
                            "Transitioning to {}... {} cycles left (Fade: {})",
                            staged_filename, cycles_left, sweep_val
                        );
                    } else {
                        println!(
                            "Hot reloading {} in {} cycles... (Waiting for full phrase length of {})",
                            staged_filename, cycles_left, target_q_cycles
                        );
                    }
                }
            }

            let pattern_len = current_program.pattern_length_cycles();

            let mut new_events = generate_next_cycle(
                &current_program,
                bpm,
                next_cycle_start_ms,
                cycle_count,
                pattern_len,
                transition_progress,
            );

            new_events.sort_by(|a, b| b.start_ms().partial_cmp(&a.start_ms()).unwrap());
            upcoming_events = new_events;

            next_cycle_start_ms += cycle_duration_ms;
            cycle_count += 1;
        }

        active_notes.retain(|&(off_time, channel, pitch)| {
            if elapsed_ms >= off_time {
                send_midi!(midi_tx, vec![MIDI_NOTE_OFF | channel, pitch, 0]);
                false
            } else {
                true
            }
        });

        while let Some(next_event) = upcoming_events.last() {
            if elapsed_ms >= next_event.start_ms() {
                let event = upcoming_events.pop().unwrap();

                match event {
                    ScheduledEvent::Note {
                        channel,
                        pitch,
                        velocity,
                        start_ms,
                        duration_ms,
                    } => {
                        // VOICE STEALING: Prevent overlapping/stuck notes
                        active_notes.retain(|&(_off_time, c, p)| {
                            if c == channel && p == pitch {
                                send_midi!(midi_tx, vec![MIDI_NOTE_OFF | c, p, 0]);
                                false
                            } else {
                                true
                            }
                        });

                        send_midi!(midi_tx, vec![MIDI_NOTE_ON | channel, pitch, velocity]);
                        active_notes.push((start_ms + duration_ms, channel, pitch));
                    }
                    ScheduledEvent::CC {
                        channel,
                        controller,
                        value,
                        ..
                    } => {
                        send_midi!(midi_tx, vec![MIDI_CC | channel, controller, value]);
                    }
                }
            } else {
                break;
            }
        }

        let now_ms = start_time.elapsed().as_secs_f64() * 1000.0;
        let mut next_wakeup_ms = next_cycle_start_ms;

        if let Some(event) = upcoming_events.last() {
            if event.start_ms() < next_wakeup_ms {
                next_wakeup_ms = event.start_ms();
            }
        }

        for &(off_time, _, _) in &active_notes {
            if off_time < next_wakeup_ms {
                next_wakeup_ms = off_time;
            }
        }
        
        // Ensure the thread wakes up in time to fire the next clock pulse
        if next_clock_ms < next_wakeup_ms {
            next_wakeup_ms = next_clock_ms;
        }

        let wait_ms = next_wakeup_ms - now_ms;

        if wait_ms > 3.0 {
            thread::sleep(Duration::from_millis(2));
        } else if wait_ms > 1.0 {
            thread::sleep(Duration::from_secs_f64((wait_ms - 0.5) / 1000.0));
        } else if wait_ms > 0.0 {
            std::hint::spin_loop();
        }
    }

    println!("Stopping playback and clearing active notes...");

    for &(_, channel, pitch) in &active_notes {
        send_midi!(midi_tx, vec![MIDI_NOTE_OFF | channel, pitch, 0]);
    }

    for ch in 0..16 {
        send_midi!(midi_tx, vec![MIDI_CC | ch, MIDI_ALL_NOTES_OFF, 0]);
    }

    // Send MIDI Stop message to sync external gear
    send_midi!(midi_tx, vec![MIDI_STOP]);

    send_midi!(midi_tx, vec![]);
    thread::sleep(Duration::from_millis(50));
}
