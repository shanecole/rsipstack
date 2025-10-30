//! Types for identity header management
//!
//! This module defines types that control how User-Agent and Server headers
//! are added to SIP messages according to RFC 3261.

/// Controls when identity headers (User-Agent/Server) are added to SIP messages
///
/// RFC 3261 specifies:
/// - User-Agent header is used in **requests** to identify the UAC software
/// - Server header is used in **responses** to identify the UAS software
///
/// This enum allows fine-grained control over when these headers are automatically
/// added to messages, supporting different use cases like UAC/UAS endpoints and proxies.
///
/// # Examples
///
/// ## User Agent Client/Server (Always add headers)
///
/// ```rust,no_run
/// use rsipstack::{EndpointBuilder, IdentityBehavior};
///
/// # fn example() {
/// let endpoint = EndpointBuilder::new()
///     .with_identity("MyPhone/1.0")
///     .with_identity_behavior(IdentityBehavior::Always)
///     .build();
/// # }
/// ```
///
/// ## Stateful Proxy (Only add to generated messages)
///
/// ```rust,no_run
/// use rsipstack::{EndpointBuilder, IdentityBehavior};
///
/// # fn example() {
/// let endpoint = EndpointBuilder::new()
///     .with_identity("sip-proxy/1.2.0")
///     .with_identity_behavior(IdentityBehavior::OnlyGenerated)
///     .build();
/// # }
/// ```
///
/// ## Proxy with Full Control (Never add headers)
///
/// ```rust,no_run
/// use rsipstack::{EndpointBuilder, IdentityBehavior};
///
/// # fn example() {
/// let endpoint = EndpointBuilder::new()
///     .with_identity_behavior(IdentityBehavior::Never)
///     .build();
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdentityBehavior {
    /// Always add identity headers to messages
    ///
    /// This mode is suitable for User Agent Client (UAC) and User Agent Server (UAS)
    /// endpoints that originate or terminate calls.
    ///
    /// **Behavior:**
    /// - Requests: Add User-Agent header with the configured identity string
    /// - Responses: Add Server header with the configured identity string
    ///
    /// **Use case:** SIP phones, softphones, PBX endpoints
    ///
    /// **RFC 3261 Compliance:** ✅ Fully compliant when used for UAC/UAS
    Always,

    /// Never add identity headers automatically
    ///
    /// This mode gives the application complete control over identity headers.
    /// The application must use a MessageInspector to add appropriate headers.
    ///
    /// **Behavior:**
    /// - Requests: No User-Agent header added
    /// - Responses: No Server header added
    ///
    /// **Use case:** Proxies that need custom header management logic,
    /// or applications that want to use MessageInspector for all header operations
    ///
    /// **RFC 3261 Compliance:** ⚠️ Application is responsible for compliance
    Never,

    /// Add identity headers only to newly generated messages
    ///
    /// This mode is suitable for stateful proxies that need to distinguish between
    /// messages they generate (like 100 Trying, 408 Timeout) and messages they
    /// forward from other endpoints.
    ///
    /// **Behavior:**
    /// - Generated requests: Add User-Agent header
    /// - Generated responses: Add Server header
    /// - Forwarded messages: Preserve existing headers (no modification)
    ///
    /// **Use case:** SIP proxies, B2BUAs (Back-to-Back User Agents)
    ///
    /// **RFC 3261 Compliance:** ✅ Fully compliant for proxy behavior
    ///
    /// **Note:** Currently behaves the same as `Always` because rsipstack's
    /// message creation methods (`make_request`, `make_response`) are only
    /// called for generated messages, not forwarded ones. This variant exists
    /// for future extensibility and semantic clarity.
    OnlyGenerated,
}

impl Default for IdentityBehavior {
    /// Default is `Always` for backward compatibility
    ///
    /// This ensures existing code that doesn't explicitly set identity behavior
    /// continues to work as before (always adding headers).
    fn default() -> Self {
        Self::Always
    }
}

impl std::fmt::Display for IdentityBehavior {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Always => write!(f, "Always"),
            Self::Never => write!(f, "Never"),
            Self::OnlyGenerated => write!(f, "OnlyGenerated"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_always() {
        assert_eq!(IdentityBehavior::default(), IdentityBehavior::Always);
    }

    #[test]
    fn test_display() {
        assert_eq!(IdentityBehavior::Always.to_string(), "Always");
        assert_eq!(IdentityBehavior::Never.to_string(), "Never");
        assert_eq!(IdentityBehavior::OnlyGenerated.to_string(), "OnlyGenerated");
    }

    #[test]
    fn test_equality() {
        assert_eq!(IdentityBehavior::Always, IdentityBehavior::Always);
        assert_ne!(IdentityBehavior::Always, IdentityBehavior::Never);
        assert_ne!(IdentityBehavior::Never, IdentityBehavior::OnlyGenerated);
    }

    #[test]
    fn test_clone() {
        let behavior = IdentityBehavior::OnlyGenerated;
        let cloned = behavior;
        assert_eq!(behavior, cloned);
    }

    #[test]
    fn test_debug() {
        let behavior = IdentityBehavior::Always;
        assert_eq!(format!("{:?}", behavior), "Always");
    }
}
