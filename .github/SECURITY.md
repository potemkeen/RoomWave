# Security Policy

## Reporting a vulnerability

If you discover a security vulnerability in RoomWave, please do not publish it in a public GitHub Issue.

Instead, report it privately to the repository owner:

[https://github.com/potemkeen](https://github.com/potemkeen)

Please include:

- a clear description of the issue;
- affected component or platform;
- steps to reproduce;
- potential impact;
- logs or screenshots if they are relevant;
- any suggested mitigation, if known.

Please avoid including sensitive personal data, credentials, private network information or other secrets in reports.

## Scope

Security reports are especially relevant for issues involving:

- local network communication;
- device discovery;
- malformed network packets;
- unauthorized access to RoomWave hosts or clients;
- installer behavior;
- system audio configuration;
- privilege escalation;
- unsafe handling of files or configuration data;
- denial-of-service conditions caused by remote devices on the local network.

RoomWave is currently designed for use on trusted local networks. Audio and control traffic may not be encrypted.

## Supported versions

RoomWave is under active development.

Security fixes are generally applied to the latest available version. Older releases may not receive fixes.
