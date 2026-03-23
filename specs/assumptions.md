# Assumptions

Explicit, falsifiable. Validate before the relevant component is production-ready.

## Model Assumptions

**ASM-MDL-1: The chosen LLM follows the system prompt reliably.** Validation: 50 diverse prompts, >90% compliance.

**ASM-MDL-2: The chosen LLM can parse Unix command output.** Validation: 100 common tool outputs, >95% extraction accuracy.

**ASM-MDL-3: The InferenceBackend abstraction is sufficient.** Any model with tool calling and streaming can be used without architectural changes. Validation: test with at least two different backends (one hosted, one self-hosted).

**ASM-MDL-4: Models without interleaved thinking still produce adequate plans.** The architecture benefits from think-before-act but must not require it. Validation: run the same 20 multi-step tasks with a thinking model and a non-thinking model, compare success rates.

## Network Assumptions

**ASM-NET-1: Nostr relays are sufficiently available.** ≥2 of 5 configured relays reachable. Validation: 30-day availability monitoring.

**ASM-NET-2: WireGuard NAT traversal succeeds >90%.** Validation: test from 10 network configurations.

**ASM-NET-3: Nostr signaling latency <5 seconds.** Validation: measure across configured relays.

## Storage Assumptions

**ASM-STO-1: cr-sqlite handles <100K events/day.** Validation: load test at 2x expected volume.

**ASM-STO-2: Embeddings cluster operational data effectively.** Validation: 1 week real data, 50 queries, recall@5 >70%.

## Clock Assumptions

**ASM-CLK-1: Mesh members have NTP-synced clocks.** Clock skew <30 seconds. Validation: check NTP sync status on mesh join, warn if skew exceeds 5 seconds.

**ASM-CLK-2: Commit window expiry uses daemon-local clock only.** Never compared across machines. The daemon that opened the window tracks its expiry. No cross-machine clock dependency.

## Platform Assumptions

**ASM-PLT-1: macOS supports agent-side operation.** PTY, WireGuard, Nostr, cr-sqlite, vector index. Validation: build and test on macOS.

**ASM-PLT-2: Self-hosted inference meets latency targets.** <2s time-to-first-token. Validation: benchmark with representative prompts.
