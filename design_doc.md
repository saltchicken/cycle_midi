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
* `#QUANTIZE=4`: Sets the phrase boundary for live code swapping. If you edit and save the file while the sequencer is running, the engine will stage your new pattern and wait until the current cycle count is a multiple of this number before seamlessly dropping it in. Defaults to `1` (swaps on the next immediate cycle).
* `#QUANTIZE=auto`: Dynamically calculates the "Least Common Multiple" (LCM) of the cycle lengths across all unmuted tracks and sets the swap boundary to exactly match your longest loop.
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
  * *Result:* Plays on MIDI Channel 10. **Note: Because Channel 10 is traditionally used for standard MIDI Drum Kits, `T10` will automatically ignore the global `#SCALE` directive so your drum mappings don't get transposed!**

**Per-Track Modifiers (`scale`, `fast`, `slow`, `seed`)**
You can designate an entire track to play faster or slower, lock it to a specific scale, or assign a random seed. Modifiers can be placed in any order before the colon.
* `T1 fast 2: C4 D4` (Plays the sequence twice as fast, completing two full loops per cycle)
* `T2 slow 2: C3 . E3 .` (Plays the sequence at half speed, taking two cycles to complete one loop)
* `T3 scale G3 minor_pentatonic: 0 2 3 4` (Track 3 plays numeric notes in G minor pentatonic)
* `T4 seed 42: [C4 E4]?50` (Locks the probability generator to a specific seed so it repeats identically across cycles)
* `T4 seed 42 every 4: [C4]?50` (Sets a seed, but automatically increments the seed value every 4 *Macro-Cycles* (full pattern loops) to generate a new variation that then repeats).
* `T5 scale D2 dorian fast 2: 0 1 2 3` (Functionally identical to the line above)

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

### Arpeggiator `arp()`
Breaks down chords or groups of notes into sequential arpeggio patterns, inspired by TidalCycles.
* `C4+E4+G4 arp(up)`
  * *Result:* Evaluates the chord into individual notes and plays them sequentially (C4, then E4, then G4) equally spaced within the time slot.
* `[0 2 4] arp(updown)`
  * *Result:* Takes the notes 0, 2, and 4 (relative to the active scale) and plays them up then down sequentially.

**Supported Arp Styles:**
* `up`: Lowest to highest pitch
* `down`: Highest to lowest pitch
* `updown`: Up then down (exclusive of duplicated top/bottom notes)
* `downup`: Down then up
* `converge`: Outside-in (lowest, highest, second lowest, second highest...)
* `diverge`: Inside-out (middle expanding outwards)
* `pinkyup`: Alternates between each note (lowest to highest) and the highest note.
* `pinkyupdown`: PinkyUp going up, then coming back down.

---

## 7. Conditionals

Conditionals can act on two different time horizons: the **Micro-Cycle** (the standard 1-bar cycle limit) and the **Macro-Cycle** (the full length of the pattern, automatically calculated as the Least Common Multiple (LCM) of all repeating tracks combined). 

### Micro Conditionals (`if`, `only`)
These evaluate against the fast, underlying 1-bar cycle count.

* **`if(interval, offset)`**: Appends to any postfix modifier (like `*`, `/`, or `arp()`) to apply it *only* on specific cycles.
  * `[C4 D4] *2 if(4)` (Plays twice as fast only on cycle 0, 4, 8...)
* **`only(interval, offset)`**: Appends to a node to make it play *only* on specific cycles. On cycles where the condition fails, it evaluates as a Rest (`.`).
  * `C4 only(4)` (Plays C4 on cycle 0, 4, 8... completely silent on others)

### Macro Conditionals (`m_if`, `m_only`)
These evaluate against the overarching *Macro-Cycle*, allowing you to trigger variations at the true bounds of your entire arrangement.

* **`m_if(interval, offset)`**: Applies a modifier for the ENTIRE duration of the matched macro loop count.
  * `[C4 D4] arp(up) m_if(4)` (Arpeggiates continuously during every 4th full repetition of the multi-track pattern).
* **`m_only(interval, offset)`**: Gates a node so it triggers **ONLY on the first cycle** of the matched macro loop count. Perfect for turnaround crash cymbals or fills that you don't want to repeat unnecessarily.
  * `36 m_only(4)` (Plays a single crash cymbal at the exact beginning of every 4th full pattern loop, and remains silent for the rest of it).

*Note: The `offset` parameter is optional for all conditionals. `if(4)` is automatically shorthand for `if(4, 0)`.*

## 8. Rust Abstract Syntax Tree (AST) Mapping

The notation is designed to be parsed via `chumsky` into the following structured Rust AST:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub bpm: Option<f64>,
    pub quantize: Option<QuantizeMode>,
    pub scale: Option<ScaleDef>,
    pub global_silence: bool,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QuantizeMode {
    Fixed(usize),
    Auto,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub channel: u8,
    pub is_muted: bool,
    pub scale: Option<ScaleDef>,
    pub seed: Option<SeedDef>,
    pub root_node: Node,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeedDef {
    pub base: u64,
    pub macro_interval: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pitch {
    Absolute(u8),
    Numeric(i32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ArpStyle {
    Up, Down, UpDown, DownUp, Converge, Diverge, PinkyUp, PinkyUpDown
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Note { pitch: Pitch, velocity: u8, gate: u8, prob: u8 },
    Chord(Vec<Node>),
    Rest,
    Hold,
    Sequence(Vec<Node>),
    Parallel(Vec<Vec<Node>>),
    Euclidean(Box<Node>, u8, u8),
    Alternator(Vec<Node>),
    SpeedModifier(Box<Node>, f32),
    Arp(Box<Node>, ArpStyle),
    
    // Evaluates against the 1-bar cycle limit
    Condition {
        interval: usize,
        offset: usize,
        true_branch: Box<Node>,
        false_branch: Box<Node>,
    },
    
    // Evaluates against the LCM of all tracks
    MacroCondition {
        interval: usize,
        offset: usize,
        true_branch: Box<Node>,
        false_branch: Box<Node>,
    }
}
