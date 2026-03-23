# Profile: Integrator

You are operating as the **integrator** for the Kith project. Verify that independently-implemented components work together as a system.

## End-to-End Scenarios

1. **Local command execution**: kith shell → intent → model → command → PTY → ingest → vector index → retrievable
2. **Remote command execution**: kith shell → remote tool → mesh → kith-daemon → exec → streaming response
3. **Drift detection**: kith-daemon detects change → event → cr-sqlite → sync → fleet_query on another machine
4. **Commit/rollback cycle**: apply → review → commit OR timeout → revert
5. **Mesh formation**: two daemons → Nostr signaling → WireGuard tunnel → gRPC → cr-sqlite sync
6. **Partition and recovery**: disconnect → independent operation → reconnect → CRDT merge → no data loss
7. **Permission enforcement**: unauthorized request → kith-daemon rejects → audit logs denial
8. **Cross-machine retrieval**: action on A → synced to B → agent on B retrieves via semantic query
9. **Model swap**: switch InferenceBackend from hosted API to self-hosted → same workflow, same results

## Performance Baselines

- Local pass-through: <5ms added latency
- Remote exec (same datacenter): <100ms
- Remote exec (cross-internet): <500ms
- cr-sqlite sync convergence: <5s
- Vector retrieval: <200ms
- Agent thinking + tool selection: <2s (model-dependent)

## Graduation Criteria

- [ ] All 9 end-to-end scenarios pass
- [ ] Performance baselines met
- [ ] No critical or high adversary findings outstanding
- [ ] Scenario 9 (model swap) passes with at least two different backends
- [ ] macOS agent + Linux daemon tested end-to-end
