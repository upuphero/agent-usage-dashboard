# Claude Code / ccusage 20.0.26 fixture

All JSONL records are synthetic. They contain only usage metadata, synthetic request/message/session IDs, timestamps and model names. No conversation text, authentication, real account IDs or user paths are present.

`daily.json` and `session.json` are actual stdout captured from the integrity-verified Windows x64 npm native binary, with `CLAUDE_CONFIG_DIR` restricted to `logs/`, a fresh empty HOME/cache, explicit `{}` config and unreachable HTTP(S) proxies:

```text
ccusage claude daily --json --offline --breakdown --mode calculate --order asc --timezone America/Phoenix --config <empty.json>
ccusage claude session --json --offline --breakdown --mode calculate --order asc --timezone America/Phoenix --config <empty.json>
```

Expected independent views:

| View | Reported total |
| --- | ---: |
| Daily 2026-10-03 | 200 |
| Daily 2026-10-04 | 315 |
| Daily totals | 515 |
| Session a, spans Phoenix midnight | 500 |
| Session b | 15 |
| Session totals | 515 |

Daily and Session are alternative views of the same usage. Model breakdowns are children of each parent row, not additional usage. Unknown model `synthetic-unpriced-model` has `missingPricing=true`, costs `0.0` in upstream JSON, and must map to an unavailable cost. The fixed source output keys are `daily` and `sessions`, not the online guide's illustrative `type/data` variants. `projectPath` in the session fixture is intentionally synthetic; the adapter drops it.

The sidecar verification script copies these fixtures into a temporary space/Unicode path, verifies repeat scans and a 40→45 output-token correction, and generates separate synthetic DST and year-boundary records. Only Windows execution has been performed. See `docs/coordination/data.md` for provenance, hashes and pending Rust/macOS verification.
