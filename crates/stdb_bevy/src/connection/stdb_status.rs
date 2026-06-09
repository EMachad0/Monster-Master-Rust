use bevy::ecs::{resource::Resource, system::Res};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Resource)]
pub enum StdbStatus {
    Connecting,
    Connected,
    Disconnected,
}

pub fn stdb_connected(status: Res<StdbStatus>) -> bool {
    *status == StdbStatus::Connected
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use crate::is_stdb_connected;
    use crate::lifecycle::lifecycle_channel::LifecycleChannel;
    use crate::test_support::{FakeConn, FakeConnectionDriver, test_app};

    #[derive(Resource, Default)]
    struct RunCount(u32);

    fn count_up(mut count: ResMut<RunCount>) {
        count.0 += 1;
    }

    #[test]
    fn stdb_connected_run_condition_gates_systems() {
        let mut app = test_app(FakeConnectionDriver::default());
        app.init_resource::<RunCount>();
        app.add_systems(Update, count_up.run_if(is_stdb_connected));

        let sink = app.world().resource::<LifecycleChannel<FakeConn>>().sink();

        // Disconnected: the gated system never runs.
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<RunCount>().0,
            0,
            "gated system must not run before a connection exists",
        );

        // Connected: it runs. (Two frames: one to drain+insert the resource, one to run.)
        sink.connected(FakeConn).unwrap();
        app.update();
        app.update();
        let while_connected = app.world().resource::<RunCount>().0;
        assert!(while_connected > 0, "gated system must run while connected");

        // Disconnected again: it stops.
        sink.disconnected().unwrap();
        app.update();
        let after_disconnect = app.world().resource::<RunCount>().0;
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<RunCount>().0,
            after_disconnect,
            "gated system must stop running after disconnect",
        );
    }
}
