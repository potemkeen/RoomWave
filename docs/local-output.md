# Selected channels on the PC

The PC is a receiver of the same `AudioBlock` stream as the phones. Its worker joins
the existing subscriber hub, uses a bounded input queue, and renders through event-driven
WASAPI. Packet frame numbers and absolute host presentation deadlines are unchanged.
The local renderer follows `IAudioClock` position/QPC timestamps with bounded fractional
rate correction, fades on starvation/re-alignment, and contributes its output lead to
the existing group delay budget. Endpoint metadata polling runs outside the render worker.

The UI supports an explicit capture source, an independent physical PC output and
multiple selected speaker bits. Exact positions are preserved when the output exposes
them. For stereo outputs, Center is mixed into both front channels; selected left/right
surround channels fold into their respective sides. Unselected channels contribute zero.
Each matrix row is normalized for headroom. Missing source channels remain selected,
contribute silence, and are reported as unavailable rather than reassigned.

## Required source/output separation

Endpoint loopback captures a stream already being rendered. It cannot remove rear
channels from that original hardware playback. Sending that capture back to the same
endpoint also creates feedback. RoomWave therefore refuses local playback without two
explicit, different endpoint IDs. It does not mute other applications or change Windows
defaults automatically.

For front speakers on the PC and rear speakers on phones:

1. Provide a separate 5.1/7.1 source, typically a multichannel virtual render endpoint.
2. Select that endpoint as the output in Windows/the source player and as RoomWave's
   capture source. Apply the source first so RoomWave discovers its actual speaker mask.
3. Choose the physical PC speakers as local output, select FL + FC + FR and enable it.
4. Assign BL/BR or SL/SR to the phones according to the source's actual mask.

An ordinary stereo source cannot provide independent center or rear content. RoomWave
does not synthesize those channels. A virtual driver is not bundled or installed.
For a non-virtual source, its own original physical output may still be audible.

Settings persist in `%LOCALAPPDATA%/RoomWave/local-output.json`. The feature is off by
default and leaves the existing default-endpoint stereo path in place. With local
routing enabled, Test speakers injects the tone once through the shared source hub;
the PC and phones receive it according to their assignments, without an extra local
test renderer. Without local routing, the existing direct Windows test is retained.

Tests cover FL/FC/FR-to-stereo mixing, exclusion of rear channels, mono packet routing,
missing-channel silence and bounded matrix gain. Hardware alignment, end-to-end latency
and long-run stability of the new local sink still require measurement on the selected
source/output combination; UI synchronization is an estimate from Windows audio clocks.
