# tempo-chess

Typed chess content, prescribed-move grading and pure content-change planning.
This crate implements Study's grader interface and imports only `tempo-study`
and `tempo-types` among Tempo crates. It performs no storage or provider I/O.

## Public contract

| Interface / value | Responsibility |
| --- | --- |
| `ChessDefinition`, `Decision` | Repertoire/tactic identity and ordered learner decisions with separate automatic replies. |
| `ExactSequenceGrader` | Compare a supplied UCI move with the prescribed move; produce prompt, feedback, next state and optional whole-exercise assessment. |
| `ContentItem`, `Catalog`, `RepertoireLine` | Versioned content, provenance, active/quarantined status and ordered occurrences sharing target identities. |
| `ContentParser::parse` | Convert PGN plus source/repertoire context into a proposed catalog. |
| `PositionCanonicalizer::canonicalize` | Convert a FEN into the domain's canonical position identity. |
| `GameComparator::compare` | Compare a game with supplied content and return deviation facts. |
| `plan_import` | Merge proposed content and report added, metadata, semantic, unchanged and quarantine effects. |

`ChessDefinition` persists `GradingScheme::ExactSequenceV1`. Unknown serialized
scheme names are rejected. A future scheme change is a semantic content change;
it cannot silently reinterpret an old attempt.

Repertoire content has exactly one learner decision for a `(repertoire,
position)` target. Repeated line occurrences refer to that same target. A tactic
has one or more learner decisions but one target and assessment. Automatic
replies are feedback/context, never independent learner grades.

`plan_import` preserves versions for metadata, increments a target's version
for a changed answer/sequence, retains provenance, and rejects target identity
changes. It never receives or changes learning state. Identical tactics add
provenance; distinct unresolved tactics sharing a practice-start key are
quarantined together. The application applies any required learning resets and
commits the entire change atomically.

## Real behavior and explicit fixtures

Real here: typed contracts; ordered prescribed-move grading; first-miss behavior
through Study; target/line structural checks; semantic content diffing and
quarantine decisions. Structural move/FEN checks **do not validate chess
legality** or calculate whether a move is best.

`FixtureParser`, `FixtureCanonicalizer` and `FixtureComparator` accept explicitly
registered inputs and return an error for unknown inputs. They are replacement
points for a real PGN/chess library and game comparison. They do not silently
pretend to parse arbitrary input.

`demo_catalog()` contains two repertoire positions and one two-decision exercise
using legal opening positions. The multi-decision exercise is an interface
fixture, not a tactical puzzle recommendation. Helpers expose the registered
PGNs and fixture implementations for the application's demo and integration tests.

## Check

From the assembled development workspace:

```sh
cargo test -p tempo-chess
```

The public contract is in [src/lib.rs](src/lib.rs). Initial `=0.1.0` Tempo
dependencies use explicit local workspace patches until coordinated component
versions are published. Real parser/legality, SQLite constraints and application
transactions still require their own acceptance evidence when implemented.
