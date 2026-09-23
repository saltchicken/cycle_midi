use crate::ast::{MidiImportOptions, Node};
use chumsky::Parser;
use midly::{Smf, Timing, TrackEventKind, MidiMessage};
use std::collections::HashMap;
use std::path::Path;

fn get_scale_intervals(name: &str) -> Vec<i32> {
    match name.to_lowercase().as_str() {
        "minor" => vec![0, 2, 3, 5, 7, 8, 10],
        "minor_pentatonic" => vec![0, 3, 5, 7, 10],
        "dorian" => vec![0, 2, 3, 5, 7, 9, 10],
        "phrygian" => vec![0, 1, 3, 5, 7, 8, 10],
        "lydian" => vec![0, 2, 4, 6, 7, 9, 11],
        "mixolydian" => vec![0, 2, 4, 5, 7, 9, 10],
        "locrian" => vec![0, 1, 3, 5, 6, 8, 10],
        "pentatonic" => vec![0, 2, 4, 7, 9],
        "chromatic" => vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        _ => vec![0, 2, 4, 5, 7, 9, 11], // major fallback
    }
}

fn parse_root_pitch(note_name: &str) -> i32 {
    let note_name = note_name.trim().to_uppercase();
    let split_idx = note_name.find(|c: char| c.is_ascii_digit() || c == '-').unwrap_or(note_name.len());
    let (name, oct_str) = note_name.split_at(split_idx);
    let octave = oct_str.parse::<i32>().unwrap_or(4);

    let base = match name {
        "C" => 0, "C#" | "DB" => 1, "D" => 2, "D#" | "EB" => 3,
        "E" => 4, "F" => 5, "F#" | "GB" => 6, "G" => 7,
        "G#" | "AB" => 8, "A" => 9, "A#" | "BB" => 10, "B" => 11,
        _ => 0,
    };
    (octave + 1) * 12 + base
}

fn format_degree(midi_note: i32, root_midi: i32, intervals: &[i32], vel: u8, preserve_velocity: bool) -> String {
    let diff = midi_note - root_midi;
    let octave_diff = diff.div_euclid(12);
    let pc_diff = diff.rem_euclid(12);
    let scale_len = intervals.len() as i32;

    let (numeric_degree, accidental) = if let Some(idx) = intervals.iter().position(|&x| x == pc_diff) {
        ((octave_diff * scale_len) + idx as i32, 0)
    } else {
        let mut closest_idx = 0;
        for (i, &interval) in intervals.iter().enumerate() {
            if interval < pc_diff {
                closest_idx = i;
            }
        }
        let deg = (octave_diff * scale_len) + closest_idx as i32;
        let acc = pc_diff - intervals[closest_idx];
        (deg, acc)
    };

    let acc_str = "#".repeat(accidental as usize);
    let degree_str = format!("{}{}", numeric_degree, acc_str);
    
    if preserve_velocity {
        format!("{}.v({})", degree_str, vel)
    } else {
        degree_str
    }
}

#[derive(Debug, Clone)]
struct NoteSpan {
    start_tick: f64,
    end_tick: f64,
    repr: String,
}

pub fn convert_midi_to_node(
    options: &MidiImportOptions,
    base_dir: &Path,
    parser: &impl Parser<char, Node, Error = chumsky::error::Simple<char>>,
) -> Result<Node, String> {
    let mut full_path = base_dir.join(&options.path);
    
    // Smart path fallback: If the file isn't in the configured MMN workspace,
    // check if it exists relative to the directory where you launched the app.
    if !full_path.exists() {
        if let Ok(cwd) = std::env::current_dir() {
            let cwd_path = cwd.join(&options.path);
            if cwd_path.exists() {
                full_path = cwd_path;
            }
        }
    }

    let data = std::fs::read(&full_path)
        .map_err(|e| format!("Could not read MIDI file {}: {}", full_path.display(), e))?;
        
    let smf = Smf::parse(&data)
        .map_err(|e| format!("Failed to parse MIDI structure for {}: {}", full_path.display(), e))?;

    let ticks_per_beat = match smf.header.timing {
        Timing::Metrical(t) => t.as_int() as f64,
        _ => return Err("SMPTE timecode timing is not supported for MIDI imports".into()),
    };

    let parts: Vec<&str> = options.target_scale.split_whitespace().collect();
    let root_name = parts.get(0).unwrap_or(&"C4");
    let scale_name = parts.get(1).unwrap_or(&"major");
    let root_midi = parse_root_pitch(root_name);
    let intervals = get_scale_intervals(scale_name);

    let mut note_spans = Vec::new();

    for track in smf.tracks {
        let mut abs_tick = 0.0;
        let mut active_notes: HashMap<u8, (f64, u8)> = HashMap::new();

        for event in track {
            abs_tick += event.delta.as_int() as f64;
            
            match event.kind {
                TrackEventKind::Midi { message: MidiMessage::NoteOn { key, vel }, .. } => {
                    let k = key.as_int();
                    let v = vel.as_int();
                    if v > 0 {
                        active_notes.insert(k, (abs_tick, v));
                    } else {
                        if let Some((start_tick, orig_vel)) = active_notes.remove(&k) {
                            let repr = format_degree(k as i32, root_midi, &intervals, orig_vel, options.preserve_velocity);
                            note_spans.push(NoteSpan { start_tick, end_tick: abs_tick, repr });
                        }
                    }
                }
                TrackEventKind::Midi { message: MidiMessage::NoteOff { key, .. }, .. } => {
                    let k = key.as_int();
                    if let Some((start_tick, orig_vel)) = active_notes.remove(&k) {
                        let repr = format_degree(k as i32, root_midi, &intervals, orig_vel, options.preserve_velocity);
                        note_spans.push(NoteSpan { start_tick, end_tick: abs_tick, repr });
                    }
                }
                _ => {}
            }
        }
    }

    if note_spans.is_empty() {
        return parser.parse(".".to_string())
            .map_err(|_| "Failed parsing empty fallback".to_string());
    }

    let ticks_per_cycle = ticks_per_beat * options.beats;
    let step_ticks = ticks_per_cycle / options.grid as f64;
    let mut grid = Vec::new();

    if options.snap_to_grid {
        let mut quantized = Vec::new();
        for span in &note_spans {
            let start_step = (span.start_tick / step_ticks).round() as usize;
            let mut end_step = (span.end_tick / step_ticks).round() as usize;
            if end_step <= start_step {
                end_step = start_step + 1;
            }
            quantized.push((start_step, end_step, span.start_tick, span.repr.clone()));
        }

        let max_step = quantized.iter().map(|q| q.1).max().unwrap_or(0);
        let mut total_steps = max_step;
        if total_steps == 0 { 
            total_steps = options.grid; 
        } else if total_steps % options.grid != 0 { 
            total_steps += options.grid - (total_steps % options.grid); 
        }

        for step in 0..total_steps {
            let mut starting: Vec<_> = quantized.iter().filter(|q| q.0 == step).collect();
            if !starting.is_empty() {
                starting.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap());
                
                if options.chords {
                    if options.subdivide {
                        let mut chords = Vec::new();
                        let mut current_chord = Vec::new();
                        let mut last_tick = -1.0;
                        
                        for q in &starting {
                            // 15 ticks gives standard chord 'strums' a 30-40ms grouped tolerance
                            if last_tick < 0.0 || (q.2 - last_tick).abs() < 15.0 {
                                current_chord.push(q.3.clone());
                            } else {
                                if current_chord.len() == 1 {
                                    chords.push(current_chord[0].clone());
                                } else {
                                    chords.push(format!("par({})", current_chord.join(", ")));
                                }
                                current_chord = vec![q.3.clone()];
                            }
                            last_tick = q.2;
                        }
                        if !current_chord.is_empty() {
                            if current_chord.len() == 1 {
                                chords.push(current_chord[0].clone());
                            } else {
                                chords.push(format!("par({})", current_chord.join(", ")));
                            }
                        }
                        
                        if chords.len() > 1 {
                            grid.push(format!("[{}]", chords.join(" ")));
                        } else {
                            grid.push(chords[0].clone());
                        }
                    } else {
                        // Not subdividing: squash all notes in this grid step into a single chord hit
                        if starting.len() == 1 {
                            grid.push(starting[0].3.clone());
                        } else {
                            let inner = starting.iter().map(|q| q.3.clone()).collect::<Vec<_>>().join(", ");
                            grid.push(format!("par({})", inner));
                        }
                    }
                } else {
                    // Original non-chord grouping behavior
                    if options.subdivide && starting.len() > 1 {
                        let inner = starting.iter().map(|q| q.3.clone()).collect::<Vec<_>>().join(" ");
                        grid.push(format!("[{}]", inner));
                    } else {
                        grid.push(starting[0].3.clone());
                    }
                }
            } else {
                let holding = quantized.iter().any(|q| q.0 < step && q.1 > step);
                if holding {
                    grid.push("_".to_string());
                } else {
                    grid.push(".".to_string());
                }
            }
        }
    } else {
        let max_tick = note_spans.iter().map(|s| s.end_tick).fold(0.0f64, f64::max);
        let mut total_steps = ((max_tick + step_ticks - 1.0) / step_ticks).floor() as usize;
        if total_steps == 0 { 
            total_steps = options.grid; 
        } else if total_steps % options.grid != 0 { 
            total_steps += options.grid - (total_steps % options.grid); 
        }

        for step in 0..total_steps {
            let window_start = step as f64 * step_ticks;
            let window_end = (step + 1) as f64 * step_ticks;

            let mut starting: Vec<_> = note_spans.iter().filter(|s| window_start <= s.start_tick && s.start_tick < window_end).collect();

            if !starting.is_empty() {
                starting.sort_by(|a, b| a.start_tick.partial_cmp(&b.start_tick).unwrap());

                if options.chords {
                    if options.subdivide {
                        let mut chords = Vec::new();
                        let mut current_chord = Vec::new();
                        let mut last_tick = -1.0;

                        for s in &starting {
                            if last_tick < 0.0 || (s.start_tick - last_tick).abs() < 15.0 {
                                current_chord.push(s.repr.clone());
                            } else {
                                if current_chord.len() == 1 {
                                    chords.push(current_chord[0].clone());
                                } else {
                                    chords.push(format!("par({})", current_chord.join(", ")));
                                }
                                current_chord = vec![s.repr.clone()];
                            }
                            last_tick = s.start_tick;
                        }
                        if !current_chord.is_empty() {
                            if current_chord.len() == 1 {
                                chords.push(current_chord[0].clone());
                            } else {
                                chords.push(format!("par({})", current_chord.join(", ")));
                            }
                        }

                        if chords.len() > 1 {
                            grid.push(format!("[{}]", chords.join(" ")));
                        } else {
                            grid.push(chords[0].clone());
                        }
                    } else {
                        if starting.len() == 1 {
                            grid.push(starting[0].repr.clone());
                        } else {
                            let inner = starting.iter().map(|q| q.repr.clone()).collect::<Vec<_>>().join(", ");
                            grid.push(format!("par({})", inner));
                        }
                    }
                } else {
                    // Original non-chord grouping behavior
                    if options.subdivide && starting.len() > 1 {
                        let inner = starting.iter().map(|q| q.repr.clone()).collect::<Vec<_>>().join(" ");
                        grid.push(format!("[{}]", inner));
                    } else {
                        grid.push(starting[0].repr.clone());
                    }
                }
            } else {
                let overlap = window_start + (step_ticks * 0.25);
                let holding = note_spans.iter().any(|s| s.start_tick < window_start && s.end_tick > overlap);
                if holding {
                    grid.push("_".to_string());
                } else {
                    grid.push(".".to_string());
                }
            }
        }
    }

    let final_string = if options.format == "raw" {
        format!("[ {} ]", grid.join(" "))
    } else if options.format == "arrange" {
        let chunks: Vec<_> = grid.chunks(options.grid).collect();
        let mut segments = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            segments.push(format!("({}, {}): [{}]", i, i + 1, chunk.join(" ")));
        }
        format!("arrange [ {} ]", segments.join(", "))
    } else {
        // chain (Run Length Encoding default)
        let chunks: Vec<_> = grid.chunks(options.grid).collect();
        let mut rle = Vec::new();
        if !chunks.is_empty() {
            let mut curr = chunks[0];
            let mut count = 1;
            for chunk in chunks.iter().skip(1) {
                if chunk == &curr {
                    count += 1;
                } else {
                    rle.push((count, curr));
                    curr = *chunk;
                    count = 1;
                }
            }
            rle.push((count, curr));
        }
        let mut segments = Vec::new();
        for (c, chunk) in rle {
            segments.push(format!("{}: [{}]", c, chunk.join(" ")));
        }
        format!("chain [ {} ]", segments.join(", "))
    };

    if options.debug {
        println!("\n[DEBUG] MIDI Import Output for '{}':\n{}\n", options.path, final_string);
    }

    // Feed the native Rust generator output straight back through our standard node parser
    parser.parse(final_string)
        .map_err(|errs| {
            let mut msg = String::new();
            for e in errs {
                msg.push_str(&format!("Error at char {}: {:?}\n", e.span().start, e.reason()));
            }
            format!("Failed to parse constructed AST string: {}", msg)
        })
}
