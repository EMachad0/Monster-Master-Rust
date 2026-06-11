use bevy::ecs::{observer::On, resource::Resource, system::ResMut};

use crate::connection::connection_events::{StdbConnect, StdbDisconnect};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource)]
pub(crate) enum StdbIntent {
    Connected,
    Disconnected,
}

pub(crate) fn update_intent_on_stdbconnect(_: On<StdbConnect>, mut intent: ResMut<StdbIntent>) {
    *intent = StdbIntent::Connected;
}

pub(crate) fn update_intent_on_stdbdisconnect(
    _: On<StdbDisconnect>,
    mut intent: ResMut<StdbIntent>,
) {
    *intent = StdbIntent::Disconnected;
}

#[cfg(test)]
mod tests {
    use super::StdbIntent;
    use crate::test_support::{FakeConnectionDriver, test_app};
    use crate::{StdbConnect, StdbDisconnect, StdbStatus};

    #[test]
    fn engine_starts_disconnected_with_disconnected_intent() {
        let app = test_app(FakeConnectionDriver::default());

        assert_eq!(
            *app.world().resource::<StdbStatus>(),
            StdbStatus::Disconnected
        );
        assert_eq!(
            *app.world().resource::<StdbIntent>(),
            StdbIntent::Disconnected
        );
    }

    #[test]
    fn stdb_connect_request_sets_intent_connected() {
        let mut app = test_app(FakeConnectionDriver::default());

        app.world_mut().trigger(StdbConnect);
        app.update();

        assert_eq!(*app.world().resource::<StdbIntent>(), StdbIntent::Connected);
    }

    #[test]
    fn stdb_disconnect_request_sets_intent_disconnected() {
        let mut app = test_app(FakeConnectionDriver::default());

        // Reach Connected intent first, so the flip back is observable.
        app.world_mut().trigger(StdbConnect);
        app.update();
        assert_eq!(*app.world().resource::<StdbIntent>(), StdbIntent::Connected);

        app.world_mut().trigger(StdbDisconnect);
        app.update();

        assert_eq!(
            *app.world().resource::<StdbIntent>(),
            StdbIntent::Disconnected
        );
    }
}
