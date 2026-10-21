# Source audit and Discord revision

Inspected 2026-10-17. This is a selective source audit, not a claim of feature parity or a line-by-line port. Upstream clones are under ignored `target/reference/`. Graphify 0.9.84 was installed into this project's Codex skill configuration and used for AST graphs of Mornye, Hermes's Discord plugin, mem0's memory package, and Graphify itself. No model credentials were needed for this extraction.

## Pinned sources and findings

| Project | Revision | Inspected implementation | Consequence for Mornye |
| --- | --- | --- | --- |
| [Hermes](https://github.com/NousResearch/hermes-agent/tree/521f4dc2a1a4e19d56419a899f779d60efa01716) | `521f4dc2a1a4e19d56419a899f779d60efa01716` | `plugins/platforms/discord/adapter.py`: SDK lifecycle, message dispatch, heartbeat health, mention policy; `adapter_authorization.py`: fresh role authorization | Use a maintained Discord SDK, separate gateway lifecycle from agent work, scope authorization to transport identity. Mornye uses a simpler fixed owner/channel policy, not Hermes role/profile routing. |
| Hermes | same | `tools/memory_tool.py`: curated MEMORY/USER files, frozen session prompt snapshot, exact-entry approval pinning | Separate stable persona/profile from retrieved episodic facts. Existing Mornye SOUL remains per-turn, so frozen-prefix caching is still a gap. |
| Hermes | same | `tools/skill_manager_guards.py`: background reviewers must read an existing skill before patching it | Skill updates need provenance and review, not merely model-authored success claims. Mornye now captures recent actual action records separately from the claim; automatic curator/evaluation remains absent. |
| [mem0](https://github.com/mem0ai/mem0/tree/0724291bd864cf807e88ee8981c8226644e8fb42) | `0724291bd864cf807e88ee8981c8226644e8fb42` | `mem0/memory/main.py`, `_add_to_vector_store`: V3 additive extraction, scoped recent context, retrieval, batched embeddings, hash deduplication, confirmed persistence/history | Do not describe current upstream as only the older ADD/UPDATE/DELETE prompt loop. Mornye now deduplicates exact facts, records real message provenance, accepts explicit useful/wrong feedback, removes disputed facts from retrieval, and stores corrections as superseding events. It does not yet have mem0’s extraction/embedding pipeline. |
| mem0 | same | `_search_vector_store` and `mem0/utils/scoring.py`: semantic candidates, optional keyword retrieval, entity boosts, expiration and semantic threshold before ranking | Current SQLite FTS is not semantic memory. A bounded embedded vector index with provider embeddings is the intended next implementation; benchmark it on the box before choosing dimensions/caps. No Neo4j or vector server is justified yet. |
| [Graphify](https://github.com/Graphify-Labs/graphify/tree/86c57be8204673c2a1b9849a3292a7dd3ab373d8) | `86c57be8204673c2a1b9849a3292a7dd3ab373d8` | `graphify/symbol_resolution.py`: conservative import-evidence-based cross-file resolution, code-only candidate gate, relation-aware edge identity | A syntax edge is not a resolved call. Keep Mornye's existing graph explicitly labeled syntax-only until resolution and ambiguity tests exist. Use actual Graphify locally for this audit. |
| Graphify | same | `graphify/cache.py`, `_grammar_fingerprint`: grammar/runtime versions participate in cache invalidation | Added a parser schema plus Cargo.lock fingerprint to Mornye's derived syntax index. Unchanged source is reparsed after dependency/schema changes. This intentionally over-invalidates on unrelated dependency changes. |
| Graphify | same | `graphify/reflect.py`, `aggregate_lessons`: useful/dead-end/corrected outcomes, distinct source evidence, time decay, corroboration, contested results | Saved text is not measured learning. Future skill evaluation should preserve negative results and corrections, require independent corroboration, and keep executable updates behind explicit review. |

These are independently written adaptations of observed design patterns. No upstream source was copied into the Rust runtime. Upstream licenses must be reviewed before incorporating code/assets later.

## Discord implementation in this revision

`src/discord.rs` uses [Serenity 0.12.5](https://docs.rs/serenity/0.12.5/serenity/) without its cache, framework, voice or collector features. Serenity manages WebSocket heartbeat/resume/reconnection and HTTP rate limiting. The gateway runs on its own thread/runtime; inference and host operations stay on the agent runtime. SQLite ingress/outbox transactions cover durable admission, duplicate IDs, interruption recovery and reply checkpointing.

Only the configured human owner is accepted. Default scope is DMs; an optional exact channel ID restricts the bot to that channel. Bot/webhook messages are ignored. Model/tool output cannot approve actions. All output mentions are disabled. Incoming messages are limited to 16 KiB with at most 64 queued/running items. Responses are chunked by UTF-16 units.

Queued requests survive a restart. Running requests become interrupted and are not automatically replayed, including `/approve`. Reply delivery is at least once around the send/checkpoint crash window. Offline message backfill, application slash commands, attachments, voice, immediate cancellation and concurrent agents are not implemented. Undeliverable output blocks the worker rather than silently discarding the response. Durable ID receipts accumulate on the microSD; retention/compaction needs a defined replay horizon before pruning.

The legacy Telegram source is retained for baseline tests only; `serve` now runs Discord. The old `telegram_user_id` config field is accepted for compatibility but does not authorize Discord.

## Remaining implementation priorities

1. Memory: scoped fact extraction, provider embeddings, hybrid retrieval, contradictory-fact handling, source references and explicit corrections. Keep persona/profile distinct from task evidence. Add tests for paraphrase recall, scope isolation and false memories.
2. Context: token-aware budgets, explicit compacted summaries and persisted lineage; do not silently lose unresolved task constraints when history is trimmed.
3. Coding jobs: snapshot revision/artifact hashes, bind test results to those hashes, record deployment targets and actual health checks. Exit zero alone never certifies deployment.
4. Learning: versioned candidates, reproducible evaluation cases, negative feedback and rollback; promotion is a human decision until measured quality warrants a different policy.
5. Code intelligence: import-based symbol resolution and ambiguous-edge tests, or a remote Graphify worker with compact result transfer. Avoid running the full Python analysis stack on the 2 GB device by default.
6. Operations: live Discord reconnect/replay tests and live provider tool turns, then ARM memory/latency measurements of this revision. Prior 4 MiB idle measurements apply only to the earlier CLI build, not this Discord SDK build.

The checked-in service definition remains a proposal. No new service has been installed or enabled on the B860H by this revision. Live verification requires Discord and model credentials; do not interpret local fixture tests as an end-to-end deployment.
