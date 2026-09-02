mod ast;
mod parser;
mod render;

use ast::{Program, ScheduledNote};
use parser::mmn_parser;
use render::generate_next_cycle;

use chumsky::Parser;
use midir::MidiOutput;
use midir::os::unix::VirtualOutput; 
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::mpsc::{channel, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Deserialize)]
struct AppConfig {
    mmn_directory: String,
    midi_port: Option<String>,
}

/// A simple helper to expand `~/` into the user's actual home directory
fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        if let Some(mut home) = dirs::home_dir() {
            home.push(&path[2..]);
            return home;
        }
    }
    PathBuf::from(path)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --- CONFIGURATION & WORKSPACE SETUP ---
    let config_dir = dirs::config_dir()
        .expect("Could not find user config directory")
        .join("cycle_midi");

    if !config_dir.exists() {
        fs::create_dir_all(&config_dir).expect("Failed to create cycle_midi config directory");
    }

    let config_path = config_dir.join("config.toml");

    // Auto-generate a default configuration file if one doesn't exist
    if !config_path.exists() {
        let default_workspace = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("cycle_midi_workspace");

        let default_config_content = format!(
            "# cycle_midi configuration\n# Specify the absolute path or use ~/ for your home directory\nmmn_directory = \"{}\"\n# Optional: Specify a default MIDI output port name to connect to\n# midi_port = \"Midi Through Port-0\"\n",
            default_workspace.display()
        );
        fs::write(&config_path, default_config_content).expect("Failed to write default config.toml");
        println!("Created default configuration file at: {}", config_path.display());
    }

    // Read and parse the configuration
    let config_str = fs::read_to_string(&config_path).expect("Failed to read config.toml");
    let config: AppConfig = toml::from_str(&config_str).expect("Failed to parse config.toml");

    let mmn_dir = expand_tilde(&config.mmn_directory);
    if !mmn_dir.exists() {
        fs::create_dir_all(&mmn_dir).expect("Failed to create designated MMN directory");
        println!("Created MMN workspace directory at: {}", mmn_dir.display());
    }

    let file_path = mmn_dir.join("live.mmn");
    // ----------------------------------------
    
    // Set up the graceful shutdown flag and Ctrl-C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        println!("\nReceived shutdown signal! Cleaning up MIDI notes...");
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");

    let (tx, rx) = channel::<Program>();

    if !file_path.exists() {
        fs::write(&file_path, "#BPM=120\n#SCALE=C4 minor\nT1: 0 2 3 4 . 7 _\nT2(G3 minor_pentatonic): {-7 | 0}").expect("Failed to create initial file");
    }

    let thread_file_path = file_path.clone();
    let thread_watch_dir = mmn_dir.clone();

    thread::spawn(move || {
        let (watch_tx, watch_rx) = channel();
        let mut watcher = RecommendedWatcher::new(
            watch_tx,
            Config::default().with_poll_interval(Duration::from_millis(50)),
        ).unwrap();

        watcher.watch(&thread_watch_dir, RecursiveMode::NonRecursive).unwrap();
        println!("Listening for changes to {} in directory {}...", thread_file_path.display(), thread_watch_dir.display());

        let parser = mmn_parser();

        // Send the initial parse to the main thread immediately
        if let Ok(contents) = fs::read_to_string(&thread_file_path) {
             if let Ok(initial_prog) = parser.parse(contents) {
                 let _ = tx.send(initial_prog);
             }
        }

        let mut last_update = Instant::now() - Duration::from_secs(1);

        for res in watch_rx {
            if let Ok(event) = res {
                let is_target_file = event.paths.iter().any(|p| p == &thread_file_path);
                
                if is_target_file && !event.kind.is_access() {
                    if last_update.elapsed() < Duration::from_millis(100) {
                        continue;
                    }

                    thread::sleep(Duration::from_millis(15));
                    
                    if let Ok(contents) = fs::read_to_string(&thread_file_path) {
                        if contents.trim().is_empty() {
                            let empty_prog = Program { bpm: None, quantize: None, scale: None, global_silence: true, tracks: vec![] };
                            if tx.send(empty_prog).is_ok() {
                                println!("File empty. Silencing all tracks.");
                                last_update = Instant::now();
                            }
                            continue;
                        }

                        match parser.parse(contents) {
                            Ok(new_prog) => {
                                if tx.send(new_prog).is_ok() {
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

    let mut midi_out = MidiOutput::new("Cycle MIDI Scheduler")?;

    let mut conn_out = 'setup: {
        if let Some(target_port) = &config.midi_port {
            let ports = midi_out.ports();
            let mut found_port = None;
            for p in &ports {
                if let Ok(name) = midi_out.port_name(p) {
                    if name.to_lowercase().contains(&target_port.to_lowercase()) {
                        found_port = Some(p.clone());
                        println!("Found matching MIDI port: {}", name);
                        break;
                    }
                }
            }

            if let Some(p) = found_port {
                match midi_out.connect(&p, "Cycle MIDI Out") {
                    Ok(conn) => {
                        println!("Successfully connected to designated MIDI port!");
                        break 'setup conn; // Return the connection instantly
                    }
                    Err(e) => {
                        eprintln!("Failed to connect to MIDI port: {}", e);
                        // midir connect consumes the interface on failure, so we unwrap it back out for the fallback
                        midi_out = e.into_inner();
                    }
                }
            } else {
                println!("MIDI port '{}' not found in available ports.", target_port);
            }
        }

        // The fallback only happens if no config was set, port wasn't found, or connection failed
        println!("Falling back to Virtual MIDI Port.");
        let conn = midi_out.create_virtual("MMN Live Port")?;
        println!("Virtual MIDI Port 'MMN Live Port' created. Route it to your synth!");
        conn
    };

    let mut bpm = 120.0;
    let mut cycle_duration_ms = (60_000.0 / bpm) * 4.0;
    
    // Core Engine State
    let mut current_program = Program { bpm: None, quantize: None, scale: None, global_silence: false, tracks: vec![] };
    let mut staged_program: Option<Program> = None;
    let mut current_quantize = 1;
    let mut cycle_count = 0;

    // Wait synchronously for the first AST to finish compiling before starting the clock
    println!("Waiting for initial AST compilation...");
    if let Ok(initial_prog) = rx.recv() {
        current_program = initial_prog;
        if let Some(q) = current_program.quantize {
            current_quantize = q;
        }
        if let Some(new_bpm) = current_program.bpm {
            bpm = new_bpm;
            cycle_duration_ms = (60_000.0 / bpm) * 4.0;
        }
        println!("Initial AST loaded. Sequence running...");
    }
    
    // NOW we start the clock!
    let start_time = Instant::now();
    let mut next_cycle_start_ms = 0.0;
    
    let mut upcoming_notes: Vec<ScheduledNote> = Vec::new();
    let mut active_notes: Vec<(f64, u8, u8)> = Vec::new();

    println!("Starting Scheduler Loop...");

    loop {
        // Exit condition
        if !running.load(Ordering::SeqCst) {
            break; 
        }

        let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;

        match rx.try_recv() {
            Ok(new_prog) => {
                // Since the first AST is already loaded, anything received here is a live edit
                println!("AST staged! Waiting for phrase boundary...");
                staged_program = Some(new_prog);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => break,
        }

        if elapsed_ms >= next_cycle_start_ms {
            
            // Check for staged pattern swap
            if let Some(staged) = &staged_program {
                let target_q = staged.quantize.unwrap_or(current_quantize);
                let position_in_phrase = cycle_count % target_q;
                
                if position_in_phrase == 0 {
                    current_program = staged_program.take().unwrap();
                    current_quantize = target_q;
                    println!("Swapped to new pattern! (Quantize: {} cycles)", current_quantize);

                    // Apply BPM if it changed
                    if let Some(new_bpm) = current_program.bpm {
                        if (new_bpm - bpm).abs() > f64::EPSILON {
                            bpm = new_bpm;
                            cycle_duration_ms = (60_000.0 / bpm) * 4.0;
                            println!("BPM updated to: {}", bpm);
                        }
                    }
                } else {
                    let cycles_left = target_q - position_in_phrase;
                    println!("Swapping in {}...", cycles_left);
                }
            }

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

    println!("Stopping playback and clearing active notes...");
    
    // Explicitly turn off any notes we know are currently playing
    for &(_, channel, pitch) in &active_notes {
        let _ = conn_out.send(&[0x80 | channel, pitch, 0]);
    }

    // Safety net: Send standard MIDI CC 123 (All Notes Off) on all 16 channels
    for ch in 0..16 {
        let _ = conn_out.send(&[0xB0 | ch, 123, 0]); 
        // Also send CC 120 (All Sound Off) for good measure, which kills reverb/release tails
        // let _ = conn_out.send(&[0xB0 | ch, 120, 0]); 
    }

    println!("Graceful shutdown complete.");
    Ok(())
}
