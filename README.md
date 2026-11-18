# Tempo Chess

Chess owns content identity, repertoire/tactic definitions, prescribed-move
grading, pure game comparison and import planning. It imports Study's interface
and the small shared vocabulary. It performs no storage, network or provider I/O.

## Start with these three concepts

| Concept | What it is | When you reach for it |
| --- | --- | --- |
| `Catalog` | The collection of targets and repertoire lines. | Validate an import, resolve a selected target, or compare a game with current content. |
| `ContentItem` | One stable target, content version, definition and metadata. | Resolve a target into a Study exercise or inspect its provenance/availability. |
| `ChessDefinition` | The rules and ordered learner decisions needed by a grader. | Start or answer an exercise without handing the grader the whole catalog. |

A label edit keeps a target's identity and version. A prescribed-response edit
keeps its identity but receives a newer content version. An issued attempt
continues to identify the definition it was issued; the application prevents
stale work from grading a replacement definition.

Every named concept has its own file, a “When to use” explanation and an in-file
`#[cfg(test)]` usage scenario. The root re-exports are unchanged, so callers can
continue to write `use tempo_chess::ContentItem`. Small tests demonstrate a
particular consumer workflow; the sequence and import tests retain the longer
end-to-end domain behavior.

## Content navigation

| Concept | File | Usage scenario in that file |
| --- | --- | --- |
| Collection and cross-reference validation | [catalog.rs](src/catalog.rs) | Repeated targets are allowed; a second response target for one repertoire position is rejected. |
| Stable target and content version | [content_item.rs](src/content_item.rs) | An issued exercise retains version 1 while a changed response produces version 2. |
| Grader instructions | [chess_definition.rs](src/chess_definition.rs) | Repertoire requires one decision; a tactic can contain the full sequence. |
| Repertoire versus tactic identity | [content_kind.rs](src/content_kind.rs) | A repertoire question and tactic share a starting board but remain different targets. |
| Canonical position identity | [position_key.rs](src/position_key.rs) | Resolve a board within one named repertoire. |
| One learner move plus replies | [decision.rs](src/decision.rs) | Automatic opponent replies do not create additional learner questions or grades. |
| Ordered repertoire context | [repertoire_line.rs](src/repertoire_line.rs) | Three line occurrences can refer to only two learning targets. |
| Availability and quarantine | [content_status.rs](src/content_status.rs) | Retain conflicting content for inspection while excluding it from new selection. |
| Content failures | [chess_error.rs](src/chess_error.rs) | Missing target identity is an explicit comparison error, not a learner grade. |

Repertoire content has one learner decision for each `(repertoire, position)`
target. Repetitions and transpositions refer to that same target. A tactic has
one or more learner decisions but one target and whole-exercise assessment.
A repertoire line groups separate targets; it does not merge their schedules.

## Grading navigation

| Concept | File | Usage scenario in that file |
| --- | --- | --- |
| Move input | [chess_answer.rs](src/chess_answer.rs) | Package board input as UCI; malformed notation is an input error. |
| Resumable decision cursor | [chess_state.rs](src/chess_state.rs) | Resume at the remaining decision and reject a cursor beyond completion. |
| Board/instructions for the UI | [chess_prompt.rs](src/chess_prompt.rs) | Show the next position; completion removes the prompt. |
| Per-response feedback | [chess_feedback.rs](src/chess_feedback.rs) | A wrong move gives feedback without automatic replies. |
| Persisted algorithm identity | [grading_scheme.rs](src/grading_scheme.rs) | Select the implementation from a loaded enum, without persisting a callback. |
| Sequence grading algorithm | [exact_sequence_grader.rs](src/exact_sequence_grader.rs) | Clean completion, a miss on either decision, correction without extra credit, and malformed input. |
| Concrete Study attempt alias | [chess_attempt.rs](src/chess_attempt.rs) | Generic Study progression retains target and content version. |
| Result of one submission | [chess_progress.rs](src/chess_progress.rs) | Correct feedback after a miss has no second assessment. |

`ChessFeedback.correct` is about one response. `ChessProgress.assessment` is a
newly emitted whole-attempt outcome. `attempt.terminal` is the outcome retained
from earlier submissions. Study owns the sticky, at-most-once assessment: use
`tempo_study::submit` to progress attempts, including correction flows.

The persisted `GradingScheme::ExactSequenceV1` rejects unknown serialized scheme
names. Future changes to grading semantics are content-version changes rather
than silent reinterpretations of old attempts.

## Import and game comparison navigation

| Concept | File | Usage scenario in that file |
| --- | --- | --- |
| PGN plus source, repertoire and learner color | [import_request.rs](src/import_request.rs) | Reject a mismatched repertoire, retain provenance, and preserve legacy requests. |
| Parser interface | [content_parser.rs](src/content_parser.rs) | Parse and plan through the trait without a store. |
| Standard repertoire PGN import | [compile_repertoire.rs](src/compile_repertoire.rs) | Import legal nested variations, share transposed positions, and retain comments for teaching. |
| Complete repertoire replacement | [plan_repertoire_replacement.rs](src/plan_repertoire_replacement.rs) | Replace routes, retire omitted targets, and preserve versions of unchanged answers. |
| Learner color | [learner_color.rs](src/learner_color.rs) | Study Black's reply from a White-to-move starting position. |
| Registered parser substitute | [fixture_parser.rs](src/fixture_parser.rs) | Known PGN works; unknown input explicitly fails. |
| Position identity interface | [position_canonicalizer.rs](src/position_canonicalizer.rs) | Ask the trait for a key instead of treating raw FEN as identity. |
| Orthodox position identity | [standard_position_canonicalizer.rs](src/standard_position_canonicalizer.rs) | Distinguish castling rights, side to move, and legally effective en passant while ignoring move counters. |
| Registered identity substitute | [fixture_canonicalizer.rs](src/fixture_canonicalizer.rs) | Explicitly map two FEN counter variants to one key. |
| Pure game comparison interface | [game_comparator.rs](src/game_comparator.rs) | Compare a supplied game with an explicit content snapshot. |
| Registered comparison substitute | [fixture_comparator.rs](src/fixture_comparator.rs) | Validate referenced targets and reject unknown games. |
| Game feedback fact | [deviation.rs](src/deviation.rs) | Identify the target, move and game location without changing content. |
| Import algorithm | [plan_import.rs](src/plan_import.rs) | Preserve metadata versions, version changed responses, merge provenance and quarantine conflicting tactics. |
| Complete import proposal | [content_plan.rs](src/content_plan.rs) | Review a merged catalog while both original inputs remain unchanged. |
| One target's reported effect | [content_change.rs](src/content_change.rs) | A duplicate source ID maps to the retained target. |
| Meaning of an import effect | [change_kind.rs](src/change_kind.rs) | Select semantic changes requiring learning resets. |
| Coherent demo dataset | [demo_fixtures.rs](src/demo_fixtures.rs) | Parser and comparator fixtures refer to the same catalog targets. |

`plan_import` receives no learning state. It produces the content proposal; the
application prepares any learning resets and commits both atomically. Changes
can contain more than one entry for a target, for example `Added` followed by
`Quarantined`. They are not a unique target-to-kind map.

Game comparison returns facts. The application attaches provider/event identity,
checks dataset/content version, and decides whether to submit external learning
feedback. It does not route background game I/O through Study.

## Real behavior and explicit limits

Real behavior here includes typed contracts, prescribed-move sequencing,
structural target/line validation, semantic content changes and quarantine
planning. Move/FEN shape checks do not validate chess legality or calculate the
best move. A `PositionKey` is not itself a canonicalizer.

`compile_repertoire` accepts a stable repertoire ID, display name, and
`LearnerColor`. Uploads support up to 256 KiB, 32 games, 512 plies per game
including variations, and 24 levels of variation nesting. Every game starts
from the orthodox initial position, uses standard SAN, and ends with a result
marker. Opponent alternatives form separate routes. Competing learner moves at
the same position produce a conflict. Move comments go into the lesson metadata
for teaching, and the prescribed sequence stays in the definition.

`StandardPositionCanonicalizer` checks standard-chess positions with
`cozy-chess`. The record key uses the first four FEN fields and retains an en
passant square when a legal capture exists. The importer uses this key with
fixed counters for the prompt, so changing the move order into a transposition
does not revise the definition by itself.

The fixture parser, canonicalizer and comparator still accept only their
registered inputs. `demo_catalog()` uses legal opening positions to exercise
the interfaces.

## Run the examples

From the assembled development workspace:

```sh
cargo test -p tempo-chess
cargo test -p tempo-chess content_item::tests
cargo test -p tempo-chess exact_sequence_grader::tests
cargo test -p tempo-chess --doc
cargo doc -p tempo-chess --no-deps --open
cargo clippy -p tempo-chess --all-targets -- -D warnings
```

The root [lib.rs](src/lib.rs) is an export index. The longer grader and import
files keep each algorithm with its behavior scenarios; `demo_fixtures.rs` keeps
one dataset together. Initial `=0.1.0` dependencies still use the local workspace
patches until coordinated releases are available.

## Compile and present a tactic

The converter applies the one explicit opponent setup move with
`apply_setup_move(fen, uci)` for catalog packs. Stored cards already start on
the learner's turn. Pass the full learner-first UCI line in a `TacticRecord`
to `compile_tactics(&records)`. The compiler checks all plies on a standard
board, including a final opponent reply, and builds the existing decisions.
It normalizes initial move clocks and ineffective en passant for definition
comparison. The source converter keeps the original FEN and line in its archive.

`TacticRecord::target_id()` returns
`tactic:<lowercase hex UTF-8 namespace>:<lowercase hex UTF-8 source ID>`.
This is also a reserved provenance token. After `plan_import`, find the
retained target by that token. Identical lines merge into a retained target,
while competing lines at one practice start are quarantined. The planner
checks retained aliases against the original practice start before matching
other definitions. If a primary changes, an omitted alias keeps its prior line.
It can receive its own target and enter quarantine with the conflicting primary.
If aliases and the primary move to one equal definition in the same batch, they
remain merged under the original primary and its new version.

Use `LegalSequenceGrader` in the real runtime, passing the learner's standard
UCI answer to Study. Malformed and illegal moves return an input error. Legal
alternatives follow existing exact-sequence grading and correction.
`ExactSequenceGrader` remains available to structural fixture callers.

Use `project_position(definition, state, ready)` on the issued version of the
definition. It returns authoritative FEN, learner orientation, check status
and all legal standard UCI moves when ready. A locked, incomplete view retains
its current decision board. A completed view reconstructs the board after the
last prescribed move and any trailing reply.
