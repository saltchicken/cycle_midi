# MIDI Mini-Notation (MMN) - Language Design Document

## 1. Overview and Philosophy
The MIDI Mini-Notation (MMN) is a domain-specific language designed for live coding MIDI sequences. It addresses the shortcomings of sample-first languages like TidalCycles by providing native support for standard MIDI concepts (Velocity, Gate, Chords) while drastically improving readability during live performance. 

**Core Tenets:**
* **Time is Implicit:** Time is always relative to a master "Cycle" (e.g., 1 bar). The cycle is divided equally by the number of root-level elements.
* **Structure is Explicit:** Different types of groupings (sequential vs. parallel) use distinct visual brackets to avoid symbol soup.
* **MIDI-First:** Modifiers for velocity and note length are attached inline via postfix operators.

---

## 2. Program Structure, Tracks, & Directives

An MMN file consists of optional global directives followed by track declarations.

### Global Directives
Directives control the overall playback state of the sequencer. They are evaluated at the top of the file before any tracks.
* `#BPM=120` or `#BPM=128.5`: Sets the master tempo. If omitted during a live-coding session, the engine simply maintains the last known BPM.
* `#SILENCE`: Instantly mutes all playback globally when present. Useful for quick panic stops or dramatic dropouts during a performance.

### Scale Definitions
You can optionally lock the sequencer to a musical scale using the `#SCALE=` directive followed by a root note and a scale type.
* `#SCALE=C4 minor` 
* `#SCALE=G3 pentatonic`

**Available scales:** `major`, `minor`, `dorian`, `phrygian`, `lydian`, `mixolydian`, `locrian`, `pentatonic`, `minor_pentatonic`.

When a scale is active, all plain integers (`0`, `1`, `-2`) are interpreted as **Scale Degrees** relative to the root note rather than raw MIDI integers:
* `0` = Root note (e.g. C4)
* `2` = The 3rd step of the scale (e.g. Eb4 in minor)
* `7` = An octave above the root (e.g. C5)
* `-1` = One step below the root (e.g. Bb3 in minor)

*Note: Explicit note strings like `C4` will always bypass the scale and play the exact note requested. If `#SCALE` is omitted, plain integers fall back to playing raw MIDI notes (e.g. `60`).*

### Tracks and Channels
Sequences are routed to specific MIDI channels using track declarations `TX:`, where `X` is the channel number (1-16).
* `T1: C4 . D4 _` 
  * *Result:* Plays the sequence on MIDI Channel 1.
* `T10: 36 [38 38] 42 .` 
  * *Result:* Plays on MIDI Channel 10 (traditionally used for drum machines).

**Per-Track Scales**
You can optionally lock a specific track to its own scale by providing the scale in parentheses right after the track declaration. This overrides the global `#SCALE` directive for that track only, making polytonal music very easy to write.
* `T2(G3 minor_pentatonic): 0 2 3 4` (Track 2 plays numeric notes in G minor pentatonic)
* `!T3(C4 lydian): 0 . 4 2` (Track 3 is muted, but ready to play in C Lydian)

**Track Muting (`!`)**
Prefix a track declaration with an exclamation point to mute it instantly without having to delete your code.
* `!T1: C4 E4 G4 .` (Track 1 is muted and will not generate MIDI events)

---

## 3. Core Elements

### Pitches
Notes can be written using standard musical notation or raw MIDI integers.
* `C4`, `D#3`, `Bb2` (Note names)
* `60`, `62`, `127` (MIDI integer values)

### Rests (`.`)
A dot represents a rest. It occupies one fraction of the current time slot, creating clean visual negative space.
* `C4 . D4 E4` (Cycle divided by 4: Note, Rest, Note, Note)

### Sustain / Holds (`_`)
An underscore holds the previous note for another step, extending the MIDI note-length (Gate) without re-triggering the note.
* `60 . 62 _` (60 plays for 1/4 cycle, rest for 1/4, 62 plays for 2/4 cycle)

---

## 4. Grouping & Layering

### Sequential Subdivisions `[ ]`
Subdivides a specific slot of time into smaller equal parts.
* `C4 [D4 E4] F4 .` 
  * *Result:* The cycle is divided into 4 slots. Slot 2 is further divided in half, playing D4 and E4 as eighth notes.

### Parallel Layers `{ | }`
Plays multiple sequences simultaneously in the same timeframe, enabling easy polyrhythms and counterpoint.
* `{ C4 E4 G4 | C3 . }` 
  * *Result:* Layer 1 plays triplets. Layer 2 plays half-notes. Both share the same total cycle length.

### Chords `+`
Links notes together so they fire at the exact same millisecond.
* `C4+E4+G4 [F4 A4]` 
  * *Result:* A C-Major chord plays for half the cycle, followed by F4 and A4 playing for a quarter cycle each.

---

## 5. Inline MIDI Modifiers
Modifiers are applied directly to notes, chords, or groups using postfix operators.

* **Velocity `@`**: Values from 0 to 127. 
  * `C4@100` (C4 with 100 velocity)
* **Gate / Length `%`**: Percentage of the step size to hold the note before sending the `Note Off` message. Default is 100%.
  * `C4%50` (C4 played staccato; Note Off fires exactly halfway through its time slot)
* **Probability `?`**: Percentage chance (0-100) the note will fire on a given cycle.
  * `[C4 E4]?75` (Both notes have a 75% chance of playing)
* **Stacking Modifiers**: Modifiers can be chained.
  * `C4+E4@80%20?50` (Chord, 80 velocity, 20% gate duration, 50% chance to play)

---

## 6. Algorithmic Generators

### Euclidean Rhythms `(pulses, steps)`
Generates highly musical, evenly distributed rhythms based on Euclidean geometry.
* `C4(3,8)` 
  * *Result:* Distributes 3 C4 notes as evenly as possible across 8 subdivisions (the classic Tresillo rhythm).

### Repetition `*` and Slowdown `/`
Multiplies or divides the rate of playback for an element within its time slot.
* `C4*3` (Plays C4 three times evenly inside its time slot)
* `[C4 E4]/2` (Plays C4 on the first cycle, E4 on the second cycle)

### Alternators `< >`
Cycles through the contained elements one by one each time the master loop repeats.
* `<C4 E4 G4> D4` 
  * *Cycle 1:* `C4 D4`
  * *Cycle 2:* `E4 D4`
  * *Cycle 3:* `G4 D4`

---

## 7. Rust Abstract Syntax Tree (AST) Mapping

The notation is designed to be parsed via `chumsky` into the following structured Rust AST:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// Master tempo (optional to allow carrying over previous state)
    pub bpm: Option<f64>,
    /// Master scale for numeric pitches
    pub scale: Option<ScaleDef>,
    /// Global panic / mute toggle
    pub global_silence: bool,
    /// All defined MIDI tracks
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    /// 0-indexed MIDI channel (0-15 corresponds to Channels 1-16)
    pub channel: u8,
    /// Whether the track is prefixed with the `!` mute operator
    pub is_muted: bool,
    /// Track specific scale definition
    pub scale: Option<ScaleDef>,
    /// The parsed musical sequence for this track
    pub root_node: Node,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pitch {
    Absolute(u8),
    Numeric(i32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// A single MIDI note with its computed modifiers
    Note { 
        pitch: Pitch, 
        velocity: u8, 
        gate: u8, 
        prob: u8 
    },
    
    /// Multiple nodes firing simultaneously
    Chord(Vec<Node>),
    
    /// Silence for the duration of the time slot
    Rest,
    
    /// Extends the gate of the previously fired note
    Hold,
    
    /// Sequential time subdivision: [ A B C ]
    Sequence(Vec<Node>),
    
    /// Polyrhythmic / parallel playback: { A B | C D }
    Parallel(Vec<Vec<Node>>),
    
    /// Algorithmic distribution: Node(pulses, steps)
    Euclidean(Box<Node>, u8, u8),
    
    /// Rotates through children per master cycle: < A B C >
    Alternator(Vec<Node>),
    
    /// Multiplier (*) or Slowdown (/)
    SpeedModifier(Box<Node>, f32),
}
