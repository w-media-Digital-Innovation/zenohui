# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a change log](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Bound interactive history per topic, globally by entry count and estimated payload memory, and bound topic metadata.
- Replace the unbounded history handoff channel with a bounded queue; truncate payloads before enqueueing without first copying the full sample.
- Show discarded history entries and preserve history limits across reconnects.

## [0.2.0] - 2026-10-23

### Added

- Settings tab with editable mode, endpoints and multicast discovery
- Background reconnect with retained session on connection failure
- Save/load connection profiles with F2/F3
- Connected remote routers and peers in discovery tabs with connection status
- Mouse navigation between all four tabs

- Peer/router detail panel in scouting tabs with full locator listing

### Changed

- Summarize peer/router address columns with a "+N more" indicator
- Clicking a peer/router row now selects it instead of switching tabs
- CLI now avoids duplicate `clean` alias to satisfy newer clap debug assertions
- Bumped Rust requirement to 1.92.0 and edition to 2024
- Scouting now respects the session mode instead of forcing peer mode

## [0.1.0] - 2025-12-27

### Added

- Initial release of zenohui
- Terminal UI for interactive Zenoh key expression subscription
- Publish command to send payloads to Zenoh keys
- Log command to monitor Zenoh traffic to stdout
- Read-one command to read a single payload value
- Clean command to delete keys/key trees
- Support for TCP/UDP Zenoh peers and listeners
- JSON and MessagePack payload visualization
- Connection history tracking

### Changed

- Forked from [mqttui](https://github.com/EdJoPaTo/mqttui) and replaced the MQTT backend with Zenoh
- Replaced MQTT broker concepts with Zenoh peer/listen architecture
- Adapted CLI for Zenoh key expressions instead of MQTT topics
