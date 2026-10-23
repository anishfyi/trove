# ADR 0002: Event sourcing

All state derives from the event log. Projections replay events from the
beginning, so a corrupted projection is rebuilt by replay rather than by
patching tables.
