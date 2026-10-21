# Graph Report - mornye  (2026-10-22)

## Corpus Check
- 18 files · ~15,260 words
- Verdict: corpus is large enough that graph structure adds value.
- Unclassified: 8 file(s) not represented in the graph (top: .service 4, .example 2, (none) 1)

## Summary
- 198 nodes · 343 edges · 12 communities (9 shown, 3 thin omitted)
- Extraction: 100% EXTRACTED · 0% INFERRED · 0% AMBIGUOUS
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `ce0cc40c`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- discord.rs
- Store
- Agent
- tools.rs
- Mornye: research and proposed design
- tests.rs
- anyhow
- main.rs
- Mornye
- AGENTS.md
- mornye
- B860H validation â€” 2026-10-17

## God Nodes (most connected - your core abstractions)
1. `Store` - 20 edges
2. `Agent` - 19 edges
3. `agent()` - 13 edges
4. `Config` - 12 edges
5. `Queue` - 11 edges
6. `Mornye: research and proposed design` - 10 edges
7. `B860H validation â€” 2026-10-17` - 10 edges
8. `serve()` - 9 edges
9. `Mornye` - 9 edges
10. `tick()` - 8 edges

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

## Communities (12 total, 3 thin omitted)

### Community 0 - "discord.rs"
Cohesion: 0.14
Nodes (9): authorized(), chunks(), dispatch_schedule(), durable_queue_deduplicates_and_never_replays_running_approvals(), Handler, ingress_policy_and_unicode_limits(), Queue, reply_builder() (+1 more)

### Community 1 - "Store"
Cohesion: 0.14
Nodes (7): index(), index_file(), read(), search(), setup(), walk(), Store

### Community 2 - "Agent"
Cohesion: 0.18
Nodes (12): Agent, budget_rows(), DEFAULT_CHARACTER_PROFILE, review(), RULES, unresolved(), api(), authorized() (+4 more)

### Community 3 - "tools.rs"
Cohesion: 0.15
Nodes (10): Config, drain(), execute(), MAX_FILE, OUTPUT_LIMIT, path(), prepare(), run_shell() (+2 more)

### Community 4 - "Mornye: research and proposed design"
Cohesion: 0.08
Nodes (19): Fictional reference knowledge, Mornye-inspired character profile, Roleplay boundary, Sources, Voice and conduct, Build sequence and acceptance criteria, Coding and deployment with evidence, Features to adapt (+11 more)

### Community 5 - "tests.rs"
Cohesion: 0.16
Nodes (19): agent(), approval_is_exact_single_use_and_survives_restart(), auto_approval_resumes_a_previously_pending_action_once_after_restart(), call(), changed_file_cannot_be_deleted_with_old_approval(), character_profile_is_installed_by_default_and_user_edits_survive_restart(), command_timeout_is_recorded_as_failure(), config() (+11 more)

### Community 7 - "main.rs"
Cohesion: 0.20
Nodes (6): Cli, Command, Ask, Chat, Doctor, Serve

### Community 8 - "Mornye"
Cohesion: 0.22
Nodes (9): B860H deployment, Coding and deployment, Commands and checks, Discord / continuous operation, Memory, personality, and learning, Mornye, Start on your development machine, Tool approval (+1 more)

### Community 11 - "B860H validation â€” 2026-10-17"
Cohesion: 0.20
Nodes (10): B860H validation â€” 2026-10-17, Build and execution, Device, Discord deployment, 2026-10-18, Discord revision validation, 2026-10-17, Files on the device, Memory correction revision validation, 2026-10-17, Memory observation (+2 more)

## Knowledge Gaps
- **5 isolated node(s):** `mornye`, `Chat`, `Ask`, `Serve`, `Doctor`
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 76 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **3 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Agent` connect `Agent` to `discord.rs`, `Store`, `tools.rs`, `anyhow`?**
  _High betweenness centrality (0.157) - this node is a cross-community bridge._
- **What connects `mornye`, `Chat`, `Ask` to the rest of the system?**
  _5 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `discord.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.14153846153846153 - nodes in this community are weakly interconnected._
- **Why does `Store` connect `Store` to `Agent`, `tools.rs`, `anyhow`?**
  _High betweenness centrality (0.119) - this node is a cross-community bridge._
- **Should `Store` be split into smaller, more focused modules?**
  _Cohesion score 0.14461538461538462 - nodes in this community are weakly interconnected._
- **Why does `Config` connect `tools.rs` to `Store`, `Agent`, `anyhow`?**
  _High betweenness centrality (0.053) - this node is a cross-community bridge._
- **Should `Mornye: research and proposed design` be split into smaller, more focused modules?**
  _Cohesion score 0.08333333333333333 - nodes in this community are weakly interconnected._