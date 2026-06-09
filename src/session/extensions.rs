use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

/// A type-keyed bag of values attached to a connection and snapshotted into
/// each session.
///
/// Holds one value per concrete type. Auth handlers and middleware stash typed
/// data here; the handler reads it back with [`get`](Self::get). Values must be
/// `Clone + Send + Sync + 'static`: every session on a connection gets its own
/// clone, so mutations never leak across sessions. Wrap non-`Clone` data (or
/// data sessions should genuinely share) in an `Arc`.
///
/// Use a newtype (`struct RequestId(String)`) rather than a bare `String` so
/// distinct values don't collide on the same `TypeId`.
#[derive(Default, Clone)]
pub struct Extensions(HashMap<TypeId, Box<dyn CloneAny>>);

/// Object-safe `Any + Clone`. `DynClone` makes `Box<dyn CloneAny>: Clone`;
/// `as_any` recovers `&dyn Any` for downcasting, which a subtrait of `Any`
/// can't do directly.
trait CloneAny: dyn_clone::DynClone + Send + Sync + 'static {
    fn as_any(&self) -> &dyn Any;
}

dyn_clone::clone_trait_object!(CloneAny);

impl<T: Any + Clone + Send + Sync> CloneAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Extensions {
    /// Store a value, replacing any existing value of the same type.
    pub fn insert<T: Any + Clone + Send + Sync>(&mut self, value: T) {
        self.0.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Borrow the stored value of type `T`, if present.
    #[must_use]
    pub fn get<T: Any>(&self) -> Option<&T> {
        // Deref past the Box: the blanket impl covers `Box<dyn CloneAny>`
        // itself, so `.as_any()` on the box would downcast to the box type.
        let boxed = self.0.get(&TypeId::of::<T>())?;

        (**boxed).as_any().downcast_ref::<T>()
    }

    /// Fold `other` into `self`; on type collisions `other` wins.
    pub(crate) fn merge(&mut self, other: Self) {
        self.0.extend(other.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Clone)]
    struct Account(u32);

    #[derive(Debug, PartialEq, Clone)]
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
    fn clones_are_independent() {
        let mut original = Extensions::default();
        original.insert(Account(1));

        let mut snapshot = original.clone();
        snapshot.insert(Account(2));

        assert_eq!(original.get::<Account>(), Some(&Account(1)));
        assert_eq!(snapshot.get::<Account>(), Some(&Account(2)));
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
