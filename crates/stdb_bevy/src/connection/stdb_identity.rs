use bevy::prelude::*;

/// The connected client's identity, as issued by the server.
///
/// Carries the identity's raw 32 bytes rather than the SpacetimeDB SDK's own type, so the
/// Bridge's core owns the shape of its public API and stays independent of the SDK. The SDK
/// adapter converts once, at the connection seam.
#[derive(Resource, Deref, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StdbIdentity([u8; 32]);

impl StdbIdentity {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl PartialEq<[u8; 32]> for StdbIdentity {
    fn eq(&self, other: &[u8; 32]) -> bool {
        &self.0 == other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_identity_matches_the_bytes_it_was_built_from() {
        let identity = StdbIdentity::new([7; 32]);

        assert!(
            identity == [7; 32],
            "the Game answers \"is this mine?\" by comparing the resource against a row's \
             identity bytes, so the two must compare equal",
        );
    }

    #[test]
    fn an_identity_does_not_match_another_players_bytes() {
        let identity = StdbIdentity::new([7; 32]);

        assert!(
            identity != [9; 32],
            "a row belonging to another player must not read as the local identity",
        );
    }
}
