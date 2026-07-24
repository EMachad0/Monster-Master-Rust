use bevy::prelude::*;

#[derive(Resource, Deref)]
pub struct StdbIdentity(pub spacetimedb_sdk::Identity);

impl PartialEq<spacetimedb_sdk::Identity> for StdbIdentity {
    fn eq(&self, other: &spacetimedb_sdk::Identity) -> bool {
        &self.0 == other
    }
}
