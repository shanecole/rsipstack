# Changelog

All notable changes to rsipstack will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.3.0]: https://github.com/restsend/rsipstack/compare/v0.2.85...v0.3.0
[0.2.85]: https://github.com/restsend/rsipstack/releases/tag/v0.2.85