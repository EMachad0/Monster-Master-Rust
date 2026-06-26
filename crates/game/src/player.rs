use bevy::prelude::*;
use spacetimedb_sdk::Identity;
use stdb_bevy::{RowDeleted, RowEntities, RowInserted, StdbSync, Subscription};

pub fn subscribe_to_players(mut commands: Commands) {
    commands.spawn(Subscription::query(
        "SELECT * FROM player WHERE online = true;",
    ));
}

const PLAYER_COLORS: [Color; 6] = [
    Color::srgb(1.0, 0.0, 0.0),
    Color::srgb(0.0, 1.0, 0.0),
    Color::srgb(0.0, 0.0, 1.0),
    Color::srgb(1.0, 1.0, 0.0),
    Color::srgb(0.0, 1.0, 1.0),
    Color::srgb(1.0, 0.0, 1.0),
];

#[derive(Debug, Default, Resource)]
pub struct OnlineCounter(pub i32);

impl OnlineCounter {
    pub fn inc(&mut self) {
        self.0 += 1;
    }

    pub fn dec(&mut self) {
        self.0 -= 1;
    }
}

#[derive(Clone, Copy, Component)]
#[relationship(relationship_target = IdentityOwnership)]
pub struct OwnedBy(pub Entity);

#[derive(Component)]
#[relationship_target(relationship = OwnedBy)]
pub struct IdentityOwnership(Vec<Entity>);

#[derive(Clone, Copy, Default, Component)]
pub struct Own;

#[derive(Clone, Default, PartialEq, Component)]
pub struct Player {
    pub identity: Identity,
    pub name: String,
    pub color: Color,
}

impl From<&stdb_bindings::Player> for Player {
    fn from(value: &stdb_bindings::Player) -> Self {
        Self {
            identity: value.identity,
            name: value.name.clone(),
            color: PLAYER_COLORS[value.color as usize % PLAYER_COLORS.len()],
        }
    }
}

impl StdbSync for Player {
    type Row = stdb_bindings::Player;
    type Key = Identity;

    fn key(&self) -> Self::Key {
        self.identity
    }
}

/// The connection proof: log the online player count whenever it changes.
pub fn spawn_player_on_insert(
    mut messages: MessageReader<RowInserted<stdb_bindings::Player>>,
    mut commands: Commands,
    mut counter: ResMut<OnlineCounter>,
) {
    for message in messages.read() {
        counter.inc();
        commands.spawn(Player::from(message.row()));
        info!("{} player(s) online", counter.0);
    }
}

/// The connection proof: log the online player count whenever it changes.
pub fn despawn_player_on_delete(
    mut messages: MessageReader<RowDeleted<stdb_bindings::Player>>,
    mut commands: Commands,
    mut counter: ResMut<OnlineCounter>,
    row_entities: RowEntities<Player>,
) {
    for message in messages.read() {
        for entity in row_entities.get_by_row(message.row()) {
            commands.entity(*entity).despawn();
        }
        counter.dec();
        info!("{} player(s) online", counter.0);
    }
}
