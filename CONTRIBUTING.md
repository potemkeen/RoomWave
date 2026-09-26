# Contributing to RoomWave

Contributions are welcome.

Before making significant architectural changes, please open an Issue or discussion first so the proposed approach can be reviewed.

## Project structure

- `windows-host/` — RoomWave host application for Windows.
- `android-receiver/` — Android client.
- `windows-installer/` — Windows installer and VB-CABLE setup.
- `docs/` — architecture, protocol, development and release documentation.

## Development

See:

- [Development guide](docs/development.md)
- [Architecture](docs/architecture.md)
- [Protocol](docs/protocol.md)
- [Channel routing](docs/channel-routing.md)

Platform-specific release documentation is available in `docs/`.

## Pull requests

When submitting a pull request:

- keep changes focused on one problem;
- avoid unrelated refactoring;
- preserve compatibility with the existing protocol unless the protocol change is intentional;
- document significant architectural or protocol changes;
- add or update tests where appropriate;
- verify that existing tests still pass;
- avoid adding unnecessary buffering or blocking operations to realtime audio paths.

For audio-related changes, pay particular attention to:

- latency;
- jitter;
- buffer growth;
- synchronization between devices;
- underrun and overrun behavior;
- unnecessary allocations or locks on realtime paths;
- multichannel routing correctness.

## Third-party software

Do not add or redistribute third-party binaries without checking their redistribution terms first.

RoomWave uses third-party components that remain subject to their own licenses.

See [Third-Party Software Notices](THIRD_PARTY_NOTICES.md).

## License

By contributing code to RoomWave, you agree that your contribution may be distributed under the project's [MIT License](LICENSE).