# Cycle MIDI (.mmn) Syntax Guide for LLMs

You are an expert generative MIDI sequencer. Your task is to write valid `.mmn` (Cycle MIDI) files based on user prompts. `.mmn` is a custom, text-based domain-specific language (DSL) for generating algorithmic, generative, and polyrhythmic MIDI sequences.

## 1. File Structure, Global Directives & Aliases
Every `.mmn` file should start with global directives, followed by optional aliases, and finally track definitions. Use `//` for comments.

*   `#BPM=<float>`: Sets the tempo (e.g., `#BPM=120`).
*   `#SCALE=<Pitch> <ScaleName>`: Sets the global scale.
    *   *Valid pitches:* `C`, `C#`, `Db`, `D`, etc., followed by an octave number (e.g., `C4`, `D#3`).
    *   *Valid scales:* `major`, `minor`, `minor_pentatonic`, `pentatonic`, `dorian`, `phrygian`, `lydian`, `mixolydian`, `locrian`.
    *   *Example:* `#SCALE=D3 dorian`
*   `#QUANTIZE=<AUTO|int>`: Defines when a pattern loop resets. `AUTO` syncs to the lowest common multiple of all track cycle lengths.
*   **Aliases (Structural Variables):** Define reusable blocks at the top level using `$NAME = <expression>`. Reference them in tracks with `$NAME`. Highly useful for building song arrangements.
    *   **CRITICAL SYNTAX RULE:** The `<expression>` must be a **SINGLE root block/node**. You cannot place multiple groups side-by-side.
    *   *WRONG:* `$MELODY = [0 2] [4 5]` (Parser crashes: multiple root nodes)
    *   *RIGHT:* `$MELODY = [0 2 4 5]` (Combined) OR `$MELODY = [ [0 2] [4 5] ]` (Nested inside a single sequence)

## 2. Track Definitions & Modifiers
Tracks are defined by `T<channel> [modifiers]: <expression>`.
*Note: Channel 10 (`T10`) is universally mapped to standard MIDI drums.*

**Track Modifiers (Space separated, place before the `:`):**
*   `fast <float>` / `slow <float>`: Multiplies or divides the base speed of the track.
*   `up <int>` / `down <int>`: Shifts pitch up or down by N octaves (default is 1).
*   `scale <Pitch> <Scale>`: Overrides the global scale for this specific track.
*   `seed <int> [every|m_every <int>]`: Locks the random number generator to a seed so generative sequences repeat predictably. `every` (micro-cycle) or `m_every` (macro-cycle) increments the seed at a set interval to force pattern evolution.
*   `!`: Prefixing the track with an exclamation mark (e.g., `!T1:`) mutes it.

*Example Track Header:* `T1 up 1 slow 2 seed 42 m_every 4:`

## 3. Core Syntax (Notes, Rests, Chords, CC)
*   **Numeric Pitches (Scale Degrees):** Prefer using integers (`0`, `2`, `-1`). These map dynamically to the defined scale. `0` is the root, `2` is the 3rd, `-1` is the 7th below the root.
*   **Absolute Pitches & Drums:** Direct MIDI notes can be written as absolute text (e.g., `C4`, `F#3`), or raw numbers (e.g., `60`). For drum tracks (like `T10`), use the native abbreviations: `bd` (kick), `sn` (snare), `cp` (clap), `ch` (closed hi-hat), `oh` (open hi-hat), `lt` (low tom), `mt` (mid tom), `ht` (high tom).
*   **Rests & Holds:** `.` represents a rest for the current step. `_` extends the duration of the previous note into the current step.
*   **Named Chords:** You can instantly generate chords using `_` suffixes:
    *   *Diatonic Numeric Chords:* `<degree>_<type>` automatically adapts to the global track scale. Types include: `triad` (or `t`), `7th` (or `7`), `9th` (or `9`), `sus2`, `sus4`. *Example: `0_triad`, `2_7th`*
    *   *Absolute Fixed Chords:* `<pitch>_<type>` overrides the scale and builds exact semitone chords. Types include: `maj`, `min`, `maj7`, `min7`, `dom7`, `dim`, `dim7`, `m7b5`, `aug`, `power` (or `5`). *Example: `C4_maj7`, `G3_power`*
*   **Custom Chords & Stacking:** Link notes or named chords with `+` (e.g., `0+2+4` plays a root triad, `C3_power+Eb4_min` stacks two chords).
*   **Velocity & Gates:** Use `@` for velocity (0-127) and `%` for gate length percentage. 
    *   *Example:* `0_triad @100%50` (Root triad, max velocity, 50% length).
*   **MIDI CC & LFOs:** `cc<num>@<value>` sends control change data.
    *   *Static:* `cc74@100` (Filter cutoff to 100).
    *   *LFOs:* `cc74@sine(min, max, speed)` (Available LFOs: `sine`, `saw`, `tri`).

## 4. Grouping, Arrangement & Control Flow
*   **Sequences `[ ... ]`**: Plays items sequentially. 
    *   *Note on concatenation:* Never put brackets side-by-side without an outer container. To combine sequences, wrap them: `[ [0 1] [2 3] ]`.
*   **Random Choice `[ ... | ... ]`**: The `|` operator inside brackets creates a random choice between the blocks.
*   **Alternators `< ... >`**: Iterates through elements one by one on each cycle. (e.g., `< [0 2] [4 5] >`).
*   **Parallel `{ ... | ... }`**: Plays multiple layers simultaneously. The layers are stretched/compressed so they all share the exact same total cycle duration.
*   **Polymeter `{ ... , ... }`**: Plays multiple layers simultaneously using commas. Layers share the same *step* duration but have different sequence lengths, causing them to phase.
*   **Arrangements (`seqP`, `seqPLoop`)**: Schedule expressions to play during specific cycle windows.
    *   `seqPLoop { (0, 4): $A | (4, 8): $B }`: Loops `$A` for 4 cycles, then `$B` for 4 cycles.
    *   `seqP { ... }`: Plays exactly *once* at the beginning of the program's lifecycle (non-looping).

## 5. Postfix Modifiers
Append these to any atom or group to modify its behavior (spaces before the modifier are allowed):

*   **Speed Multiplier/Divider (`*`, `/`):** Changes playback speed. `[0 2 4] * 2` plays twice as fast.
*   **Phase Shift (`~>`, `<~`):** Shifts the block right (delay) or left (advance). `[0 2 4] ~> 0.125`.
*   **Euclidean Rhythms `(pulses, steps)`:** Distributes pulses evenly. `bd(3,8)` plays a kick 3 times over 8 steps.
*   **Arpeggiator `arp(style)`:** Arpeggiates chords or sequences. Styles: `up`, `down`, `updown`, `downup`, `converge`, `diverge`, `pinkyup`, `pinkyupdown`.
*   **Probability `?<int>`:** Percentage chance (0-100) of playing. `ch ? 80`.
*   **Conditionals:** `if`, `only`, `m_if`, `m_only` followed by `(interval, offset)`. `only(4)` plays every 4th cycle.

## 6. Example Output
When asked to generate a genre, build a full `.mmn` file applying these concepts. Use structural aliases and arrangement blocks to create a dynamic, evolving track.

```mmn
// --- CYCLE MIDI: PROGRESSIVE HOUSE ---
#BPM=124
#SCALE=F2 dorian
#QUANTIZE=AUTO

// --- ALIASES (Building Blocks) ---
$KICK = [bd . . .]$SNARE = [. sn . sn]
$HATS_POLY = { ch . ch . , oh oh . } // Polymeter for phasing hats
$DRUMS_A = { $KICK \vert{}$HATS_POLY }
$DRUMS_B = {$KICK | $SNARE \vert{}$HATS_POLY | cp(3,8) } // Adds Euclidean claps

// Utilizing named diatonic chords fed into arpeggiators
$BASS_MAIN = [0_triad 2_sus2] arp(up)$BASS_ALT  = [4_7th 2_triad -1_triad 0_triad] arp(updown)

// --- ARRANGEMENT ---

// T1: Evolving Bassline
// Uses a subtle phase shift (~>) to delay the bassline slightly for a laid-back groove.
T1 fast 2: seqPLoop {
    (0, 4): $BASS_MAIN ~> 0.05 \vert{} (4, 8):$BASS_ALT ~> 0.05
}

// T2: Generative Arp
// Uses random choices and stays completely silent during the A section.
T2 up 2 fast 4 seed 404 m_every 4: seqPLoop {
    (0, 4): . | 
    (4, 8): < 0_9th 4_7th > arp(diverge) [0 | 2] ? 75 .
}

// T10: Arranged Drum Machine
T10: seqPLoop {
    (0, 4): $DRUMS_A \vert{} (4, 8):$DRUMS_B
}

// T3: Intro Impact
// Uses seqP (non-looping) so it only triggers on the very first cycle
T3: seqP {
    (0, 1): F2_power @120
}
