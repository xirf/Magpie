# Conversational assistant and Discord progress

Mornye's default role is general-purpose personal assistance: everyday conversation, learning, planning, writing, decisions, and practical help. Coding is available when requested. The system prompt asks for the user's language, the existing character's calm warmth and dry humor, and replies proportional to the conversation. It no longer assumes every host is a 2 GB ARM device or prescribes deployment proposals and skill creation after ordinary conversation. User-edited personality files remain intact.

Discord sessions receive channel-specific context on every model call, including resumed tool turns. Replies should use natural paragraphs and occasional short lists, without tables or routine report headings. CLI sessions are labeled separately. These are model instructions, not a deterministic style guarantee; no paid-model quality evaluation was performed during development.

## Concrete use of the references

- [LangChain: The Art of Loop Engineering](https://www.langchain.com/blog/the-art-of-loop-engineering): distinguish the model/tool loop, verification, event triggers, and improvement from traces. Mornye now publishes typed activity from the actual model/tool loop rather than asking the model to invent progress. Deterministic tests verify the transport and state transitions. Existing action evidence, explicit skill promotion, and scheduled event queue remain the foundation for reviewing and improving behavior. This change does not introduce automatic prompt rewriting, extra judge-model calls, or a new autonomous improvement service.
- [Cloudflare: Build your own vulnerability harness](https://blog.cloudflare.com/build-your-own-vulnerability-harness): keep state outside model context, make resumability explicit, and validate results rather than accepting an HTTP success alone. Status-message IDs and final edit targets live in SQLite; final reply insertion and inbox completion share a transaction. Recovery updates interrupted status messages without replaying actions. Empty provider responses are rejected before entering history as successful assistant turns. No claim is made that every nonempty provider response is semantically correct.
- [Cloudflare security-audit-skill](https://github.com/cloudflare/security-audit-skill), reviewed at `c1c8a8c1471069fb0e188eeaff69b8e8db6564a8`: its AI/LLM guidance distinguishes prompts from deterministic authority controls and local evidence from deployment assumptions. Progress contains fixed descriptions, not model text, shell arguments, file paths, or hidden reasoning. Owner/channel authorization, disabled mentions, exact approval binding, and execution evidence stay in code. Local HTTP fixtures test real Serenity requests. This is an implementation informed by the reference, not a completed security audit or adoption of its full multi-agent audit pipeline.

## Delivery lifecycle

1. The existing owner/channel policy admits a request to SQLite and the worker claims it.
2. The worker sends a brief status message, then stores its Discord message ID on that inbox row. Failure or timeout leaves work free to proceed without a status target.
3. The agent publishes `Thinking` before provider calls and `Tool(name)` before authorized tool execution. A watch channel retains only the latest state; fast intermediate steps may be coalesced.
4. Every four seconds while work is pending, the worker attempts typing and, when activity text changes, a status edit. Each activity request has a short timeout. Status edits are best effort; they never approve tools or establish task success.
5. The final result, including an error or pending approval, is split by UTF-16 length. SQLite commits the final outbox chunks and completed inbox state atomically. The first chunk targets the existing status message; later chunks create messages.
6. Delivery retries preserve the edit target. A 404 while editing means the status is gone and delivery falls back to a new message; other errors retain the outbox entry for retry. On restart, running inbox rows become interrupted and queue a notice targeting their saved status message.

Schema changes are additive and preserve old pending replies. Existing state ownership rules and one-process locks still apply. No new dependencies or configuration keys are required.

There is still a crash/timeout window between Discord accepting a new status or fallback message and SQLite recording it. That may leave an orphan status or duplicate fallback/new reply. Edits of known IDs are repeatable; this is not exactly-once delivery. Activity shares the worker runtime, so synchronous host work can delay typing. Cancellation still follows the existing queue behavior and is not an immediate interruption of an in-flight model or tool call. Token streaming and live gateway behavior have not been validated here.

## Validation

The regression suite uses a local Chat Completions fixture and a local Discord REST fixture with Serenity's proxy setting; no Discord account or paid inference is involved. It checks:

- general-purpose persona and Discord channel instructions actually reach the provider; CLI context remains separate;
- casual conversation completes without tool actions;
- empty HTTP-success provider responses are rejected;
- a five-second shell operation produces repeated typing requests and a command-activity edit, followed by a final edit of the same message;
- outgoing status and final messages disable mentions and omit tool arguments;
- deleted status messages fall back to a new reply;
- migration preserves old outbox rows, interrupted statuses survive restart, and only the first long-reply chunk edits the original status;
- existing authorization, approval, persistence, memory, indexing, and bounded-output tests remain passing.

Run `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`, and `cargo build --locked --release` from the repository root. A deployment smoke test still needs the bot's credentials and an authorized Discord conversation; local fixtures cannot establish real gateway permissions, rate limits, or model tone quality.
