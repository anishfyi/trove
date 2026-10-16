---
title: Use Postgres for the ledger
slug: use-postgres
type: decision
created: 2026-07-30
tags: [db]
---

Project Nightjar needed transactional ledgers, so we moved from DynamoDB to Postgres.

**Why it matters:** do not reintroduce eventually consistent writes on the ledger.
