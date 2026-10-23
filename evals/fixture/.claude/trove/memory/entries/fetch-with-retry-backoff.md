---
title: fetchWithRetry uses exponential backoff
type: reference
created: 2026-01-06
tags: [http]
---

fetchWithRetry retries upstream requests with exponential backoff so
transient failures do not surface to callers.
