# Runtime benchmark — 20 October 2026

Measured on Windows 11 build 10.0.26100 using United Release `United.exe` published at 20:10:53 +07:00. Each scenario ran from a clean United/WebView2 stop, waited 55 seconds after launch, then sampled process memory and CPU for 10 seconds. CPU is summed process CPU time divided by logical processor count and sample duration. RAM values sum `PrivateMemorySize64` and `WorkingSet64` for United plus matching WebView2 processes. A later fresh `win-x64` runtime rebuild at 21:58:30 +07:00 passed a separate 65-second hibernation check (0 WebView2 processes). These figures are historical evidence for that build; the current package hash is recorded in [VERIFICATION.md](../VERIFICATION.md).

| Scenario | United / WebView2 processes | Private MB | Working set MB | CPU % |
|---|---:|---:|---:|---:|
| All 6 hibernated at startup | 1 / 0 | 52.7 | 125.1 | 0.00 |
| WhatsApp active, 5 hibernated | 8 / 7 | 854.2 | 651.0 | 0.75 |
| All 6 active | 16 / 15 | 1581.4 | 2045.5 | 0.50 |

## Notes

- Hibernated services stayed asleep: app log reported six dormant services; process capture found zero `msedgewebview2.exe` processes for United.
- One active service creates one shared WebView2 process tree. The tree contains seven processes, not seven independent browser instances.
- Measurements are single-run observations, not guarantees or averages. Site content, login state, cache, machine load, and network can change results. In particular, the 10-second CPU sample is short.
- Initial benchmark attempt was invalid because stale WebView2 processes remained. Those numbers are excluded. This report uses the rerun after stopping and verifying the process tree.
- Original machine setting file was restored byte-for-byte after benchmark (`theme=dark`, zero hibernated services); United was stopped by the benchmark cleanup.

## Raw evidence

- [`raw/2026-10-20-all-hibernated-clean.json`](raw/2026-10-20-all-hibernated-clean.json)
- [`raw/2026-10-20-1-active-5-hibernated.json`](raw/2026-10-20-1-active-5-hibernated.json)
- [`raw/2026-10-20-all-6-active.json`](raw/2026-10-20-all-6-active.json)

Build and tests at measurement time: Debug and Release builds succeeded with 0 warnings/errors; 43/43 tests passed. The current test suite has 54/54 passing. The package produced for that run is not the current downloadable build; see [VERIFICATION.md](../VERIFICATION.md) for the final artifact hash.
