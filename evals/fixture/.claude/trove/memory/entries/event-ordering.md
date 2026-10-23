---
title: Event ordering by aggregate key
type: gotcha
created: 2026-01-07
tags: [kafka]
---

publishEvent keys every message by aggregate id, so a single partition
preserves order for that aggregate. Parallel partitions trade per-aggregate
sequencing for throughput.
