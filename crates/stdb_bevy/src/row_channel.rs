use bevy::ecs::{
    message::Message,
    resource::Resource,
    system::{Commands, Res},
};

#[derive(Message)]
pub struct RowInserted<T>(pub T);

impl<T> RowInserted<T> {
    pub fn new(row: T) -> Self {
        Self(row)
    }

    pub fn row(&self) -> &T {
        &self.0
    }
}

#[derive(Message)]
pub struct RowUpdated<T> {
    pub old: T,
    pub new: T,
}

impl<T> RowUpdated<T> {
    pub fn new(old: T, new: T) -> Self {
        Self { old, new }
    }
}

pub(crate) enum RowEvent<T> {
    Insert(T),
    Update { old: T, new: T },
}

pub(crate) struct RowSink<T> {
    pub sender: crossbeam_channel::Sender<RowEvent<T>>,
}

impl<T> RowSink<T> {
    pub fn insert(&self, row: T) -> Result<(), crossbeam_channel::SendError<RowEvent<T>>> {
        self.sender.send(RowEvent::Insert(row))
    }

    pub fn update(&self, old: T, new: T) -> Result<(), crossbeam_channel::SendError<RowEvent<T>>> {
        self.sender.send(RowEvent::Update { old, new })
    }
}

#[derive(Resource)]
pub(crate) struct RowChannel<T> {
    sender: crossbeam_channel::Sender<RowEvent<T>>,
    receiver: crossbeam_channel::Receiver<RowEvent<T>>,
}

impl<T> RowChannel<T> {
    pub fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self { sender, receiver }
    }

    pub fn sink(&self) -> RowSink<T> {
        RowSink {
            sender: self.sender.clone(),
        }
    }
}

impl<T> Default for RowChannel<T> {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn drain_row_sink<T: 'static + Send + Sync>(
    row_channel: Res<RowChannel<T>>,
    mut commands: Commands,
) {
    while let Ok(stdb_event) = row_channel.receiver.try_recv() {
        match stdb_event {
            RowEvent::Insert(row) => {
                commands.write_message(RowInserted(row));
            }
            RowEvent::Update { old, new } => {
                commands.write_message(RowUpdated::new(old, new));
            }
        }
    }
}
