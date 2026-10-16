---
title: CI token has org-wide scope
slug: ci-token
type: gotcha
created: 2026-09-01
tags: [ci, security]
---

The CI pipeline authenticates with @@GH_TOKEN@@, which can write to every repo in the
org. The runner sits at 10.42.7.19 (IPv6 fd12:3456:789a::7) behind the VPN.

**Why it matters:** rotate it when someone leaves.
