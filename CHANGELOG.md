# Changelog

## [0.1.2] 2024-07-31

### Changed

- ``clnrest-swagger-root`` is now `"/"` again by default, but ``"/"`` will redirect to ``"/swagger-ui"`` while other values will enforce the custom path
- Also call ``plugin.shutdown()`` on a shutdown notification. This is probably not fixing 100% of the shutdown issues (maybe a cln-plugin bug)
- Add ``upgrade`` header to the tests. This is needed to differentiate between an intent to establish a websocket connection and an intent to open the swagger-ui on the same path. Usually websocket clients inlcude this header automatically but the tests were not
- Extracted release binaries are now called ``clnrest-rs``

## [0.1.0] 2024-07-27

### Added

- initial release