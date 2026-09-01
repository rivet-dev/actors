# Changelog

## [1.0.0] - 2026-02-10

### Added
- Initial UPM package release
- Automatic `ENABLE_WEBRTC` scripting define via asmdef `versionDefines`
- README with installation and Edgegap deployment instructions

### Changed (from upstream cakeslice/FishyWebRTC)
- Updated logging API for FishNet 4.6.18 (`NetworkManager.LogWarning()` instead of deprecated `CanLog()`)
- Added `SetHTTPS()`, `SetNoClientPort()`, `GetHTTPS()`, `GetNoClientPort()` methods for runtime configuration
- Fixed Android SDK version in BuildProcessor
