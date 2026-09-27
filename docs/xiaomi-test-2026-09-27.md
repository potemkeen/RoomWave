# Xiaomi receiver experiment: 2026-09-27

Source: session-1790537637748-31128.jsonl (510.189 seconds). Both receivers
report native-format-probe-v3. No counter resets observed. Counter differences
exclude activity before recording: Xiaomi already had 611 missing PCM blocks.
Rows were deduplicated by receiver report timestamp. Screen intervals below use
reported device state; telemetry lags broadcast events by a few seconds.

| Xiaomi reported screen | Approx interval | New missing PCM / consumed blocks | Median estimated latency | Median audio output |
|---|---|---|---|---|
| On | 0–133 s | 1 / 26,669 (0.004%) | 120.7 ms | 86.2 ms |
| Off | 134–415 s | 1,950 / 56,321 (3.462%) | 148.6 ms | 112.5 ms |
| On again | 416–510 s | 8 / 18,974 (0.042%) | 115.4 ms | 82.8 ms |

The final on interval includes transition losses; from 425 s onward both missing
PCM and late counters are flat. Total new Xiaomi missing PCM: 1,959; late: 2,111.
Samsung: zero new missing/late PCM in all three phases. Both have zero hardware
xruns and queue collisions. Missing PCM is not identical to lost UDP datagrams.

## Findings

- All 12 Xiaomi candidates returned NONE/shared, 770-frame bursts, 1540-frame
  buffers, actual 48 kHz. PCM16/native-rate selection did not unlock a fast path.
  Samsung retained exclusive/low-latency, 240/480 frames and one open attempt.
- Xiaomi off-screen output cost grew about 26 ms. Samsung output stayed around
  17–18 ms, but its end-to-end estimate increased to 148 ms because of the shared
  host budget. These estimates exclude pre-capture and acoustic latency.
- Xiaomi off-screen rolling transit p99 estimates had median 147 ms, peak 185 ms;
  rolling transit maximum reached 214 ms. Receive interval lifetime maximum rose
  to 350 ms. These are different metrics, not a single packet's complete trace.
- UDP priority -16 was granted. Timeout-overrun lifetime maximum remained 7.125 ms;
  callback gap maximum stayed 46.696 ms and work maximum reached only 0.634 ms.
  This favors delayed delivery upstream of application processing over long CPU
  stalls, but does not prove whether AP, radio or OS network buffering caused it.
- No FEC or retransmission blocks were actually played as recoveries. XOR parity
  cannot solve a burst in which both originals and parity miss their deadlines.
- Both on 5180 MHz; Samsung USB powered, Xiaomi on battery. Neither had a battery
  optimization exemption. Xiaomi entered deviceIdle late (~379 s); losses already
  occurred before that (1,482 new losses over 135–375 s). Doze alone is insufficient
  as an explanation. Both wake locks remained held.
- Host queue drops, trimmed/skipped frames are zero; send interval maximum <27 ms.
  This does not resemble a 200–350 ms host-wide sending freeze.

## Conclusions / next controlled checks

No demonstrated latency improvement for Xiaomi; do not claim a causal loss-rate
improvement versus earlier runs with different budgets/conditions. New diagnostics
did establish that candidate negotiation failed and screen-off affects both delivery
and presentation lead. Do not lower buffers to conceal this.

Next: same on/off/on run with Xiaomi battery policy set to unrestricted, preserving
the AP, charging status and host configuration. Separately compare OboeTester output
backends and inspect AudioFlinger/effects via ADB. Keep-screen mode is a workaround
to test separately, not a screen-off fix. Perfetto is useful if identifying the exact
radio/network/scheduler mechanism is still necessary. No source changes in this
analysis turn.
