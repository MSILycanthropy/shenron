use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

/// A type-keyed bag of values scoped to a single connection.
///
/// Holds one value per concrete type. Auth handlers and middleware stash typed
/// data here; the handler reads it back with [`get`](Self::get). Values must be
/// `Send + Sync + 'static` since they cross into the spawned session task.
///
/// Use a newtype (`struct RequestId(String)`) rather than a bare `String` so
/// distinct values don't collide on the same `TypeId`.
#[derive(Default)]
pub struct Extensions(HashMap<TypeId, Box<dyn Any + Send + Sync>>);

impl Extensions {
    /// Store a value, replacing any existing value of the same type.
    pub fn insert<T: Any + Send + Sync>(&mut self, value: T) {
        self.0.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Borrow the stored value of type `T`, if present.
    #[must_use]
    pub fn get<T: Any>(&self) -> Option<&T> {
        self.0.get(&TypeId::of::<T>())?.downcast_ref::<T>()
    }

    /// Fold `other` into `self`; on type collisions `other` wins.
    pub(crate) fn merge(&mut self, other: Self) {
        self.0.extend(other.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Account(u32);

    #[derive(Debug, PartialEq)]
    struct RequestId(String);

    #[test]
    fn insert_and_get() {
        let mut ext = Extensions::default();
        ext.insert(Account(7));

        assert_eq!(ext.get::<Account>(), Some(&Account(7)));
    }

    #[test]
    fn overwrite_same_type() {
        let mut ext = Extensions::default();
        ext.insert(Account(1));
        ext.insert(Account(2));

        assert_eq!(ext.get::<Account>(), Some(&Account(2)));
    }

    #[test]
    fn missing_is_none() {
        let ext = Extensions::default();

        assert_eq!(ext.get::<Account>(), None);
    }

    #[test]
    fn newtypes_are_distinct() {
        let mut ext = Extensions::default();
        ext.insert(Account(9));
        ext.insert(RequestId("abc".into()));

        assert_eq!(ext.get::<Account>(), Some(&Account(9)));
        assert_eq!(ext.get::<RequestId>(), Some(&RequestId("abc".into())));
    }

    #[test]
    fn merge_other_wins() {
        let mut base = Extensions::default();
        base.insert(Account(1));

        let mut other = Extensions::default();
        other.insert(Account(2));
        other.insert(RequestId("r".into()));

        base.merge(other);

        assert_eq!(base.get::<Account>(), Some(&Account(2)));
        assert_eq!(base.get::<RequestId>(), Some(&RequestId("r".into())));
    }
}
