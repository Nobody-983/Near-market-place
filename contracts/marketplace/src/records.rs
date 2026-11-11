//! Offline point-access store. No production iterator or aggregate serialization:
//! a future persistent adapter must preserve these bounded access patterns.
use std::{collections::BTreeMap, ops::Index};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Records<K: Ord, V>(BTreeMap<K, V>);

impl<K: Ord, V> Records<K, V> {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }
    pub fn contains_key(&self, key: &K) -> bool {
        self.0.contains_key(key)
    }
    pub fn get(&self, key: &K) -> Option<&V> {
        self.0.get(key)
    }
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.0.get_mut(key)
    }
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.0.insert(key, value)
    }
    #[cfg(test)]
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.0.iter()
    }
}

impl<K: Ord, V> Index<&K> for Records<K, V> {
    type Output = V;
    fn index(&self, key: &K) -> &V {
        &self.0[key]
    }
}
