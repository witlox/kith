# Kith

An intent-driven distributed shell: a reasoning layer (LLM) over a mesh of machines, where the agent uses standard Unix tools directly and only adds native tools for genuinely new capabilities.

## What Is This?

Kith replaces the traditional terminal workflow with a reasoning layer that operates across a mesh of machines — executing locally or remotely, maintaining persistent operational context, and enforcing policy-scoped containment on every action.

The Unix philosophy stays intact: standard tools, standard commands, standard pipes. The agent is the orchestrator that used to be you.

## Design Principles

- **Unix tools are the tools** — no proprietary wrappers around cat/grep/sed
- **Intent-driven, not command-driven** — express what you want; the agent composes and executes
- **Escape hatch always available** — prefix with `run:` to bypass the agent
- **Distributed by default** — mesh of kith-daemons connected via WireGuard, synced via CRDTs
- **Containment as a primitive** — every agent action is policy-scoped and audited
- **Model-agnostic** — any LLM with tool calling works

## Components

| Component | Role |
|-----------|------|
| **kith shell** | PTY wrapper + LLM inference + tool dispatch |
| **kith-daemon** | gRPC service on each machine: exec, policy, audit, drift |
| **sync layer** | cr-sqlite CRDT replication between daemons |
| **mesh network** | WireGuard tunnels + Nostr signaling for peer discovery |
| **state layer** | Vector index + keyword retrieval over operational history |
