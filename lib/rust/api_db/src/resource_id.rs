use uuid::Uuid;

/// Define a UUID-based identifier newtype.
///
/// Each generated type has:
/// - `generate()` — create a new random UUID v4
/// - `from_raw(s)` — parse and validate a string as UUID
/// - `as_str()` — borrow the inner string
/// - `Display` — delegates to the inner string
macro_rules! uuid_id {
    ($(#[$meta:meta])* $name:ident, $label:expr) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(String);

        impl $name {
            /// Generate a new random identifier.
            pub fn generate() -> Self {
                Self(Uuid::new_v4().to_string())
            }

            /// Parse and validate a raw string as a UUID.
            pub fn from_raw(s: &str) -> anyhow::Result<Self> {
                s.parse::<Uuid>()
                    .map_err(|e| anyhow::anyhow!(concat!($label, " must be a valid UUID: {}"), e))?;
                Ok(Self(s.to_owned()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

uuid_id!(
    /// Identifies a Causes instance (UUID v4).
    /// Generated at first bootstrap; stable across domain changes.
    InstanceId,
    "InstanceId"
);

uuid_id!(
    /// Identifies a resource across the federation (UUID v4).
    /// Assigned once at resource creation, reused by all subsequent
    /// journal entries regardless of which instance writes them.
    ResourceId,
    "ResourceId"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_valid_uuid() {
        let id = InstanceId::generate();
        InstanceId::from_raw(id.as_str()).unwrap();
    }

    #[test]
    fn from_raw_accepts_valid_uuid() {
        let uuid = Uuid::new_v4().to_string();
        let id = ResourceId::from_raw(&uuid).unwrap();
        assert_eq!(id.as_str(), uuid);
    }

    #[test]
    fn from_raw_rejects_empty() {
        assert!(InstanceId::from_raw("").is_err());
    }

    #[test]
    fn from_raw_rejects_non_uuid() {
        assert!(ResourceId::from_raw("not-a-uuid").is_err());
    }

    #[test]
    fn display_matches_inner() {
        let id = InstanceId::generate();
        assert_eq!(format!("{id}"), id.as_str());
    }
}
