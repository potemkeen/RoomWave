# Speaker routing

The host reads the original endpoint `IAudioClient::GetMixFormat` structure, preserving
`WAVEFORMATEXTENSIBLE.dwChannelMask`. Interleaved indices are the number of set bits
below each speaker bit. Masks are never inferred from multichannel counts. Legacy
WAVEFORMATEX mono/stereo use their conventional FC / FL+FR positions; legacy
multichannel or inconsistent masks disable routing rather than guessing positions.

Supported layouts include Mono, Stereo, Quad, 5.1 Back (0x3f), 5.1 Side (0x60f),
7.1 (0x63f) and 7.1 Wide (0xff). A metadata worker checks the default render endpoint
every 500 ms. Capture reopens when its ID, rate, channel count or mask changes; global
frame indices and the host presentation timeline survive the reopen. Brief endpoint
loss leaves control connections alive and retries capture. An unavailable speaker
assignment sends silence and remains selected until the user changes it.

Capture still uses event-driven WASAPI, PCM16 at 48 kHz and 240-frame blocks. A single
multichannel block feeds the existing peer workers. Each peer extracts its selected
speaker into mono; no second capture, queue or playback scheduler is introduced.
The default compatibility mode sends the front stereo pair (mono duplicated when
the source is mono). At a stereo endpoint its samples are unchanged.

Assignments are stored in `%LOCALAPPDATA%/RoomWave/channels.json` by discovery device
ID and speaker bit. Absence of an assignment means normal stereo streaming.

## Wire extension

Protocol v4 `ready` advertises `monoPcm: true`. The existing extended header remains
64 bytes, with channel count at byte 6. Count 1 means 480 bytes of PCM16 mono (544-byte
UDP datagram); count 2 keeps the existing 960-byte stereo payload (1024-byte datagram).
Sequence, source frame, capture/read/send/presentation timestamps are unchanged.
The retransmit cache holds already-routed payloads so changing assignments cannot
change an older packet's content. Older receivers retain stereo compatibility and
receive an explicit update error when assigned a mono channel.

Android expands mono into identical L/R samples on the network worker, before the
existing JNI ring buffer. The AAudio callback, output format, hardware buffer and
synchronization controller are unchanged. Both audio output channels therefore play
the assigned content; actual physical speaker use follows the phone's audio route.

## Speaker test and verification

One click starts the test on the current Windows render endpoint and on phones assigned
to the selected speaker. No target selector or connected phone is required for local
testing. The local event-driven WASAPI renderer uses the same mask mapping and tone
generator. During the test, captured loopback is suppressed before network test injection,
including a short drain tail, so the locally rendered signal cannot double on phones.
This starts both outputs from one action; acoustic sample-accurate PC/phone alignment
is not measured or guaranteed.

The 600 ms network test replaces the source block with silence except for the selected speaker.
The signal is a 660 Hz sine (90 Hz for LFE), with 20 ms fades. Only explicitly assigned
devices receive it; normal stereo devices remain silent during the test. It uses the
same routing, UDP and presentation timestamps as ordinary capture.

Rust tests cover speaker order for all supported masks, absent-channel silence,
bit-exact mono extraction, retained timestamps, stereo payload preservation and test
isolation. JVM tests cover mono duplication into both output channels and malformed
packet rejection. Physical 5.1/7.1 switching and before/after LAN latency require the
user's hardware run; no measured latency improvement or equality is claimed here.

Mask ordering reference: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ksmedia/ns-ksmedia-waveformatextensible

## Receiver channel label

Protocol v4 `start` and `ping` messages optionally include `routing`:
`{"speaker":16,"available":true}`. `speaker` is the assigned Windows speaker bit;
null means the normal stereo stream. Availability is evaluated against the current
capture layout. Heartbeats refresh the label after assignments or layouts change.
This metadata does not change UDP PCM framing or playback scheduling. Older clients
ignore it; a new client connected to an older host displays an unspecified channel.
The receiver clears the label on disconnect.
