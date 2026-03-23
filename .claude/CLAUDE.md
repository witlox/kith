# Workflow Orchestration

## Profile Overview

| Phase | Profile | Input | Output | Entry Criteria | Exit Criteria |
|-------|---------|-------|--------|----------------|---------------|
| 1 | `analyst.md` | Design conversation + domain knowledge | `/specs/` complete | Project start | Graduation checklist passed |
| 2 | `architect.md` | `/specs/` | `/specs/architecture/` | Analyst graduation | Architecture consistency checks passed |
| 3 | `adversary.md` (arch mode) | `/specs/` + `/specs/architecture/` | Findings report | Architecture complete | All critical/high findings resolved |
| 4 | `implementer.md` (per crate) | `/specs/` + `/specs/architecture/` | `/crates/` (component) | Adversary sign-off on architecture | Component Definition of Done met |
| 5 | `adversary.md` (impl mode) | Everything | Findings report | Component implementation complete | All critical/high findings resolved |
| 6 | `integrator.md` | Everything | Integration report + tests | Multiple components implemented | Graduation criteria met |

## Iteration Loops

### Loop A: Architecture Refinement (Phases 2-3)
```
architect → adversary → [findings] → architect → adversary → ... until clean
```

### Loop B: Component Implementation (Phases 4-5)
```
implementer(crate_N) → adversary → [findings] → implementer(crate_N) → ... until clean
```

### Loop C: Integration (Phase 6 + rework)
```
integrator → [findings] → implementer(affected crates) → adversary → integrator → ... until clean
```

### Escalation Path

Any phase can escalate to a prior phase:
- Implementer → Architect (interface doesn't work)
- Implementer → Analyst (spec is ambiguous or incomplete)
- Adversary → Architect (structural flaw)
- Adversary → Analyst (spec gap)
- Integrator → Architect (cross-cutting structural issue)

Escalations go to `/specs/escalations/` and must be resolved before the escalating phase can complete.

## Usage with Claude Code

### Swapping Profiles

```bash
./switch-profile.sh analyst
./switch-profile.sh architect
./switch-profile.sh adversary
./switch-profile.sh implementer "kith-daemon"
./switch-profile.sh integrator
```

### Recommended Implementation Order

1. **kith-common** — shared types, error taxonomy, trait definitions (including InferenceBackend)
2. **kith-mesh** — WireGuard tunnel management + Nostr signaling
3. **kith-daemon** — minimal: gRPC exec + state query
4. **kith-shell** — PTY wrapper + LLM inference + tool dispatch
5. **kith-sync** — cr-sqlite CRDT replication between daemons
6. **kith-state** — vector index + ingest daemon

## Checkpoints and Human Gates

**Recommended human gates:**
- After Phase 1 (analyst): Review specs.
- After Phase 3 (adversary on architecture): Review findings.
- After Phase 6 (integrator): Review integration report.
