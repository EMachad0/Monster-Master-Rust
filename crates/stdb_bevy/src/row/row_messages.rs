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

#[derive(Clone, Copy)]
pub struct RowMessagesMask {
    pub insert: bool,
    pub update: bool,
    pub delete: bool,
}

impl RowMessagesMask {
    pub const ALL: Self = Self {
        insert: true,
        update: true,
        delete: true,
    };

    pub const NONE: Self = Self {
        insert: false,
        update: false,
        delete: false,
    };
}

impl Default for RowMessagesMask {
    fn default() -> Self {
        Self::ALL
    }
}

#[derive(Clone, Copy)]
pub struct KeylessMessagesMask {
    pub insert: bool,
    pub delete: bool,
}

impl KeylessMessagesMask {
    pub const INSERT_DELETE: Self = Self {
        insert: true,
        delete: true,
    };

    pub const NONE: Self = Self {
        insert: false,
        delete: false,
    };
}

impl Default for KeylessMessagesMask {
    fn default() -> Self {
        Self::INSERT_DELETE
    }
}

impl From<KeylessMessagesMask> for RowMessagesMask {
    fn from(value: KeylessMessagesMask) -> Self {
        Self {
            insert: value.insert,
            update: false,
            delete: value.delete,
        }
    }
}
