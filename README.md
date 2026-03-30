# Causes

A bug tracker built to free developers from the tedium of manual bug gardening.
It models defects as the interaction between four entities: a **project timeline**, **signs** (machine-observed anomalies), **symptoms** (human-reported issues), and **plans** (intended changes).

## Status

Design phase.
Implementation has not started.
The language and stack are not yet decided.

See [designdocs/](designdocs/) for the full design documentation.

## Components

| Path | Description |
|---|---|
| [services/causes_api](services/causes_api/) | gRPC API server |
| [services/causes_cli](services/causes_cli/) | Command-line client |
| [lib/rust/api_db](lib/rust/api_db/) | Database layer |
| [proto/](proto/) | Protocol buffer definitions |
| [infra/terraform](infra/terraform/) | AWS infrastructure |
