use std::{
    cell::RefCell,
    collections::{btree_map::IntoValues, BTreeMap},
};

use visualize::visualize_to_vec;

/// Top-level storage abstraction.
/// Should be able to hold storage connection and to start transaction when
/// needed. All query operations will be exposed using [StorageContext].
pub trait Storage<'db> {
    /// Storage transaction type
    type Transaction;

    /// Storage context type
    type StorageContext: StorageContext<'db, Error = Self::Error>;

    /// Storage context type for transactional data
    type TransactionalStorageContext: StorageContext<'db, Error = Self::Error>;

    /// Storage context type for mutli-tree batch operations
    type BatchStorageContext;

    /// Storage context type for multi-tree batch operations inside transaction
    type BatchTransactionalStorageContext;

    /// Error type
    type Error: std::error::Error + Send + Sync + 'static;

    /// Starts a new transaction
    fn start_transaction(&'db self) -> Self::Transaction;

    /// Consumes and commits a transaction
    fn commit_transaction(&self, transaction: Self::Transaction) -> Result<(), Self::Error>;

    /// Rollback a transaction
    fn rollback_transaction(&self, transaction: &Self::Transaction) -> Result<(), Self::Error>;

    /// Consumes and applies multi-context batch.
    fn commit_multi_context_batch(
        &self,
        batch: StorageBatch,
        transaction: Option<&'db Self::Transaction>,
    ) -> Result<(), Self::Error>;

    /// Forces data to be written
    fn flush(&self) -> Result<(), Self::Error>;

    /// Make storage context for a subtree with path
    fn get_storage_context<'p, P>(&'db self, path: P) -> Self::StorageContext
    where
        P: IntoIterator<Item = &'p [u8]>;

    /// Make storage context for a subtree on transactional data
    fn get_transactional_storage_context<'p, P>(
        &'db self,
        path: P,
        transaction: &'db Self::Transaction,
    ) -> Self::TransactionalStorageContext
    where
        P: IntoIterator<Item = &'p [u8]>;

    /// Make batch storage context for a subtree with path
    fn get_batch_storage_context<'p, P>(
        &'db self,
        path: P,
        batch: &'db StorageBatch,
    ) -> Self::BatchStorageContext
    where
        P: IntoIterator<Item = &'p [u8]>;

    /// Make batch storage context for a subtree on transactional data
    fn get_batch_transactional_storage_context<'p, P>(
        &'db self,
        path: P,
        batch: &'db StorageBatch,
        transaction: &'db Self::Transaction,
    ) -> Self::BatchTransactionalStorageContext
    where
        P: IntoIterator<Item = &'p [u8]>;
}

/// Storage context.
/// Provides operations expected from a database abstracting details such as
/// whether it is a transaction or not.
pub trait StorageContext<'db> {
    /// Storage error type
    type Error: std::error::Error + Send + Sync + 'static;

    /// Storage batch type
    type Batch: Batch;

    /// Storage raw iterator type (to iterate over storage without supplying a
    /// key)
    type RawIterator: RawIterator;

    /// Put `value` into data storage with `key`
    fn put<K: AsRef<[u8]>>(&self, key: K, value: &[u8]) -> Result<(), Self::Error>;

    /// Put `value` into auxiliary data storage with `key`
    fn put_aux<K: AsRef<[u8]>>(&self, key: K, value: &[u8]) -> Result<(), Self::Error>;

    /// Put `value` into trees roots storage with `key`
    fn put_root<K: AsRef<[u8]>>(&self, key: K, value: &[u8]) -> Result<(), Self::Error>;

    /// Put `value` into GroveDB metadata storage with `key`
    fn put_meta<K: AsRef<[u8]>>(&self, key: K, value: &[u8]) -> Result<(), Self::Error>;

    /// Delete entry with `key` from data storage
    fn delete<K: AsRef<[u8]>>(&self, key: K) -> Result<(), Self::Error>;

    /// Delete entry with `key` from auxiliary data storage
    fn delete_aux<K: AsRef<[u8]>>(&self, key: K) -> Result<(), Self::Error>;

    /// Delete entry with `key` from trees roots storage
    fn delete_root<K: AsRef<[u8]>>(&self, key: K) -> Result<(), Self::Error>;

    /// Delete entry with `key` from GroveDB metadata storage
    fn delete_meta<K: AsRef<[u8]>>(&self, key: K) -> Result<(), Self::Error>;

    /// Get entry by `key` from data storage
    fn get<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Get entry by `key` from auxiliary data storage
    fn get_aux<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Get entry by `key` from trees roots storage
    fn get_root<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Get entry by `key` from GroveDB metadata storage
    fn get_meta<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Initialize a new batch
    fn new_batch(&self) -> Self::Batch;

    /// Commits changes from batch into storage
    fn commit_batch(&self, batch: Self::Batch) -> Result<(), Self::Error>;

    /// Get raw iterator over storage
    fn raw_iter(&self) -> Self::RawIterator;
}

/// Database batch (not to be confused with multi-tree operations batch).
pub trait Batch {
    /// Error type for failed operations on the batch.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Appends to the database batch a put operation for a data record.
    fn put<K: AsRef<[u8]>>(&mut self, key: K, value: &[u8]) -> Result<(), Self::Error>;

    /// Appends to the database batch a put operation for aux storage.
    fn put_aux<K: AsRef<[u8]>>(&mut self, key: K, value: &[u8]) -> Result<(), Self::Error>;

    /// Appends to the database batch a put operation for subtrees roots
    /// storage.
    fn put_root<K: AsRef<[u8]>>(&mut self, key: K, value: &[u8]) -> Result<(), Self::Error>;

    /// Appends to the database batch a delete operation for a data record.
    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error>;

    /// Appends to the database batch a delete operation for aux storage.
    fn delete_aux<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error>;

    /// Appends to the database batch a delete operation for a record in subtree
    /// roots storage.
    fn delete_root<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error>;
}

/// Allows to iterate over database record inside of storage context.
pub trait RawIterator {
    /// Move iterator to first valid record.
    fn seek_to_first(&mut self);

    /// Move iterator to last valid record.
    fn seek_to_last(&mut self);

    /// Move iterator forward until `key` is hit.
    fn seek<K: AsRef<[u8]>>(&mut self, key: K);

    /// Move iterator backward until `key` is hit.
    fn seek_for_prev<K: AsRef<[u8]>>(&mut self, key: K);

    /// Move iterator to next record.
    fn next(&mut self);

    /// Move iterator to previous record.
    fn prev(&mut self);

    /// Return value of key-value pair where raw iterator points at.
    fn value(&self) -> Option<&[u8]>;

    /// Return key of key-value pair where raw iterator points at.
    fn key(&self) -> Option<&[u8]>;

    /// Check if raw iterator points into a valid record
    fn valid(&self) -> bool;
}

/// Structure to hold deferred database operations in "batched" storage
/// contexts.
#[derive(Debug)]
pub struct StorageBatch {
    operations: RefCell<Operations>,
}

#[derive(Default)]
struct Operations {
    data: BTreeMap<Vec<u8>, BatchOperation>,
    roots: BTreeMap<Vec<u8>, BatchOperation>,
    aux: BTreeMap<Vec<u8>, BatchOperation>,
    meta: BTreeMap<Vec<u8>, BatchOperation>,
}

impl std::fmt::Debug for Operations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut fmt = f.debug_struct("Operations");

        fmt.field("data", &self.data.values());
        fmt.field("aux", &self.aux.values());
        fmt.field("roots", &self.roots.values());
        fmt.field("meta", &self.meta.values());

        fmt.finish()
    }
}

impl StorageBatch {
    /// Create empty batch.
    pub fn new() -> Self {
        StorageBatch {
            operations: RefCell::new(Operations::default()),
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        let operations = self.operations.borrow();
        operations.data.len()
            + operations.roots.len()
            + operations.aux.len()
            + operations.meta.len()
    }

    /// Add deferred `put` operation
    pub fn put(&self, key: Vec<u8>, value: Vec<u8>) {
        self.operations
            .borrow_mut()
            .data
            .insert(key.clone(), BatchOperation::Put { key, value });
    }

    /// Add deferred `put` operation for aux storage
    pub fn put_aux(&self, key: Vec<u8>, value: Vec<u8>) {
        self.operations
            .borrow_mut()
            .aux
            .insert(key.clone(), BatchOperation::PutAux { key, value });
    }

    /// Add deferred `put` operation for subtree roots storage
    pub fn put_root(&self, key: Vec<u8>, value: Vec<u8>) {
        self.operations
            .borrow_mut()
            .roots
            .insert(key.clone(), BatchOperation::PutRoot { key, value });
    }

    /// Add deferred `put` operation for metadata storage
    pub fn put_meta(&self, key: Vec<u8>, value: Vec<u8>) {
        self.operations
            .borrow_mut()
            .meta
            .insert(key.clone(), BatchOperation::PutMeta { key, value });
    }

    /// Add deferred `delete` operation
    pub fn delete(&self, key: Vec<u8>) {
        let operations = &mut self.operations.borrow_mut().data;
        if operations.get(&key).is_none() {
            operations.insert(key.clone(), BatchOperation::Delete { key });
        }
    }

    /// Add deferred `delete` operation for aux storage
    pub fn delete_aux(&self, key: Vec<u8>) {
        let operations = &mut self.operations.borrow_mut().aux;
        if operations.get(&key).is_none() {
            operations.insert(key.clone(), BatchOperation::DeleteAux { key });
        }
    }

    /// Add deferred `delete` operation for subtree roots storage
    pub fn delete_root(&self, key: Vec<u8>) {
        let operations = &mut self.operations.borrow_mut().roots;
        if operations.get(&key).is_none() {
            operations.insert(key.clone(), BatchOperation::DeleteRoot { key });
        }
    }

    /// Add deferred `delete` operation for metadata storage
    pub fn delete_meta(&self, key: Vec<u8>) {
        let operations = &mut self.operations.borrow_mut().meta;
        if operations.get(&key).is_none() {
            operations.insert(key.clone(), BatchOperation::DeleteMeta { key });
        }
    }

    /// Merge batch into this one
    pub fn merge(&self, other: StorageBatch) {
        for op in other.into_iter() {
            match op {
                BatchOperation::Put { key, value } => self.put(key, value),
                BatchOperation::PutAux { key, value } => self.put_aux(key, value),
                BatchOperation::PutRoot { key, value } => self.put_root(key, value),
                BatchOperation::PutMeta { key, value } => self.put_meta(key, value),
                BatchOperation::Delete { key } => self.delete(key),
                BatchOperation::DeleteAux { key } => self.delete_aux(key),
                BatchOperation::DeleteRoot { key } => self.delete_root(key),
                BatchOperation::DeleteMeta { key } => self.delete_meta(key),
            }
        }
    }
}

/// Iterator over storage batch operations.
pub struct StorageBatchIter {
    data: IntoValues<Vec<u8>, BatchOperation>,
    aux: IntoValues<Vec<u8>, BatchOperation>,
    meta: IntoValues<Vec<u8>, BatchOperation>,
    roots: IntoValues<Vec<u8>, BatchOperation>,
}

impl Iterator for StorageBatchIter {
    type Item = BatchOperation;

    fn next(&mut self) -> Option<Self::Item> {
        self.meta
            .next()
            .or_else(|| self.aux.next())
            .or_else(|| self.roots.next())
            .or_else(|| self.data.next())
    }
}

impl IntoIterator for StorageBatch {
    type IntoIter = StorageBatchIter;
    type Item = BatchOperation;

    fn into_iter(self) -> Self::IntoIter {
        let operations = self.operations.into_inner();

        StorageBatchIter {
            data: operations.data.into_values(),
            aux: operations.aux.into_values(),
            meta: operations.meta.into_values(),
            roots: operations.roots.into_values(),
        }
    }
}

impl Default for StorageBatch {
    fn default() -> Self {
        Self::new()
    }
}

/// Deferred storage operation.
#[allow(missing_docs)]
#[derive(strum::AsRefStr)]
pub enum BatchOperation {
    /// Deferred put operation
    Put { key: Vec<u8>, value: Vec<u8> },
    /// Deferred put operation for aux storage
    PutAux { key: Vec<u8>, value: Vec<u8> },
    /// Deferred put operation for roots storage
    PutRoot { key: Vec<u8>, value: Vec<u8> },
    /// Deferred put operation for metadata storage
    PutMeta { key: Vec<u8>, value: Vec<u8> },
    /// Deferred delete operation
    Delete { key: Vec<u8> },
    /// Deferred delete operation for aux storage
    DeleteAux { key: Vec<u8> },
    /// Deferred delete operation for roots storage
    DeleteRoot { key: Vec<u8> },
    /// Deferred delete operation for metadata storage
    DeleteMeta { key: Vec<u8> },
}

impl std::fmt::Debug for BatchOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut fmt = f.debug_struct(self.as_ref());

        let mut key_buf = Vec::new();
        let mut value_buf = Vec::new();

        match self {
            BatchOperation::Put { key, value }
            | BatchOperation::PutAux { key, value }
            | BatchOperation::PutMeta { key, value }
            | BatchOperation::PutRoot { key, value } => {
                key_buf.clear();
                value_buf.clear();
                visualize_to_vec(&mut key_buf, key.as_slice());
                visualize_to_vec(&mut value_buf, value.as_slice());
                fmt.field("key", &String::from_utf8_lossy(&key_buf))
                    .field("value", &String::from_utf8_lossy(&value_buf));
            }
            BatchOperation::Delete { key }
            | BatchOperation::DeleteAux { key }
            | BatchOperation::DeleteMeta { key }
            | BatchOperation::DeleteRoot { key } => {
                key_buf.clear();
                visualize_to_vec(&mut key_buf, key.as_slice());
                fmt.field("key", &String::from_utf8_lossy(&key_buf));
            }
        }

        fmt.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_output_batch_operation() {
        let op1 = BatchOperation::PutMeta {
            key: b"key1".to_vec(),
            value: b"value1".to_vec(),
        };
        let op2 = BatchOperation::DeleteRoot {
            key: b"key1".to_vec(),
        };
        assert_eq!(
            format!("{:?}", op1),
            "PutMeta { key: \"[hex: 6b657931, str: key1]\", value: \"[hex: 76616c756531, str: \
             value1]\" }"
        );
        assert_eq!(
            format!("{:?}", op2),
            "DeleteRoot { key: \"[hex: 6b657931, str: key1]\" }"
        );
    }
}
