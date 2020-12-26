use std::cell::UnsafeCell;
use std::collections::HashMap;

/// This fast cell map performs faster than a regular RefCell<HashMap<TKey, Rc<TValue>>
/// because it avoids doing runtime checks on borrowing and mutation. This collection
/// remains safe to the user because the value in the hashmap is a reference.
/// Additionally, the underlying collection is never exposed to the user.
pub struct FastCellMap<'a, TKey, TValue> {
    value: UnsafeCell<HashMap<TKey, &'a TValue>>,
}

impl<'a, TKey, TValue> FastCellMap<'a, TKey, TValue> where TKey : std::cmp::Eq + std::hash::Hash + Clone {
    pub fn new() -> FastCellMap<'a, TKey, TValue> {
        FastCellMap {
            value: UnsafeCell::new(HashMap::new()),
        }
    }

    pub fn replace_map(&mut self, new_map: HashMap<TKey, &'a TValue>) {
        self.value = UnsafeCell::new(new_map);
    }

    pub fn contains_key(&self, key: &TKey) -> bool {
        unsafe {
            (*self.value.get()).contains_key(key)
        }
    }

    pub fn insert(&self, key: TKey, value: &'a TValue) {
        unsafe {
            (*self.value.get()).insert(key, value);
        }
    }

    pub fn remove(&self, key: &TKey) -> Option<&'a TValue> {
        unsafe {
            (*self.value.get()).remove(key)
        }
    }

    pub fn clone_map(&self) -> HashMap<TKey, &'a TValue> {
        unsafe {
            (*self.value.get()).clone()
        }
    }

    /// Used in the printer to panic if any item exists in the collection
    /// at the end of printing.
    #[cfg(debug_assertions)]
    pub fn get_any_item(&self) -> Option<&'a TValue> {
        unsafe {
            (*self.value.get()).iter().map(|(_, b)| b.clone()).next()
        }
    }
}
