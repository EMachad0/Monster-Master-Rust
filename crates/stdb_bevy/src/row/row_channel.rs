use bevy::ecs::{
    resource::Resource,
    system::{Commands, Res},
};

use crate::row::row_messages::{RowDeleted, RowInserted, RowUpdated};

pub trait StdbRow: 'static + Send + Sync + Clone + PartialEq {}

impl<R> StdbRow for R where R: 'static + Send + Sync + Clone + PartialEq {}

pub enum RowEvent<R> {
    Insert(R),
    Update { old: R, new: R },
    Delete(R),
}

#[derive(Clone)]
pub struct RowSink<R: StdbRow> {
    pub sender: crossbeam_channel::Sender<RowEvent<R>>,
}

impl<R: StdbRow> RowSink<R> {
    pub fn insert(&self, row: R) {
        self.sender
            .send(RowEvent::Insert(row))
            .unwrap_or_else(|err| bevy::log::error!(%err, "row sink insert send failed"));
    }

    pub fn update(&self, old: R, new: R) {
        self.sender
            .send(RowEvent::Update { old, new })
            .unwrap_or_else(|err| bevy::log::error!(%err, "row sink update send failed"));
    }

    pub fn delete(&self, row: R) {
        self.sender
            .send(RowEvent::Delete(row))
            .unwrap_or_else(|err| bevy::log::error!(%err, "row sink delete send failed"));
    }
}

#[derive(Resource)]
pub(crate) struct RowChannel<R: StdbRow> {
    sender: crossbeam_channel::Sender<RowEvent<R>>,
    receiver: crossbeam_channel::Receiver<RowEvent<R>>,
}

impl<R: StdbRow> RowChannel<R> {
    pub fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self { sender, receiver }
    }

    pub fn sink(&self) -> RowSink<R> {
        RowSink {
            sender: self.sender.clone(),
        }
    }
}

impl<R: StdbRow> Default for RowChannel<R> {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn drain_row_sink<R: StdbRow>(
    label: &'static str,
) -> impl FnMut(Res<RowChannel<R>>, Commands) {
    move |row_channel: Res<RowChannel<R>>, mut commands: Commands| {
        while let Ok(stdb_event) = row_channel.receiver.try_recv() {
            match stdb_event {
                RowEvent::Insert(row) => {
                    bevy::log::trace!(table = label, "row inserted");
                    commands.write_message(RowInserted(row));
                }
                RowEvent::Update { old, new } => {
                    bevy::log::trace!(table = label, "row updated");
                    commands.write_message(RowUpdated::new(old, new));
                }
                RowEvent::Delete(row) => {
                    bevy::log::trace!(table = label, "row deleted");
                    commands.write_message(RowDeleted(row));
                }
            }
        }
    }
}

pub(crate) fn clear_row_sink<R: StdbRow>(row_channel: Res<RowChannel<R>>) {
    while row_channel.receiver.try_recv().is_ok() {}
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use super::{RowChannel, drain_row_sink};
    use crate::{RowDeleted, RowInserted, RowUpdated};

    /// A stand-in row type. A real Module row is `Clone + Send + Sync + 'static`.
    #[derive(Clone, PartialEq, Debug)]
    struct Foo {
        id: u32,
    }

    /// An app with the `Foo` channel + messages + drain installed, but no connection — the drain is
    /// what's under test, fed directly through the channel sink.
    fn foo_app() -> App {
        let mut app = App::new();
        app.insert_resource(RowChannel::<Foo>::new());
        app.add_message::<RowInserted<Foo>>();
        app.add_message::<RowUpdated<Foo>>();
        app.add_message::<RowDeleted<Foo>>();
        app.add_systems(Update, drain_row_sink::<Foo>("foo"));
        app
    }

    #[derive(Resource, Default)]
    struct CapturedInserts(Vec<Foo>);

    fn capture_inserts(
        mut reader: MessageReader<RowInserted<Foo>>,
        mut captured: ResMut<CapturedInserts>,
    ) {
        for msg in reader.read() {
            captured.0.push(msg.0.clone());
        }
    }

    #[derive(Resource, Default)]
    struct CapturedUpdates(Vec<(Foo, Foo)>);

    fn capture_updates(
        mut reader: MessageReader<RowUpdated<Foo>>,
        mut captured: ResMut<CapturedUpdates>,
    ) {
        for msg in reader.read() {
            captured.0.push((msg.old.clone(), msg.new.clone()));
        }
    }

    #[derive(Resource, Default)]
    struct CapturedDeletes(Vec<Foo>);

    fn capture_deletes(
        mut reader: MessageReader<RowDeleted<Foo>>,
        mut captured: ResMut<CapturedDeletes>,
    ) {
        for msg in reader.read() {
            captured.0.push(msg.0.clone());
        }
    }

    #[test]
    fn insert_event_becomes_row_inserted_message() {
        let mut app = foo_app();
        app.init_resource::<CapturedInserts>();
        app.add_systems(Update, capture_inserts);

        // Push a row insert through the same seam the SDK on_insert callback uses in production.
        let sink = app.world().resource::<RowChannel<Foo>>().sink();
        sink.insert(Foo { id: 7 });

        // One frame to drain the channel into a message, one for the reader to observe it.
        app.update();
        app.update();

        let captured = &app.world().resource::<CapturedInserts>().0;
        assert_eq!(
            captured.len(),
            1,
            "one queued insert should produce exactly one RowInserted message",
        );
        assert_eq!(captured[0], Foo { id: 7 });
    }

    #[test]
    fn update_event_becomes_row_updated_message() {
        let mut app = foo_app();
        app.init_resource::<CapturedUpdates>();
        app.add_systems(Update, capture_updates);

        let sink = app.world().resource::<RowChannel<Foo>>().sink();
        sink.update(Foo { id: 1 }, Foo { id: 2 });

        app.update();
        app.update();

        let captured = &app.world().resource::<CapturedUpdates>().0;
        assert_eq!(
            captured.len(),
            1,
            "one queued update should produce exactly one RowUpdated message",
        );
        assert_eq!(captured[0], (Foo { id: 1 }, Foo { id: 2 }));
    }

    #[test]
    fn delete_event_becomes_row_deleted_message() {
        let mut app = foo_app();
        app.init_resource::<CapturedDeletes>();
        app.add_systems(Update, capture_deletes);

        let sink = app.world().resource::<RowChannel<Foo>>().sink();
        sink.delete(Foo { id: 9 });

        app.update();
        app.update();

        let captured = &app.world().resource::<CapturedDeletes>().0;
        assert_eq!(
            captured.len(),
            1,
            "one queued delete should produce exactly one RowDeleted message",
        );
        assert_eq!(captured[0], Foo { id: 9 });
    }

    #[test]
    fn bulk_inserts_preserve_count_and_order() {
        let mut app = foo_app();
        app.init_resource::<CapturedInserts>();
        app.add_systems(Update, capture_inserts);

        // Queue several inserts before a single update — the initial-subscription dump.
        let sink = app.world().resource::<RowChannel<Foo>>().sink();
        sink.insert(Foo { id: 1 });
        sink.insert(Foo { id: 2 });
        sink.insert(Foo { id: 3 });

        app.update();
        app.update();

        let captured = &app.world().resource::<CapturedInserts>().0;
        assert_eq!(
            *captured,
            vec![Foo { id: 1 }, Foo { id: 2 }, Foo { id: 3 }],
            "all queued inserts should surface as messages, in send order",
        );
    }
}
