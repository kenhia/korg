# Agent Space direction note

Date: 2026-07-23 · Status: direction accepted; detailed planning deferred until
after the handoff sprint lands

## Origin

Ken's pitch "Agent Space: A Linda-Inspired Coordination Layer for AI Agents"
(Linda/tuple-space + blackboard lineage): agents coordinate through durable,
typed work objects — put / match / claim / patch / resolve — instead of direct
agent-to-agent calls, exposed through a small MCP surface. Reviewed with Claude
2026-07-23; this note records the agreed framing so the next planning session
doesn't re-derive it.

## Decision

Agent Space lands as a **korg feature cluster sequenced after the handoff
sprint** — and it is explicitly **not "handoff v2"**.

The pitch's coordination object is a *live* object: it has a state machine
(`AwaitingConsumerReview` → `ChangesRequested`), a claim/lease, concurrency
tokens, and routing. The handoff plan
([2026-07-21-handoff-node-plan.md](2026-07-21-handoff-node-plan.md))
deliberately made handoffs lifecycle-free artifacts precisely so lifecycle
semantics don't leak into backlog/survey/planning surfaces. Growing the handoff
node into a coordination object would reverse that decision. The coordination
object is much closer to a **work item with claim semantics**.

Mapping the pitch's object shape onto korg:

| Pitch concept | korg home |
|---|---|
| Coordination object (state, priority, project) | Work item (already has status, tags, project) |
| `artifacts` array | Handoff nodes attached via `has_handoff` |
| `history` | Comments (Sprint 012 read contract) |
| `routing` (domain, capability, watchers) | New fields — genuinely just "a few extra fields" |
| `concurrency` (lease, etag) | **New mechanism** — the novel work |
| `watch` / subscriptions | korg/kmon orchestration — the other novel work |

## MVP slice (post-handoff)

1. **claim/lease + optimistic concurrency on work items** — new MCP verbs
   (claim/release with lease + etag semantics).
2. **Routing fields** — `domain` / `requiredCapability` / `watchers`; small
   additive schema.
3. **Pickup query surface** — "unclaimed objects matching X", building on
   existing survey/list machinery.
4. **kmon as first watcher**; nudge/escalation orchestration later.
5. **Handoffs are the context payload** — the pitch's `artifacts` array is
   `has_handoff` edges, nothing new to build.

claim/lease is independently justified before Agent Space proper: multiple
concurrent sessions running start-sprint / refill-queue against the same
Planning queue can already race today. It fixes a latent korg hazard on its own
merits even if Agent Space never grows beyond it.

## Sequencing and current status (2026-07-23)

- korg deep review: mostly complete.
- Linking layer ([2026-07-23-linking-layer](2026-07-23-linking-layer/SUMMARY.md)):
  LB-1 (`korg:596`) **complete**; LB-2 (`korg:597`) next; LB-3 (`korg:598`)
  queued. The handoff sprint follows on the cleaned linking layer.
- Agent Space planning must be reconciled against the post-handoff architecture
  before implementation — same discipline the handoff plan applied to the deep
  review.

## Notes for LB-2 / LB-3 (597/598)

- Agent Space MVP needs **no new relationship labels** beyond the handoff
  sprint's `has_handoff`: claim/lease and routing live on the work item, not on
  edges. Closing and enforcing the vocabulary (D-11/D-12) is unaffected.
- LB-2's self-reported `origin` convention already accommodates future writers
  (`claim`, an agent-space skill, kmon orchestration); nothing to pre-build.
- LB-3's two-level edge-context read contract is **load-bearing for automated
  consumers**, not just human ergonomics: a claiming agent discovers attached
  handoffs through exactly that path. Keep truncation flags exact.

## Kill criterion

After claim + watch exist: if the only consumer is still kmon-on-a-timer and
every "handoff" is a human pasting a korg id into a session, the standalone
extraction (separate Agent Space MCP layer) hasn't earned itself. Do not
extract until **two genuinely independent consumers** exist.

## Adversarial notes worth keeping

- Tuple spaces (JavaSpaces, TSpaces) historically lost to queues on operational
  debuggability ("why did nobody pick this up?"). The modern bet — make it
  explicit whenever this is pitched — is that LLM agents make rich associative
  matching cheap, and the human-readable object doubles as agent context.
- The concurrency objection ("you don't have enough agents") was considered and
  rejected: several simultaneous sessions across projects are separate,
  uncoordinated actors within this idea's scope.
- Watch for schema sprawl (rebuilding Jira, not email): keep object types few,
  or agents burn context parsing coordination metadata instead of doing work.

Source pitch: `Agent-Space-Pitch.html` (Ken's D:\ClaudeWorks on cleo;
self-contained HTML export).
