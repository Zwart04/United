# Native WhatsApp memory comparison — 22 October 2026

Measured on Windows 10 build 10.0.19045, 12 logical cores, using United
0.1.0/0.2.0 development builds. Each row is a single clean start, waited
45 seconds, then sampled for 10 seconds with `tools/bench/measure.ps1`.
RAM sums `PrivateMemorySize64` and `WorkingSet64` for `United` plus matching
`msedgewebview2` processes. This is a comparison, not a guarantee.

| Scenario | Processes | Private MB | Working set MB | CPU % |
|---|---:|---:|---:|---:|
| All six services active | 16 | 2433.5 | 2880.8 | 25.67 |
| WhatsApp WebView hibernated | 15 | 1531.9 | 1895.4 | 3.00 |
| Native bridge helper alone (unpaired) | 1 | 55.7 | 25.0 | — |
| Estimated hybrid: WhatsApp hibernated + helper | 16 | ~1587.6 | ~1920.4 | — |

## What the numbers mean

- Hibernating the WhatsApp WebView removes the largest renderer. On this
  machine it saved roughly **900 MB private** and **985 MB working set**.
- The native helper itself is small (~56 MB private, ~25 MB working set while
  unpaired). The estimated hybrid therefore still saves roughly **840 MB
  private / 960 MB working set** versus the all-WebView build.
- The helper's footprint after a real WhatsApp pairing was not measured; it may
  grow with the linked-device session and message cache.
- The normal and hibernated runs were separate restarts. Memory pressure and
  site content changed between them, so treat the deltas as approximate.
- The CPU figure for the normal run (25.67%) is a short sample and was affected
  by page loading; it is not a steady-state claim.

## Reproduce

```powershell
# Stop United first
.\tools\bench\measure.ps1 -Label "all-six" -Seconds 10
# Hibernate WhatsApp in the UI, then:
.\tools\bench\measure.ps1 -Label "whatsapp-asleep" -Seconds 10
```

Native bridge source and runtime files are documented in
[`native/whatsapp-bridge/README.md`](../../native/whatsapp-bridge/README.md).
