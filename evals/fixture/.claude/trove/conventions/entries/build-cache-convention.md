---
title: Cache artifacts between steps
type: convention
created: 2026-01-09
tags: [build]
---

build_cache compiles artifacts once per pipeline run and reuses them for
later steps. Nothing downstream recompiles inputs that a previous step
already produced.
