use bevy::ecs::{observer::On, schedule::IntoScheduleConfigs as _, system::Res};

use crate::{
    RowDeleted, RowForwarder, RowInserted, RowUpdated, StdbConn, StdbConnected, StdbConnection,
    StdbRow, StdbSystemSet,
    row::row_channel::{RowChannel, drain_row_sink},
};

pub struct TableRegistration {
    install: Box<dyn Fn(&mut bevy::app::App) + Send + Sync + 'static>,
}

impl TableRegistration {
    pub fn new<C, R>(messages_callback: fn(&StdbConnection<C>, RowForwarder<R>)) -> Self
    where
        C: StdbConn,
        R: StdbRow,
    {
        Self {
            install: Box::new(move |app| {
                add_stdb_table(app, messages_callback);
            }),
        }
    }

    pub fn register(&self, app: &mut bevy::app::App) {
        (self.install)(app)
    }
}

pub(crate) fn add_stdb_table<C, R>(
    app: &mut bevy::app::App,
    messages_callback: fn(&StdbConnection<C>, RowForwarder<R>),
) where
    C: StdbConn,
    R: StdbRow,
{
    let row_channel = RowChannel::new();
    let sink = row_channel.sink();

    app.insert_resource(row_channel);
    app.add_message::<RowInserted<R>>();
    app.add_message::<RowUpdated<R>>();
    app.add_message::<RowDeleted<R>>();

    app.add_observer(
        move |_: On<StdbConnected>, connection: Res<StdbConnection<C>>| {
            let fwd = RowForwarder::new(sink.clone());
            (messages_callback)(&connection, fwd);
        },
    );

    app.add_systems(
        bevy::app::Update,
        drain_row_sink::<R>.in_set(StdbSystemSet::RowMessagesPush),
    );
}
