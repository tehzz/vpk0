# Changelog

## [0.8.2] 2022-01-15
### Added
* Two helper functions for handling `&[u8]` data: `encode_bytes` and `decode_bytes`

### Changed

### Fixed
* Encoding data that happens to be uncompressible (e.g. `"123456789"`) will not crash

## [0.8.1] 2021-03-11
Initial release. Supports decoding and encoding vpk0 data.
