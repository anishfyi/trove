---
title: Event sourcing replay
type: context
created: 2026-01-11
tags: [architecture]
---

State is derived by replaying the event log from the beginning; a corrupted
projection is rebuilt by replay rather than by patching tables. See ADR 0002.
