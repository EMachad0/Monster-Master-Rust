//! `StdbToken` — a shared, game-owned handle to the connection's auth token (an OIDC JWT that
//! encodes the player's `Identity`).
//!
//! It is a `Clone` handle over a single `Arc<Mutex<Option<String>>>`. The Game holds it as a
//! `Resource` (seed it from a save before connect, read it after connect to persist, `clear()` to
//! log out) and the `SdkConnectionDriver` holds a clone of the **same** handle (`with_token(get())`
//! before build; `set(Some(issued))` from the SDK's `on_connect`). The shared `Arc` is the
//! bidirectional channel between Game, driver, and the SDK callback — so all four operations
//! (seed → build → server-issue → persist) read and write one slot. See ADR 0002.
//!
//! Because it uses interior mutability (`&self`), the Game can hold it as `Res<StdbToken>` (shared)
//! and still `set`/`clear` it, and the driver can `set` it from a `&self` callback.

use std::sync::{Arc, Mutex};

#[derive(Debug, Default, Clone)]
pub struct StdbToken(Arc<Mutex<Option<String>>>);

impl StdbToken {
    pub fn new(token: impl Into<String>) -> Self {
        Self(Arc::new(Mutex::new(Some(token.into()))))
    }

    pub fn get(&self) -> Option<String> {
        match self.0.lock() {
            Ok(token) => token.clone(),
            Err(err) => {
                bevy::log::error!("could not aquire token lock {}", err);
                None
            }
        }
    }

    pub fn set(&self, token: impl Into<String>) {
        match self.0.lock() {
            Ok(mut inner) => *inner = Some(token.into()),
            Err(err) => {
                bevy::log::error!("could not aquire token lock {}", err);
            }
        };
    }

    pub fn clear(&self) {
        match self.0.lock() {
            Ok(mut inner) => *inner = None,
            Err(err) => {
                bevy::log::error!("could not aquire token lock {}", err);
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::StdbToken;

    #[test]
    fn get_is_none_by_default() {
        let token = StdbToken::default();
        assert_eq!(token.get(), None, "a fresh token is anonymous");
    }

    #[test]
    fn set_then_get_returns_it() {
        let token = StdbToken::default();
        token.set("jwt-abc".to_string());
        assert_eq!(token.get(), Some("jwt-abc".to_string()));
    }

    #[test]
    fn set_latest_wins() {
        let token = StdbToken::default();
        token.set("old".to_string());
        token.set("new".to_string());
        assert_eq!(
            token.get(),
            Some("new".to_string()),
            "the server-issued token on (re)connect replaces the seed",
        );
    }

    #[test]
    fn clear_drops_the_token() {
        let token = StdbToken::default();
        token.set("jwt-abc".to_string());
        token.clear();
        assert_eq!(token.get(), None, "clear() logs out / returns to anonymous");
    }

    /// The property the whole design rests on: a clone is the *same* slot, so a write through any
    /// handle is visible through every other. This is what lets the Game (Res), the driver (field),
    /// and the SDK `on_connect` callback all share one token.
    #[test]
    fn clones_share_state() {
        let game_handle = StdbToken::default();
        let driver_handle = game_handle.clone();

        // Game seeds from a save → the driver's handle sees it (used by `with_token` on build).
        game_handle.set("from-save".to_string());
        assert_eq!(driver_handle.get(), Some("from-save".to_string()));

        // Driver writes the server-issued token from on_connect → the Game's handle sees it (to persist).
        driver_handle.set("server-issued".to_string());
        assert_eq!(game_handle.get(), Some("server-issued".to_string()));

        // Logout through one handle clears it for all.
        driver_handle.clear();
        assert_eq!(game_handle.get(), None);
    }
}
