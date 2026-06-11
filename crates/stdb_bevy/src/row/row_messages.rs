use bevy::ecs::message::Message;

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

#[derive(Message)]
pub struct RowDeleted<T>(pub T);

impl<T> RowDeleted<T> {
    pub fn new(row: T) -> Self {
        Self(row)
    }

    pub fn row(&self) -> &T {
        &self.0
    }
}
