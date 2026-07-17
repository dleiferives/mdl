//! Dense, typed storage for compiler entities.

use std::error::Error;
use std::fmt;
use std::marker::PhantomData;

/// Implements one typed compiler identifier with a crate-private constructor.
macro_rules! entity_id {
    ($(#[$attribute:meta])* $visibility:vis struct $name:ident;) => {
        $(#[$attribute])*
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        $visibility struct $name(u32);

        impl $crate::entity::EntityId for $name {
            fn from_index(index: u32) -> Self {
                Self(index)
            }

            fn index(self) -> u32 {
                self.0
            }
        }
    };
}

pub(crate) use entity_id;

/// Internal behavior shared by typed entity identifiers.
pub(crate) trait EntityId: Copy {
    fn from_index(index: u32) -> Self;
    fn index(self) -> u32;
}

/// Failure to allocate an ID representable by the entity's `u32` index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EntityLimitError;

impl fmt::Display for EntityLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("entity store exhausted its u32 ID space")
    }
}

impl Error for EntityLimitError {}

/// Append-only dense storage keyed by one specific ID type.
///
/// There is deliberately no removal operation and no untyped indexing. IDs remain
/// stable for the lifetime of the store and are never reused.
#[derive(Clone)]
pub(crate) struct EntityVec<I, T> {
    values: Vec<T>,
    marker: PhantomData<fn(I) -> I>,
}

impl<I, T: PartialEq> PartialEq for EntityVec<I, T> {
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values
    }
}

impl<I, T: Eq> Eq for EntityVec<I, T> {}

impl<I, T> EntityVec<I, T> {
    pub(crate) const fn new() -> Self {
        Self {
            values: Vec::new(),
            marker: PhantomData,
        }
    }

    pub(crate) fn with_first(value: T) -> Self {
        Self {
            values: vec![value],
            marker: PhantomData,
        }
    }

    /// Reuses values moved from another already-constrained dense store in the
    /// same order.
    pub(crate) fn from_constrained_values(values: Vec<T>) -> Self {
        Self {
            values,
            marker: PhantomData,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.values.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl<I: EntityId, T> EntityVec<I, T> {
    pub(crate) fn push(&mut self, value: T) -> Result<I, EntityLimitError> {
        let index = u32::try_from(self.values.len()).map_err(|_| EntityLimitError)?;
        self.values.push(value);
        Ok(I::from_index(index))
    }

    pub(crate) fn get(&self, id: I) -> Option<&T> {
        usize::try_from(id.index())
            .ok()
            .and_then(|index| self.values.get(index))
    }

    pub(crate) fn get_mut(&mut self, id: I) -> Option<&mut T> {
        usize::try_from(id.index())
            .ok()
            .and_then(|index| self.values.get_mut(index))
    }

    pub(crate) fn keys(&self) -> impl ExactSizeIterator<Item = I> + '_ {
        (0..self.values.len()).map(|index| {
            let index = u32::try_from(index)
                .expect("EntityVec length is constrained by successful allocation");
            I::from_index(index)
        })
    }

    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = (I, &T)> + '_ {
        self.keys().zip(&self.values)
    }

    pub(crate) fn into_iter(self) -> impl ExactSizeIterator<Item = (I, T)> {
        let keys = (0..self.values.len()).map(|index| {
            let index = u32::try_from(index)
                .expect("EntityVec length is constrained by successful allocation");
            I::from_index(index)
        });
        keys.zip(self.values)
    }
}

impl<I, T> Default for EntityVec<I, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I: EntityId + fmt::Debug, T: fmt::Debug> fmt::Debug for EntityVec<I, T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_map().entries(self.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::EntityVec;

    entity_id!(
        struct FirstId;
    );
    entity_id!(
        struct SecondId;
    );

    #[test]
    fn allocates_dense_stable_ids() {
        let mut values = EntityVec::<FirstId, _>::new();
        let first = values.push("first").unwrap();
        let second = values.push("second").unwrap();

        assert_eq!(first, FirstId(0));
        assert_eq!(second, FirstId(1));
        assert_eq!(values.get(first), Some(&"first"));
        assert_eq!(values.get(second), Some(&"second"));
        assert_eq!(values.keys().collect::<Vec<_>>(), vec![first, second]);
    }

    #[test]
    fn invalid_lookup_is_fallible() {
        let values = EntityVec::<FirstId, i32>::new();
        assert_eq!(values.get(FirstId(9)), None);
    }

    #[test]
    fn mutable_lookup_does_not_change_identity() {
        let mut values = EntityVec::<FirstId, _>::new();
        let id = values.push(10).unwrap();
        *values.get_mut(id).unwrap() = 20;

        assert_eq!(values.get(id), Some(&20));
        assert_eq!(values.keys().collect::<Vec<_>>(), vec![id]);
    }

    #[test]
    fn debug_output_includes_typed_keys_in_order() {
        let mut values = EntityVec::<FirstId, _>::new();
        values.push("a").unwrap();
        values.push("b").unwrap();

        assert_eq!(
            format!("{values:?}"),
            "{FirstId(0): \"a\", FirstId(1): \"b\"}"
        );
    }

    #[test]
    fn id_types_are_distinct() {
        let mut first = EntityVec::<FirstId, _>::new();
        let mut second = EntityVec::<SecondId, _>::new();

        let first_id = first.push(1).unwrap();
        let second_id = second.push(2).unwrap();

        assert_eq!(first.get(first_id), Some(&1));
        assert_eq!(second.get(second_id), Some(&2));
    }
}
