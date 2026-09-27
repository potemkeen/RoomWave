# Xiaomi unrestricted battery test

Source: `session-1790538719974-31128.jsonl`, 127.176 seconds.
Compared with `session-1790537637748-31128.jsonl`. Deduplicated receiver reports;
counter deltas exclude pre-recording history (Xiaomi started at 560 missing PCM).

Battery exemption is confirmed: Xiaomi `ignoringBatteryOptimizations=true` throughout.
Both phones are on battery, both at 5180 MHz. Samsung's screen is off throughout.
Xiaomi screen-off broadcast occurs at 71.81 s; reported state changes around 73.6 s.
Neither device enters deviceIdle during this run. No low-power mode is reported.

| Xiaomi state | Reported interval | New missing PCM | Missing / consumed | Median latency | Median output |
|---|---|---|---|---|---|
| Screen on | 0–73.1 s | 2 | 0.014% | 118.1 ms | 85.6 ms |
| Screen off | 73.6–126.7 s | 406 | 3.783% | 143.4 ms | 109.4 ms |

Samsung: zero missing PCM, late packets and xruns. Both receivers report zero xruns
and native slot collisions. Xiaomi: 413 new late packets during screen-off; no
FEC/retransmission packets played as recoveries. 308 gaps expired without a NACK
during screen-off. Missing PCM is not equivalent to permanent UDP loss.

Xiaomi stays NONE/shared, 770-frame burst and 1540-frame buffer; 12 candidate
formats did not obtain low latency. UDP AUDIO priority (-16) was granted.
Output lead increases with screen-off again. Median rolling transit p99 estimate
changes from 6.4 to 145.3 ms; rolling transit maximum reaches 202.9 ms. Receive
interval maximum rises from 26.0 to 209.2 ms, whereas timeout-overrun maximum
stays at 7.123 ms and callback gap maximum stays at 42.945 ms.

Host subscriber queue drops increased by two for Xiaomi (not zero), insufficient
to explain 406 screen-off losses; no trimmed/skipped capture frames. Host send
interval maximum is 27.1 ms. Samsung is now on battery too, eliminating the prior
USB-power difference as an explanation for its clean result in this run.

Conclusion: battery exemption did not resolve the symptom. The 3.78% vs prior
3.46% screen-off missing PCM rates should not be called a regression: durations,
transitions and adaptive budgets differ. There is no demonstrated improvement.
Long pauses in packet delivery without comparable UDP timeout wakeup stalls favor
network/radio/OS buffering rather than heavy application CPU work. Audio output
also independently becomes slower. These logs do not locate the exact vendor
driver mechanism or prove an AP fault.

Next investigation should use AudioFlinger/AAudio capabilities and OboeTester for
the output path, and a controlled alternative network path/AP plus system traces
for delivery. Repeating battery-exemption toggles alone is unlikely to help.
