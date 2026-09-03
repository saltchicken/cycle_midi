use crate::ast::{self, Program, ScheduledEvent};
use crate::render::generate_next_cycle;
use rtrb::Producer;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};
use thread_priority::*;

macro_rules! send_midi {
    ($tx:expr, $msg:expr) => {
        while $tx.push($msg).is_err() {
            std::hint::spin_loop();
        }
    };
}

pub fn run_scheduler(
    rx: Receiver<Program>,
    mut midi_tx: Producer<Vec<u8>>,
    running: Arc<AtomicBool>,
) {
    let thread_id = thread_native_id();
    if let Err(e) = set_thread_priority_and_policy(
        thread_id,
        ThreadPriority::Max,
        ThreadSchedulePolicy::Realtime(RealtimeThreadSchedulePolicy::Fifo)
    ) {
        eprintln!("Notice: Could not set SCHED_FIFO real-time policy (Requires elevated permissions on Linux). Jitter may occur: {:?}", e);
    } else {
        println!("Main timing loop elevated to SCHED_FIFO Real-Time priority!");
    }

    let mut bpm = 120.0;
    let mut cycle_duration_ms = (60_000.0 / bpm) * 4.0;
    
    let mut current_program = Program { bpm: None, quantize: None, scale: None, global_silence: false, tracks: vec![] };
    let mut staged_program: Option<Program> = None;
    let mut current_quantize = ast::QuantizeMode::Fixed(1);
    let mut cycle_count = 0;

    println!("Waiting for initial AST compilation...");
    if let Ok(initial_prog) = rx.recv() {
        current_program = initial_prog;
        if let Some(q) = &current_program.quantize {
            current_quantize = q.clone();
        }
        if let Some(new_bpm) = current_program.bpm {
            bpm = new_bpm;
            cycle_duration_ms = (60_000.0 / bpm) * 4.0;
        }
        
        let initial_macro_len = current_program.pattern_length_cycles();
        println!("Initial AST loaded. Macro-cycle length is {} cycles. Sequence running...", initial_macro_len);
    }
    
    let start_time = Instant::now();
    let mut next_cycle_start_ms = 0.0;
    
    let mut upcoming_events: Vec<ScheduledEvent> = Vec::new();
    let mut active_notes: Vec<(f64, u8, u8)> = Vec::new();

    println!("Starting Scheduler Loop...");

    loop {
        if !running.load(Ordering::SeqCst) {
            break; 
        }

        let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;

        match rx.try_recv() {
            Ok(new_prog) => {
                let staged_macro_len = new_prog.pattern_length_cycles();
                println!("AST staged! Macro-cycle length is {} cycles. Waiting for phrase boundary...", staged_macro_len);
                staged_program = Some(new_prog);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => break,
        }

        if elapsed_ms >= next_cycle_start_ms {
            if let Some(staged) = &staged_program {
                
                let q_mode = staged.quantize.clone().unwrap_or(current_quantize.clone());
                
                let target_q_cycles = match q_mode {
                    ast::QuantizeMode::Fixed(n) => n,
                    ast::QuantizeMode::Auto => current_program.pattern_length_cycles(),
                };

                let position_in_phrase = cycle_count % target_q_cycles;
                
                if position_in_phrase == 0 {
                    current_program = staged_program.take().unwrap();
                    current_quantize = q_mode;
                    
                    let calculated_len = current_program.pattern_length_cycles();
                    println!("Swapped to new pattern! (Quantize: {:?} - Sequence loop length: {} cycles)", 
                        current_quantize, calculated_len);

                    if let Some(new_bpm) = current_program.bpm {
                        if (new_bpm - bpm).abs() > f64::EPSILON {
                            bpm = new_bpm;
                            cycle_duration_ms = (60_000.0 / bpm) * 4.0;
                            println!("BPM updated to: {}", bpm);
                        }
                    }
                } else {
                    let cycles_left = target_q_cycles - position_in_phrase;
                    println!("Swapping in {} cycles... (Waiting for full phrase length of {})", 
                        cycles_left, target_q_cycles);
                }
            }

            let pattern_len = current_program.pattern_length_cycles();

            let mut new_events = generate_next_cycle(
                &current_program,
                bpm,
                next_cycle_start_ms,
                cycle_count,
                pattern_len,
            );
            
            new_events.sort_by(|a, b| b.start_ms().partial_cmp(&a.start_ms()).unwrap());
            upcoming_events = new_events;
            
            next_cycle_start_ms += cycle_duration_ms;
            cycle_count += 1;
        }

        active_notes.retain(|&(off_time, channel, pitch)| {
            if elapsed_ms >= off_time {
                send_midi!(midi_tx, vec![0x80 | channel, pitch, 0]);
                false
            } else {
                true
            }
        });

        while let Some(next_event) = upcoming_events.last() {
            if elapsed_ms >= next_event.start_ms() {
                let event = upcoming_events.pop().unwrap();

                match event {
                    ScheduledEvent::Note { channel, pitch, velocity, start_ms, duration_ms } => {
                        // VOICE STEALING: Prevent overlapping/stuck notes
                        active_notes.retain(|&(_off_time, c, p)| {
                            if c == channel && p == pitch {
                                send_midi!(midi_tx, vec![0x80 | c, p, 0]);
                                false 
                            } else {
                                true
                            }
                        });

                        send_midi!(midi_tx, vec![0x90 | channel, pitch, velocity]);
                        active_notes.push((start_ms + duration_ms, channel, pitch));
                    }
                    ScheduledEvent::CC { channel, controller, value, .. } => {
                        send_midi!(midi_tx, vec![0xB0 | channel, controller, value]);
                    }
                }
            } else {
                break;
            }
        }

        let now_ms = start_time.elapsed().as_secs_f64() * 1000.0;
        let mut next_event_ms = next_cycle_start_ms;
        
        if let Some(event) = upcoming_events.last() {
            if event.start_ms() < next_event_ms {
                next_event_ms = event.start_ms();
            }
        }
        
        for &(off_time, _, _) in &active_notes {
            if off_time < next_event_ms {
                next_event_ms = off_time;
            }
        }

        let wait_ms = next_event_ms - now_ms;

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
        send_midi!(midi_tx, vec![0x80 | channel, pitch, 0]);
    }

    for ch in 0..16 {
        send_midi!(midi_tx, vec![0xB0 | ch, 123, 0]); 
    }

    send_midi!(midi_tx, vec![]);
    thread::sleep(Duration::from_millis(50));
}
