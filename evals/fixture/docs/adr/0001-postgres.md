# ADR 0001: Postgres for the event store

We compared sqlite and postgres for the event store. Postgres won on
concurrent writers: sqlite serializes writes through a single WAL, while
postgres admits many parallel writers and offers standby replicas for reads.
