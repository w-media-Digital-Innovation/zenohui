# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a change log](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
