---
title: Postgres chosen for the event store
type: context
created: 2026-01-10
tags: [postgres]
---

ADR 0001 records why postgres won over sqlite for the event store: it admits
concurrent writers and offers standby replicas for reads.
