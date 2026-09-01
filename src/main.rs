mod ast;
mod parser;
mod render;

use ast::{Program, ScheduledNote};
use parser::mmn_parser;
use render::generate_next_cycle;

use chumsky::Parser;
use midir::MidiOutput;
use midir::os::unix::VirtualOutput; //This line is critical to make the virtual output
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::fs;
use std::path::Path;
use std::sync::mpsc::{channel, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = channel::<Program>();
    let file_path = "live.mmn";

    if !Path::new(file_path).exists() {
        fs::write(file_path, "#BPM=120\nT1: C4 . D4 _\nT2: {C3 | G3}").expect("Failed to create initial file");
    }

    thread::spawn(move || {
        let (watch_tx, watch_rx) = channel();
        let mut watcher = RecommendedWatcher::new(
            watch_tx,
            Config::default().with_poll_interval(Duration::from_millis(50)),
        ).unwrap();

        let parent_dir = Path::new(file_path).parent().unwrap_or(Path::new(""));
        let watch_dir = if parent_dir.as_os_str().is_empty() { Path::new(".") } else { parent_dir };
        
        watcher.watch(watch_dir, RecursiveMode::NonRecursive).unwrap();
        println!("Listening for changes to {} in directory {:?}...", file_path, watch_dir);

        let parser = mmn_parser();

        if let Ok(contents) = fs::read_to_string(file_path) {
             if let Ok(initial_prog) = parser.parse(contents) {
                 let _ = tx.send(initial_prog);
             }
        }

        let mut last_update = Instant::now() - Duration::from_secs(1);

        for res in watch_rx {
            if let Ok(event) = res {
                let is_target_file = event.paths.iter().any(|p| p.ends_with(file_path));
                
                if is_target_file && !event.kind.is_access() {
                    if last_update.elapsed() < Duration::from_millis(100) {
                        continue;
                    }

                    thread::sleep(Duration::from_millis(15));
                    
                    if let Ok(contents) = fs::read_to_string(file_path) {
                        if contents.trim().is_empty() {
                            let empty_prog = Program { bpm: None, global_silence: true, tracks: vec![] };
                            if tx.send(empty_prog).is_ok() {
                                println!("File empty. Silencing all tracks.");
                                last_update = Instant::now();
                            }
                            continue;
                        }

                        match parser.parse(contents) {
                            Ok(new_prog) => {
                                if tx.send(new_prog).is_ok() {
                                    println!("Success! AST hot-swapped.");
                                    last_update = Instant::now();
                                }
                            }
                            Err(errs) => {
                                println!("Syntax Error! Continuing to play old sequence.");
                                for e in errs {
                                    let expected: Vec<_> = e.expected().cloned().collect();
                                    eprintln!("Expected {:?} at char {}", expected, e.span().start);
                                }
                                last_update = Instant::now();
                            }
                        }
                    }
                }
            }
        }
    });

    let midi_out = MidiOutput::new("Cycle MIDI Scheduler")?;
    let mut conn_out = midi_out.create_virtual("MMN Live Port")?;
    println!("Virtual MIDI Port 'MMN Live Port' created. Route it to your synth!");

    let mut bpm = 120.0;
    let mut cycle_duration_ms = (60_000.0 / bpm) * 4.0;
    let mut current_program = Program { bpm: None, global_silence: false, tracks: vec![] };
    let mut cycle_count = 0;
    
    let start_time = Instant::now();
    let mut next_cycle_start_ms = 0.0;
    
    let mut upcoming_notes: Vec<ScheduledNote> = Vec::new();
    let mut active_notes: Vec<(f64, u8, u8)> = Vec::new();

    println!("Starting Scheduler Loop...");

    loop {
        let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;

        match rx.try_recv() {
            Ok(new_prog) => {
                current_program = new_prog.clone();
                if let Some(new_bpm) = new_prog.bpm {
                    if (new_bpm - bpm).abs() > f64::EPSILON {
                        bpm = new_bpm;
                        cycle_duration_ms = (60_000.0 / bpm) * 4.0;
                        println!("BPM updated to: {}", bpm);
                    }
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => break,
        }

        if elapsed_ms >= next_cycle_start_ms {
            let mut new_notes = generate_next_cycle(
                &current_program,
                bpm,
                next_cycle_start_ms,
                cycle_count,
            );
            
            new_notes.sort_by(|a, b| b.start_ms.partial_cmp(&a.start_ms).unwrap());
            upcoming_notes = new_notes;
            
            next_cycle_start_ms += cycle_duration_ms;
            cycle_count += 1;
        }

        active_notes.retain(|&(off_time, channel, pitch)| {
            if elapsed_ms >= off_time {
                let _ = conn_out.send(&[0x80 | channel, pitch, 0]);
                false
            } else {
                true
            }
        });

        while let Some(next_note) = upcoming_notes.last() {
            if elapsed_ms >= next_note.start_ms {
                let note = upcoming_notes.pop().unwrap();
                let _ = conn_out.send(&[0x90 | note.channel, note.pitch, note.velocity]);
                active_notes.push((note.start_ms + note.duration_ms, note.channel, note.pitch));
            } else {
                break;
            }
        }

        let now_ms = start_time.elapsed().as_secs_f64() * 1000.0;
        let mut next_event_ms = next_cycle_start_ms;
        
        if let Some(note) = upcoming_notes.last() {
            if note.start_ms < next_event_ms {
                next_event_ms = note.start_ms;
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

    Ok(())
}
