use chumsky::prelude::*;
use midir::MidiOutput;
use midir::os::unix::VirtualOutput; // Fix: Import Unix virtual port trait
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::fs;
use std::path::Path;
use std::sync::mpsc::{channel, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

// --- 1. Abstract Syntax Tree & Render Context ---

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Note { pitch: u8, velocity: u8, gate: u8, prob: u8 },
    Chord(Vec<Node>),
    Rest,
    Hold,
    Sequence(Vec<Node>),
    Parallel(Vec<Vec<Node>>),
    Euclidean(Box<Node>, u8, u8),
    Alternator(Vec<Node>),
    SpeedModifier(Box<Node>, f32),
}

#[derive(Debug, Clone)]
pub struct ScheduledNote {
    pub pitch: u8,
    pub velocity: u8,
    pub start_ms: f64,
    pub duration_ms: f64,
}

#[derive(Debug, Clone)]
pub struct RenderContext {
    pub start_ms: f64,
    pub duration_ms: f64,
    pub cycle_count: usize,
}

// --- 2. Chumsky Parser ---

pub fn mmn_parser() -> impl Parser<char, Node, Error = Simple<char>> {
    recursive(|expr| {
        let rest = just('.').to(Node::Rest);
        let hold = just('_').to(Node::Hold);

        let int_u8 = text::int::<char, Simple<char>>(10)
            .map(|s: String| s.parse::<u8>().unwrap());
        
        let float = text::int(10)
            .chain::<char, _, _>(just('.').chain(text::digits(10)).or_not().flatten())
            .collect::<String>()
            .map(|s| s.parse::<f32>().unwrap());

        let note_name = choice((
            just("C#"), just("Db"), just("D#"), just("Eb"),
            just("F#"), just("Gb"), just("G#"), just("Ab"),
            just("A#"), just("Bb"),
            just("C"), just("D"), just("E"), just("F"),
            just("G"), just("A"), just("B")
        ));

        let pitch_str = note_name
            .then(text::int(10).map(|s: String| s.parse::<i32>().unwrap()))
            .map(|(n, oct)| {
                let base = match n {
                    "C" => 0, "C#" | "Db" => 1, "D" => 2, "D#" | "Eb" => 3,
                    "E" => 4, "F" => 5, "F#" | "Gb" => 6, "G" => 7,
                    "G#" | "Ab" => 8, "A" => 9, "A#" | "Bb" => 10, "B" => 11,
                    _ => 0,
                };
                ((oct + 1) * 12 + base).clamp(0, 127) as u8
            });

        let pitch = pitch_str.or(int_u8);
        let velocity = just('@').ignore_then(int_u8);
        let gate = just('%').ignore_then(int_u8);
        let prob = just('?').ignore_then(int_u8);

        let chord_or_note = pitch
            .separated_by(just('+'))
            .at_least(1)
            .then(velocity.or_not())
            .then(gate.or_not())
            .then(prob.or_not())
            .map(|(((pitches, v), g), pr)| {
                let notes: Vec<Node> = pitches.into_iter().map(|p| Node::Note {
                    pitch: p,
                    velocity: v.unwrap_or(100),
                    gate: g.unwrap_or(100),
                    prob: pr.unwrap_or(100),
                }).collect();
                
                if notes.len() == 1 {
                    notes.into_iter().next().unwrap()
                } else {
                    Node::Chord(notes)
                }
            });

        let seq_group = expr.clone()
            .padded()
            .repeated()
            .delimited_by(just('['), just(']'))
            .map(Node::Sequence);

        let alt_group = expr.clone()
            .padded()
            .repeated()
            .delimited_by(just('<'), just('>'))
            .map(Node::Alternator);

        let parallel_layer = expr.clone()
            .padded()
            .repeated();

        let parallel_group = parallel_layer
            .separated_by(just('|'))
            .delimited_by(just('{'), just('}'))
            .map(Node::Parallel);

        let atom = choice((
            rest, hold, seq_group, alt_group, parallel_group, chord_or_note,
        ));

        enum Postfix {
            Euclidean(u8, u8),
            Mul(f32),
            Div(f32),
        }

        let euclidean = just('(')
            .ignore_then(int_u8)
            .then_ignore(just(','))
            .then(int_u8)
            .then_ignore(just(')'))
            .map(|(p, s)| Postfix::Euclidean(p, s));

        let speed_mul = just('*').ignore_then(float).map(Postfix::Mul);
        let speed_div = just('/').ignore_then(float).map(Postfix::Div);

        let postfix = choice((euclidean, speed_mul, speed_div));

        atom.then(postfix.repeated()).map(|(base, postfixes)| {
            postfixes.into_iter().fold(base, |acc, post| match post {
                Postfix::Euclidean(p, s) => Node::Euclidean(Box::new(acc), p, s),
                Postfix::Mul(val) => Node::SpeedModifier(Box::new(acc), val),
                Postfix::Div(val) => Node::SpeedModifier(Box::new(acc), 1.0 / val),
            })
        })
    })
    .padded()
    .repeated()
    .map(Node::Sequence)
    .then_ignore(end())
}

// --- 3. AST Traversal ---

pub fn traverse_ast(node: &Node, ctx: RenderContext, out_notes: &mut Vec<ScheduledNote>) {
    match node {
        Node::Note { pitch, velocity, gate, .. } => {
            let actual_duration = ctx.duration_ms * (*gate as f64 / 100.0);
            out_notes.push(ScheduledNote {
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
                // Fix: Added parentheses around the math to prevent generic parsing errors
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
    ast: &Node, 
    bpm: f64, 
    cycle_start_time_ms: f64, 
    cycle_count: usize
) -> Vec<ScheduledNote> {
    let master_duration_ms = (60_000.0 / bpm) * 4.0; // 1 Bar in 4/4
    let ctx = RenderContext {
        start_ms: cycle_start_time_ms,
        duration_ms: master_duration_ms,
        cycle_count,
    };

    let mut notes = Vec::new();
    traverse_ast(ast, ctx, &mut notes);
    notes
}

// --- 4. Live Coding Architecture & MIDI Scheduler ---

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
