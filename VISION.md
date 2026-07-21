# Trove
## Beyond Context Windows

---

## Vision

Context windows are expensive.

Memory is cheap.

Trove exists to make large language models operate over repositories,
projects and knowledge bases far larger than their native context window.

Instead of giving models more tokens,
Trove gives them better information.

The objective is not larger prompts.

The objective is higher information density.

---

## Design Principles

Everything is indexed.

Nothing is searched twice.

Raw files are the lowest level of memory.

Every higher layer contains more meaning
while occupying fewer tokens.

Information should become progressively smaller,
more structured,
and easier to retrieve.

---

## Memory Hierarchy

### L0: Working Memory

Current task. Current edits. Current objective. Current reasoning.

Lifetime: minutes.

### L1: Active Symbols

Functions. Classes. Methods. Routes. Queries. Tests.

Only symbols involved in the current task.

### L2: Module Memory

One summary per file. Contains responsibilities, exports, dependencies,
important assumptions, side effects. Approx 200-400 tokens.

### L3: Subsystem Memory

One summary per package (authentication, billing, payments, etc.).
Approx 500-1000 tokens.

### L4: Architecture Memory

High-level design. Data flow. Ownership. Patterns. Service boundaries.
Approx 2000 tokens.

### L5: Historical Memory

Past engineering decisions. Architectural choices. Rejected approaches.
Migration history. Breaking changes. Why things exist.

### L6: Repository Memory

Global knowledge. Glossary. Naming conventions. Coding standards.
Folder conventions. Engineering philosophy. Never loaded unless needed.

---

## Retrieval Pipeline

Never retrieve files directly.

Retrieve: Architecture → Subsystem → Module → Symbol → Function → Source.

Every retrieval should ask: do I need more detail? If no, stop.

---

## Context Budget

Every task receives a budget. Example for 200k context:

| Slice | Share |
|-------|-------|
| Working memory | 10% |
| Architecture | 15% |
| Active files | 20% |
| Dependency expansion | 20% |
| Code | 20% |
| Tool output | 15% |

Never fill context simply because space exists. Unused context is healthy.

---

## Symbol Index

Every repository is parsed. Store functions, classes, interfaces, enums,
SQL, routes, migrations, models, commands, signals, workers, background
jobs, and tests.

Every symbol knows: owner, dependencies, references, git history,
complexity, embedding, hash.

---

## Dependency Graph

Maintain imports, call graph, inheritance, database relationships,
API relationships, background jobs, filesystem dependencies, and
configuration dependencies.

When one node changes, expand only neighboring nodes.

---

## Semantic Layer

Each symbol receives embedding, summary, keywords, intent,
responsibilities, risk score, and examples. Do not embed raw files.
Embed meaning.

---

## Progressive Retrieval

Instead of opening a file directly:

Summary → Signature → Implementation → Related symbols → Entire file → Neighbor files.

The model earns more context.

---

## Engineering Memory

Persist architectural decisions, database decisions, naming decisions,
and framework decisions. This survives sessions.

---

## Execution Memory

Remember opened files, searches performed, commands executed, tests run,
failures encountered, and files modified. Avoid duplicate work.

---

## Patch Memory

Store before hash, after hash, reason, related issue, related PR, author,
date, and future impact. This allows agents to explain why code changed.

---

## Knowledge Compression

Every memory object has: Raw → Summary → Ultra Summary → Keywords → Embedding.

Models choose the smallest useful representation.

---

## Dynamic Context

The context is assembled dynamically. User asks "modify vendor onboarding"
and context becomes architecture, vendor subsystem, relevant models,
services, migrations, tests, and current edits. Not the entire repository.

---

## Freshness

Every object stores last modified, git commit, hash, confidence, and
staleness score. Only stale objects are rebuilt.

---

## Confidence

Every retrieved item includes confidence, reason, retrieval path,
distance, and last verified. The model should know why it received something.

---

## Agent Reflection

After every task, generate lessons, new abstractions, architecture changes,
mistakes, and engineering decisions. Persist automatically.

---

## Repository Understanding

Continuously build glossary, subsystem map, ownership graph, data flow graph,
API graph, database graph, service graph, queue graph, and scheduler graph.
This becomes searchable.

---

## Compression Targets

Raw file → 400 token summary → 100 token summary → 20 token summary →
5 keyword fingerprint → embedding. Never lose provenance.

---

## Memory Objects

Every object contains: id, kind, path, summary, embedding, dependencies,
symbols, keywords, hash, updated_at, importance, confidence, risk.

---

## Context Assembly Algorithm

User Request → Intent Detection → Task Planner → Architecture Retrieval →
Dependency Expansion → Semantic Retrieval → Symbol Expansion → Code Retrieval →
Prompt Assembly → Model → Reflection → Memory Update.

---

## Long-Term Goal

Trove should make a 200k context model behave like it has access to millions
of tokens. Not by increasing context, but by making every retrieved token carry
dramatically more information.

The best context window is the one you never have to fill.
