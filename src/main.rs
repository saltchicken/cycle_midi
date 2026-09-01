mod ast;
mod parser;
mod render;

use ast::{Node, ScheduledNote};
use parser::mmn_parser;
use render::generate_next_cycle;

use chumsky::Parser;
use midir::MidiOutput;
use midir::os::unix::VirtualOutput; // Fix: Import Unix virtual port trait
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::fs;
use std::path::Path;
use std::sync::mpsc::{channel, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = channel::<Node>();
    let file_path = "live.mmn";

    // Setup initial file
    if !Path::new(file_path).exists() {
        fs::write(file_path, "C4 . D4 _").expect("Failed to create initial file");
    }

    // Spawn Watcher Thread
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

        if let Ok(contents) = fs::read_to_string(file_path) {
             if let Ok(initial_ast) = mmn_parser().parse(contents) {
                 let _ = tx.send(initial_ast);
             }
        }

        // 1. Initialize a debounce timer (set in the past so the first save works)
        let mut last_update = Instant::now() - Duration::from_secs(1);

        for res in watch_rx {
            if let Ok(event) = res {
                let is_target_file = event.paths.iter().any(|p| p.ends_with(file_path));
                
                if is_target_file && !event.kind.is_access() {
                    
                    // 2. Debounce: If it's been less than 100ms since our last parse, ignore this event
                    if last_update.elapsed() < Duration::from_millis(100) {
                        continue;
                    }

                    thread::sleep(Duration::from_millis(15));
                    
                    if let Ok(contents) = fs::read_to_string(file_path) {
                        if contents.trim().is_empty() { continue; }

                        match mmn_parser().parse(contents) {
                            Ok(new_ast) => {
                                if tx.send(new_ast).is_ok() {
                                    println!("Success! AST hot-swapped.");
                                    // 3. Reset the timer on a successful swap
                                    last_update = Instant::now();
                                }
                            }
                            Err(errs) => {
                                println!("Syntax Error! Continuing to play old sequence.");
                                for e in errs {
                                    let expected: Vec<_> = e.expected().cloned().collect();
                                    eprintln!("Expected {:?} at char {}", expected, e.span().start);
                                }
                                // 4. Reset the timer on an error too, so we don't spam 5 syntax errors
                                last_update = Instant::now();
                            }
                        }
                    }
                }
            }
        }
    });

    // MIDI Output Setup
    let midi_out = MidiOutput::new("Cycle MIDI Scheduler")?;
    let mut conn_out = midi_out.create_virtual("MMN Live Port")?;
    println!("Virtual MIDI Port 'MMN Live Port' created. Route it to your synth!");

    // Scheduler State
    let bpm = 120.0;
    let cycle_duration_ms = (60_000.0 / bpm) * 4.0;
    let mut current_ast = Node::Sequence(vec![]);
    let mut cycle_count = 0;
    
    let start_time = Instant::now();
    let mut next_cycle_start_ms = 0.0;
    
    let mut upcoming_notes: Vec<ScheduledNote> = Vec::new();
    let mut active_notes: Vec<(f64, u8)> = Vec::new(); // (turn_off_time_ms, pitch)

    println!("Starting Scheduler Loop...");

    // The Real-Time Loop
    loop {
        let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;

        // 1. Hot-Swap AST
        match rx.try_recv() {
            Ok(new_ast) => current_ast = new_ast,
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => break,
        }

        // 2. Generate Next Cycle if we reached the boundary
        if elapsed_ms >= next_cycle_start_ms {
            let mut new_notes = generate_next_cycle(
                &current_ast,
                bpm,
                next_cycle_start_ms,
                cycle_count,
            );
            
            // Sort chronologically descending so we can `.pop()` from the end efficiently
            new_notes.sort_by(|a, b| b.start_ms.partial_cmp(&a.start_ms).unwrap());
            upcoming_notes = new_notes;
            
            next_cycle_start_ms += cycle_duration_ms;
            cycle_count += 1;
        }

        // 3. Process Note Offs
        active_notes.retain(|&(off_time, pitch)| {
            if elapsed_ms >= off_time {
                let _ = conn_out.send(&[0x80, pitch, 0]);
                false
            } else {
                true
            }
        });

        // 4. Process Note Ons
        while let Some(next_note) = upcoming_notes.last() {
            if elapsed_ms >= next_note.start_ms {
                let note = upcoming_notes.pop().unwrap();
                let _ = conn_out.send(&[0x90, note.pitch, note.velocity]);
                active_notes.push((note.start_ms + note.duration_ms, note.pitch));
            } else {
                break; // Next note is in the future
            }
        }

        // Keep loop tight without maxing CPU
        thread::sleep(Duration::from_millis(1)); 
    }

    Ok(())
}
