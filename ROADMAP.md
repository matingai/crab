# Roadmap

Crab is an active 0.1.x prototype. The roadmap is intentionally ambitious, but each item
should make the local agent runtime more inspectable, safer, and easier to extend.

## Now

- Stabilize the core CLI and desktop bridge workflow.
- Improve agent loop observability: goal state, solve traces, tool evidence, and recovery
  events.
- Harden local runtime safety around files, browser state, Office workflows, and optional
  shell execution.
- Keep open-source hygiene strong: docs, screenshots, CI, templates, and privacy review.

## Next

- Make worker delegation more structured, with clearer input scope, output schemas, and
  evidence reconciliation.
- Add richer desktop views for agent timeline, approvals, memory, skills, and delegated
  runs.
- Improve model routing for primary, auxiliary, summarization, and worker models.
- Expand examples that show coding, research, document, browser, and automation workflows.
- Move legacy compatibility names toward Crab-branded runtime paths where practical.

## Later

- Plugin and MCP marketplace-style discovery for local capabilities.
- Stronger policy layer for tool permissions, workspace boundaries, and approvals.
- Shareable task traces that can be scrubbed for public debugging.
- Benchmarks for long-running agent tasks, recovery quality, and delegation usefulness.
- Native packaging for a polished local desktop agent environment.

## Non-Goals For 0.1.x

- Pretending the API is stable before it is.
- Optimizing for cloud-hosted multi-tenant deployment before local safety is mature.
- Hiding model/tool execution behind a black box. Crab should make agent work visible.
