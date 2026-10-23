---
title: WAL gotcha: bulk inserts
type: gotcha
created: 2026-01-05
tags: [sqlite]
---

Wrapping many inserts in a single transaction avoids SQLite WAL lock
contention. runBulkInsert demonstrates the pattern: one transaction, chunked
statements.
