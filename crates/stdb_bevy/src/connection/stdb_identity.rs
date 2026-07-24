use bevy::prelude::*;

#[derive(Resource, Deref)]
pub struct StdbIdentity(pub spacetimedb_sdk::Identity);
