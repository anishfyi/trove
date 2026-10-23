---
title: Token bucket for queue admission
type: convention
created: 2026-01-08
tags: [queues]
---

Admission goes through a token bucket with fixed capacity and a steady
refill rate. RateLimiter implements it; queue depth stays bounded and brief
surges drain instead of cascading.
