# Graph Report - mornye  (2026-10-17)

## Corpus Check
- 18 files · ~14,673 words
- Verdict: corpus is large enough that graph structure adds value.
- Unclassified: 6 file(s) not represented in the graph (top: .service 3, (none) 1, .example 1)

## Summary
- 192 nodes · 332 edges · 13 communities (9 shown, 4 thin omitted)
- Extraction: 100% EXTRACTED · 0% INFERRED · 0% AMBIGUOUS
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- discord.rs
- Store
- Agent
- tools.rs
- Mornye
- tests.rs
- provider.rs
- main.rs
- graph.rs
- AGENTS.md
- mornye
- Mornye: research and proposed design
- telegram.rs

## God Nodes (most connected - your core abstractions)
1. `Store` - 20 edges
2. `Agent` - 19 edges
3. `agent()` - 12 edges
4. `Config` - 11 edges
5. `Queue` - 11 edges
6. `Mornye: research and proposed design` - 10 edges
7. `serve()` - 9 edges
8. `Mornye` - 9 edges
9. `tick()` - 8 edges
10. `B860H validation â€” 2026-10-17` - 8 edges

## Surprising Connections (you probably didn't know these)
- `Agent` --references--> `Config`  [EXTRACTED]
  src/agent.rs → src/config.rs
- `Agent` --references--> `Provider`  [EXTRACTED]
  src/agent.rs → src/provider.rs
- `Agent` --references--> `Store`  [EXTRACTED]
  src/agent.rs → src/store.rs
- `dispatch_schedule()` --references--> `Agent`  [EXTRACTED]
  src/discord.rs → src/agent.rs
- `serve()` --references--> `Agent`  [EXTRACTED]
  src/discord.rs → src/agent.rs

## Import Cycles
- None detected.

## Communities (13 total, 4 thin omitted)

### Community 0 - "discord.rs"
Cohesion: 0.14
Nodes (9): authorized(), chunks(), dispatch_schedule(), durable_queue_deduplicates_and_never_replays_running_approvals(), Handler, ingress_policy_and_unicode_limits(), Queue, reply_builder() (+1 more)

### Community 2 - "Agent"
Cohesion: 0.28
Nodes (6): Agent, budget_rows(), DEFAULT_CHARACTER_PROFILE, review(), RULES, unresolved()

### Community 3 - "tools.rs"
Cohesion: 0.16
Nodes (10): Config, drain(), execute(), MAX_FILE, OUTPUT_LIMIT, path(), prepare(), run_shell() (+2 more)

### Community 4 - "Mornye"
Cohesion: 0.06
Nodes (26): Fictional reference knowledge, Mornye-inspired character profile, Roleplay boundary, Sources, Voice and conduct, Discord implementation in this revision, Pinned sources and findings, Remaining implementation priorities (+18 more)

### Community 5 - "tests.rs"
Cohesion: 0.17
Nodes (17): agent(), approval_is_exact_single_use_and_survives_restart(), call(), changed_file_cannot_be_deleted_with_old_approval(), character_profile_is_installed_by_default_and_user_edits_survive_restart(), command_timeout_is_recorded_as_failure(), config(), context_trimming_preserves_complete_tool_groups() (+9 more)

### Community 7 - "main.rs"
Cohesion: 0.20
Nodes (6): Cli, Command, Ask, Chat, Doctor, Serve

### Community 8 - "graph.rs"
Cohesion: 0.18
Nodes (6): index(), index_file(), read(), search(), setup(), walk()

### Community 11 - "Mornye: research and proposed design"
Cohesion: 0.20
Nodes (10): Build sequence and acceptance criteria, Coding and deployment with evidence, Features to adapt, Mornye: research and proposed design, Providers and subscriptions, Resource and reliability targets, Runtime layout, Self-improvement (+2 more)

### Community 12 - "telegram.rs"
Cohesion: 0.42
Nodes (6): api(), authorized(), flush(), queue(), serve(), tick()

## Knowledge Gaps
- **5 isolated node(s):** `mornye`, `Chat`, `Ask`, `Serve`, `Doctor`
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 72 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **4 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Agent` connect `Agent` to `discord.rs`, `Store`, `tools.rs`, `provider.rs`, `telegram.rs`?**
  _High betweenness centrality (0.162) - this node is a cross-community bridge._
- **What connects `mornye`, `Chat`, `Ask` to the rest of the system?**
  _5 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `discord.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.14153846153846153 - nodes in this community are weakly interconnected._
- **Why does `Store` connect `Store` to `graph.rs`, `Agent`, `tools.rs`?**
  _High betweenness centrality (0.123) - this node is a cross-community bridge._
- **Should `Mornye` be split into smaller, more focused modules?**
  _Cohesion score 0.06451612903225806 - nodes in this community are weakly interconnected._
- **Why does `Config` connect `tools.rs` to `graph.rs`, `Agent`, `provider.rs`?**
  _High betweenness centrality (0.047) - this node is a cross-community bridge._