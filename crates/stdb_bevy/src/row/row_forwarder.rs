use spacetimedb_sdk::table::{WithDelete, WithInsert, WithUpdate};

use crate::{
    RowMessagesMask,
    row::row_channel::{RowSink, StdbRow},
};

pub struct RowForwarder<R: StdbRow> {
    sink: RowSink<R>,
    filter: RowMessagesMask,
}

impl<R: StdbRow> RowForwarder<R> {
    pub fn new(sink: RowSink<R>) -> Self {
        Self {
            sink,
            filter: RowMessagesMask::default(),
        }
    }

    pub fn with_filter(mut self, filter: impl Into<RowMessagesMask>) -> Self {
        self.filter = filter.into();
        self
    }

    pub fn forward<T>(mut self, table: &T) -> Self
    where
        T: WithInsert<Row = R> + WithDelete<Row = R> + WithUpdate<Row = R>,
    {
        let RowMessagesMask {
            insert,
            update,
            delete,
        } = self.filter;
        if insert {
            self = self.inserts(table);
        }
        if update {
            self = self.updates(table);
        }
        if delete {
            self = self.deletes(table);
        }
        self
    }

    pub fn forward_keyless<T>(mut self, table: &T) -> Self
    where
        T: WithInsert<Row = R> + WithDelete<Row = R>,
    {
        let RowMessagesMask { insert, delete, .. } = self.filter;
        if insert {
            self = self.inserts(table);
        }
        if delete {
            self = self.deletes(table);
        }
        self
    }

    fn inserts<T>(self, table: &T) -> Self
    where
        T: WithInsert<Row = R>,
    {
        let sink = self.sink.clone();
        let _insert_handle = table.on_insert(move |_ctx, row| sink.insert(row.clone()));
        self
    }

    fn deletes<T>(self, table: &T) -> Self
    where
        T: WithDelete<Row = R>,
    {
        let sink = self.sink.clone();
        let _delete_handle = table.on_delete(move |_ctx, row| sink.delete(row.clone()));
        self
    }

    fn updates<T>(self, table: &T) -> Self
    where
        T: WithUpdate<Row = R>,
    {
        let sink = self.sink.clone();
        let _delete_handle =
            table.on_update(move |_ctx, old, new| sink.update(old.clone(), new.clone()));
        self
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use super::RowForwarder;
    use crate::row::row_channel::{RowChannel, drain_row_sink};
    use crate::test_support::FakeTable;
    use crate::{RowDeleted, RowInserted, RowMessagesMask, RowUpdated};

    #[derive(Clone, PartialEq, Debug)]
    struct Widget {
        id: u32,
    }

    #[derive(Resource, Default)]
    struct Inserts(Vec<Widget>);
    #[derive(Resource, Default)]
    struct Updates(Vec<(Widget, Widget)>);
    #[derive(Resource, Default)]
    struct Deletes(Vec<Widget>);

    fn capture_inserts(mut reader: MessageReader<RowInserted<Widget>>, mut out: ResMut<Inserts>) {
        for msg in reader.read() {
            out.0.push(msg.0.clone());
        }
    }
    fn capture_updates(mut reader: MessageReader<RowUpdated<Widget>>, mut out: ResMut<Updates>) {
        for msg in reader.read() {
            out.0.push((msg.old.clone(), msg.new.clone()));
        }
    }
    fn capture_deletes(mut reader: MessageReader<RowDeleted<Widget>>, mut out: ResMut<Deletes>) {
        for msg in reader.read() {
            out.0.push(msg.0.clone());
        }
    }

    /// An app with the `Widget` channel + messages + drain + capture systems — no connection; the
    /// forwarder is fed a fake table directly.
    fn widget_app() -> App {
        let mut app = App::new();
        app.insert_resource(RowChannel::<Widget>::new());
        app.add_message::<RowInserted<Widget>>();
        app.add_message::<RowUpdated<Widget>>();
        app.add_message::<RowDeleted<Widget>>();
        app.add_systems(Update, drain_row_sink::<Widget>("widget"));
        app.init_resource::<Inserts>();
        app.init_resource::<Updates>();
        app.init_resource::<Deletes>();
        app.add_systems(Update, (capture_inserts, capture_updates, capture_deletes));
        app
    }

    #[test]
    fn forward_emits_insert_update_delete() {
        let mut app = widget_app();
        let sink = app.world().resource::<RowChannel<Widget>>().sink();

        RowForwarder::new(sink).forward(&FakeTable {
            rows: vec![],
            inserts: vec![Widget { id: 1 }],
            updates: vec![(Widget { id: 1 }, Widget { id: 2 })],
            deletes: vec![Widget { id: 3 }],
        });

        app.update();
        app.update();

        assert_eq!(app.world().resource::<Inserts>().0, vec![Widget { id: 1 }]);
        assert_eq!(
            app.world().resource::<Updates>().0,
            vec![(Widget { id: 1 }, Widget { id: 2 })]
        );
        assert_eq!(app.world().resource::<Deletes>().0, vec![Widget { id: 3 }]);
    }

    #[test]
    fn forward_with_a_partial_filter_wires_only_those_callbacks() {
        let mut app = widget_app();
        let sink = app.world().resource::<RowChannel<Widget>>().sink();

        // The fake has an update queued, but the filter selects only inserts + deletes.
        let fake = FakeTable {
            rows: vec![],
            inserts: vec![Widget { id: 1 }],
            updates: vec![(Widget { id: 1 }, Widget { id: 2 })],
            deletes: vec![Widget { id: 3 }],
        };
        RowForwarder::new(sink)
            .with_filter(RowMessagesMask {
                insert: true,
                update: false,
                delete: true,
            })
            .forward(&fake);

        app.update();
        app.update();

        assert_eq!(app.world().resource::<Inserts>().0, vec![Widget { id: 1 }]);
        assert_eq!(app.world().resource::<Deletes>().0, vec![Widget { id: 3 }]);
        assert!(
            app.world().resource::<Updates>().0.is_empty(),
            "update is deselected, so forward must not wire on_update — no RowUpdated may surface",
        );
    }

    #[test]
    fn forward_with_an_empty_filter_wires_nothing() {
        let mut app = widget_app();
        let sink = app.world().resource::<RowChannel<Widget>>().sink();

        let fake = FakeTable {
            rows: vec![],
            inserts: vec![Widget { id: 1 }],
            updates: vec![(Widget { id: 1 }, Widget { id: 2 })],
            deletes: vec![Widget { id: 3 }],
        };
        RowForwarder::new(sink)
            .with_filter(RowMessagesMask::NONE)
            .forward(&fake);

        app.update();
        app.update();

        assert!(
            app.world().resource::<Inserts>().0.is_empty()
                && app.world().resource::<Updates>().0.is_empty()
                && app.world().resource::<Deletes>().0.is_empty(),
            "an empty filter selects no events, so forward wires no callbacks and nothing surfaces",
        );
    }
}
