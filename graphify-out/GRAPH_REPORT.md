# Graph Report - Magpie  (2026-10-23)

## Corpus Check
- 19 files · ~17,715 words
- Verdict: corpus is large enough that graph structure adds value.
- Unclassified: 8 file(s) not represented in the graph (top: .service 4, .example 2, (none) 1)

## Summary
- 246 nodes · 487 edges · 11 communities (9 shown, 2 thin omitted)
- Extraction: 99% EXTRACTED · 1% INFERRED · 0% AMBIGUOUS · INFERRED: 7 edges (avg confidence: 0.85)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `f104f21d`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- discord.rs
- Store
- Agent
- tools.rs
- B860H validation â€” 2026-10-17
- tests.rs
- Mornye: research and proposed design
- main.rs
- telegram.rs
- AGENTS.md
- mornye

## God Nodes (most connected - your core abstractions)
1. `Agent` - 24 edges
2. `Store` - 24 edges
3. `Config` - 17 edges
4. `agent()` - 15 edges
5. `Queue` - 12 edges
6. `serve()` - 10 edges
7. `Mornye: research and proposed design` - 10 edges
8. `B860H validation â€” 2026-10-17` - 10 edges
9. `mock()` - 9 edges
10. `Mornye` - 9 edges

## Surprising Connections (you probably didn't know these)
- `Agent` --references--> `Config`  [EXTRACTED]
  src/agent.rs → src/config.rs
- `Agent` --references--> `Store`  [EXTRACTED]
  src/agent.rs → src/store.rs
- `dispatch_schedule()` --references--> `Agent`  [EXTRACTED]
  src/discord.rs → src/agent.rs
- `respond()` --references--> `Agent`  [EXTRACTED]
  src/discord.rs → src/agent.rs
- `serve()` --references--> `Agent`  [EXTRACTED]
  src/discord.rs → src/agent.rs

## Import Cycles
- None detected.

## Communities (11 total, 2 thin omitted)

### Community 0 - "discord.rs"
Cohesion: 0.09
Nodes (17): authorized(), chunks(), deleted_status_falls_back_to_new_reply(), deliver(), DiscordMock, dispatch_schedule(), durable_queue_deduplicates_and_never_replays_running_approvals(), edit_builder() (+9 more)

### Community 1 - "Store"
Cohesion: 0.13
Nodes (7): index(), index_file(), read(), search(), setup(), walk(), Store

### Community 2 - "Agent"
Cohesion: 0.15
Nodes (11): Agent, budget_rows(), DEFAULT_CHARACTER_PROFILE, DISCORD_CONTEXT, Progress, Thinking, Tool, review() (+3 more)

### Community 3 - "tools.rs"
Cohesion: 0.10
Nodes (11): Config, validate(), drain(), execute(), MAX_FILE, OUTPUT_LIMIT, path(), prepare() (+3 more)

### Community 4 - "B860H validation â€” 2026-10-17"
Cohesion: 0.05
Nodes (32): Concrete use of the references, Conversational assistant and Discord progress, Delivery lifecycle, Validation, Fictional reference knowledge, Mornye-inspired character profile, Roleplay boundary, Sources (+24 more)

### Community 5 - "tests.rs"
Cohesion: 0.13
Nodes (24): agent(), approval_is_exact_single_use_and_survives_restart(), auto_approval_resumes_a_previously_pending_action_once_after_restart(), call(), changed_file_cannot_be_deleted_with_old_approval(), character_profile_is_installed_by_default_and_user_edits_survive_restart(), cli_context_does_not_claim_to_be_discord(), command_timeout_is_recorded_as_failure() (+16 more)

### Community 6 - "Mornye: research and proposed design"
Cohesion: 0.20
Nodes (10): Build sequence and acceptance criteria, Coding and deployment with evidence, Features to adapt, Mornye: research and proposed design, Providers and subscriptions, Resource and reliability targets, Runtime layout, Self-improvement (+2 more)

### Community 7 - "main.rs"
Cohesion: 0.15
Nodes (6): Cli, Command, Ask, Chat, Doctor, Serve

### Community 8 - "telegram.rs"
Cohesion: 0.44
Nodes (6): api(), authorized(), flush(), queue(), serve(), tick()

## Knowledge Gaps
- **6 isolated node(s):** `mornye`, `Thinking`, `Chat`, `Ask`, `Serve` (+1 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 91 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **2 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Agent` connect `Agent` to `discord.rs`, `Store`, `tools.rs`, `tests.rs`, `telegram.rs`?**
  _High betweenness centrality (0.170) - this node is a cross-community bridge._
- **What connects `mornye`, `Thinking`, `Chat` to the rest of the system?**
  _6 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `discord.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.08865248226950355 - nodes in this community are weakly interconnected._
- **Why does `Store` connect `Store` to `Agent`, `tools.rs`, `tests.rs`?**
  _High betweenness centrality (0.104) - this node is a cross-community bridge._
- **Should `Store` be split into smaller, more focused modules?**
  _Cohesion score 0.12688172043010754 - nodes in this community are weakly interconnected._
- **Why does `Config` connect `tools.rs` to `Store`, `Agent`, `tests.rs`?**
  _High betweenness centrality (0.051) - this node is a cross-community bridge._
- **Should `tools.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.10338680926916222 - nodes in this community are weakly interconnected._