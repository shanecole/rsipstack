# Changelog

All notable changes to rsipstack will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.3] - 2025-11-28

### Upstream Sync

Merged changes from upstream [restsend/rsipstack](https://github.com/restsend/rsipstack) v0.2.93.

- **Last synced upstream commit**: `26bfddc` (for future merge reference)
- **Upstream version range**: v0.2.87 through v0.2.93

### Added

- **`update_route_set_from_response()` method** on DialogInner to update route set from Record-Route headers
  - Client dialogs now properly learn their route set from 2xx responses (RFC 3261 §12.1.2)
  - Ensures subsequent in-dialog requests reuse the same proxy chain

### Changed

- **Route set updates from provisional responses**: 18x responses (Ringing/SessionProgress) with to-tag now trigger route set updates
- **Route set updates from 200 OK**: Final successful responses now update the route set before transitioning to Confirmed state

### Fixed

- **Issue #44 fix from upstream**: Route set update from 200 OK response now implemented correctly
- **18x with to-tag handling**: Route set is now properly captured from early dialog responses

### Tests

- **New test**: `test_route_set_updates_from_200_ok_response` verifying route set extraction and reversal from Record-Route headers

## [0.3.2] - 2025-11-14

### Added

- **set_remote_target() method** on Dialog and DialogInner to update remote target URI and Contact header
  - Useful after receiving 2xx/UPDATE responses with new Contact information
  - Ensures subsequent in-dialog requests route to the latest remote target

### Changed

- **ACK handling refactored** (merged from upstream v0.2.91):
  - Simplified `make_ack()` API: now takes `request_uri` directly instead of computing internally
  - Request URI computation moved to callers for better separation of concerns
  - Enhanced `SipAddr` to `Uri` conversion to include transport parameters
  - Now properly handles TCP, TLS, WS, WSS, TlsSctp, and Sctp transports in URI params
  - Improved destination handling for 2xx vs non-2xx ACK responses

### Fixed

- **accept_with_public_contact bug**: Custom Contact headers in the `headers` parameter are now properly preserved
  - Fixed header ordering: local_contact is added before merging custom headers
  - Removed Contact from retain() filter to prevent removal of custom Contact headers
- **Authentication retry bug** (upstream issue #40): Via branch parameter now properly updated during auth retry
  - Old branch parameter is removed before adding new one
  - Prevents multiple branch parameters in Via header (RFC 3261 violation)
  - Added comprehensive test coverage in `test_authenticate.rs`
- **Code organization**: Moved `SipAddr` conversion implementations from `connection.rs` to `sip_addr.rs`

### Tests

- **New test file**: `src/dialog/tests/test_authenticate.rs` with comprehensive authentication tests
  - Verifies old branch parameter removal
  - Verifies new branch parameter generation
  - Verifies rport parameter addition

## [0.3.1] - 2025-11-10

### Added

- **PRACK support (RFC 3262)**: Provisional Response Acknowledgment for reliable provisional responses
  - Added `prepare_prack_request()` method to generate PRACK requests
  - Added `send_prack_request()` method for sending PRACK with authentication support
  - Added `handle_prack()` method in ServerInviteDialog
  - Added `parse_rseq_header()` and `parse_rack_header()` helper functions
  - Added comprehensive PRACK test coverage in `src/dialog/tests/test_prack.rs`
  - Supports 100rel extension with RSeq/RAck headers

### Changed

- **Dialog confirmation handling improved**: Client invite dialogs are now only inserted into DialogLayer after successful (2xx) responses
- **Dependency version relaxed**: rsip-dns version constraint changed to "0" for workspace patch compatibility

### Fixed

- **Clippy warnings**: Removed unnecessary borrows in PRACK tests and other minor clippy fixes

## [0.3.0] - 2025-10-30

### Added

- **IdentityBehavior enum** to control when User-Agent/Server headers are added to messages:
  - `Always`: Always add identity headers (default, backward compatible)
  - `Never`: Never add headers automatically (full control mode for proxies)
  - `OnlyGenerated`: Only add headers to generated messages, not forwarded ones (proxy mode)
- **Separate user_agent and server fields** in `EndpointInner` and `EndpointBuilder`
- **New builder methods**:
  - `with_user_agent()`: Set User-Agent string for requests
  - `with_server()`: Set Server string for responses
  - `with_identity()`: Convenience method to set both User-Agent and Server
  - `with_identity_behavior()`: Configure when headers are added
- **Identity field to EndpointOption** (`identity_behavior`)
- **Smart defaults**: If neither user_agent nor server is set, both default to "rsipstack/VERSION"

### Changed

- **RFC 3261 COMPLIANCE FIX**: Responses now correctly use `Server` header instead of `User-Agent`
  - `EndpointInner::make_response()` now adds `Server` header (was `User-Agent`)
  - `DialogInner::make_response()` now adds `Server` header (was `User-Agent`)
  - This is a **breaking change** if you were relying on the incorrect behavior
- **EndpointInner fields** changed:
  - `user_agent: String` → `user_agent: Option<String>`
  - Added `server: Option<String>` field
- **EndpointBuilder fields** changed:
  - `user_agent: String` → `user_agent: Option<String>`
  - Added `server: Option<String>` field
- **EndpointInner::new()** signature updated:
  - Added `server: Option<String>` parameter after `user_agent`
- **ACK request handling** improved:
  - For 2xx ACK (end-to-end), User-Agent behavior depends on `IdentityBehavior`
  - For non-2xx ACK (hop-by-hop), User-Agent is added based on `IdentityBehavior`
- **Default behavior**: When no identity is configured, defaults to "rsipstack/0.3.0" (backward compatible)

### Fixed

- **RFC 3261 violation**: Responses were incorrectly using `User-Agent` header instead of `Server`
- **Proxy use case**: Proxies can now distinguish between generated and forwarded messages

### Migration Guide

#### For existing UAC/UAS applications (no changes required):

```rust
// Old code still works (backward compatible)
let endpoint = EndpointBuilder::new().build();
// Defaults: user_agent = "rsipstack/0.3.0", server = "rsipstack/0.3.0"
```

#### For custom identity strings:

```rust
// Old way (deprecated but still works)
let endpoint = EndpointBuilder::new()
    .with_user_agent("MyApp/1.0")  // Sets both user_agent AND server
    .build();

// New way (recommended)
let endpoint = EndpointBuilder::new()
    .with_identity("MyApp/1.0")  // Explicitly set both
    .build();

// Or set separately
let endpoint = EndpointBuilder::new()
    .with_user_agent("MyClient/1.0")
    .with_server("MyServer/2.0")
    .build();
```

#### For proxy applications:

```rust
// Proxy mode: only add headers to generated messages
let endpoint = EndpointBuilder::new()
    .with_identity("sip-proxy/1.2.0")
    .with_identity_behavior(IdentityBehavior::OnlyGenerated)
    .build();

// Full control mode: never add headers automatically
let endpoint = EndpointBuilder::new()
    .with_identity_behavior(IdentityBehavior::Never)
    .with_inspector(Box::new(MyCustomInspector))
    .build();
```

### Breaking Changes

1. **Responses now use `Server` header**: If your application was checking for `User-Agent` in responses, update to check for `Server` instead
2. **Internal field types changed**: If you were directly accessing `endpoint.inner.user_agent`, it's now `Option<String>`
3. **EndpointInner::new() signature**: Added `server` parameter (only affects if you're calling this directly)

### Removed

- None

## [0.2.85] - Previous releases

See git history for changes in previous versions.

[0.3.3]: https://github.com/shanecole/rsipstack/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/shanecole/rsipstack/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/restsend/rsipstack/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/restsend/rsipstack/compare/v0.2.85...v0.3.0
[0.2.85]: https://github.com/restsend/rsipstack/releases/tag/v0.2.85