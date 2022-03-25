//! Prefixed storage batch implementation for RocksDB backend.
use std::convert::Infallible;

use rocksdb::{ColumnFamily, WriteBatchWithTransaction};

use super::{make_prefixed_key, PrefixedRocksDbTransactionContext};
use crate::{Batch, BatchOperation, StorageBatch, StorageContext};

/// Wrapper to RocksDB batch
pub struct PrefixedRocksDbBatch<'db, B> {
    pub(crate) prefix: Vec<u8>,
    pub(crate) batch: B,
    pub(crate) cf_aux: &'db ColumnFamily,
    pub(crate) cf_roots: &'db ColumnFamily,
}

/// Batch with no backing storage that eventually will be merged into
/// multi-context batch.
pub struct PrefixedMultiContextBatchPart {
    pub(crate) prefix: Vec<u8>,
    pub(crate) batch: StorageBatch,
}

/// Implementation of a batch ouside a transaction
impl<'db> Batch for PrefixedRocksDbBatch<'db, WriteBatchWithTransaction<true>> {
    type Error = Infallible;

    fn put<K: AsRef<[u8]>>(&mut self, key: K, value: &[u8]) -> Result<(), Self::Error> {
        self.batch
            .put(make_prefixed_key(self.prefix.clone(), key), value);
        Ok(())
    }

    fn put_aux<K: AsRef<[u8]>>(&mut self, key: K, value: &[u8]) -> Result<(), Self::Error> {
        self.batch.put_cf(
            self.cf_aux,
            make_prefixed_key(self.prefix.clone(), key),
            value,
        );
        Ok(())
    }

    fn put_root<K: AsRef<[u8]>>(&mut self, key: K, value: &[u8]) -> Result<(), Self::Error> {
        self.batch.put_cf(
            self.cf_roots,
            make_prefixed_key(self.prefix.clone(), key),
            value,
        );
        Ok(())
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        self.batch
            .delete(make_prefixed_key(self.prefix.clone(), key));
        Ok(())
    }

    fn delete_aux<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        self.batch
            .delete_cf(self.cf_aux, make_prefixed_key(self.prefix.clone(), key));
        Ok(())
    }

    fn delete_root<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        self.batch
            .delete_cf(self.cf_roots, make_prefixed_key(self.prefix.clone(), key));
        Ok(())
    }
}

/// Implementation of a batch inside a transaction.
/// Basically just proxies all calls to the underlying transaction.
impl<'db, 'ctx> Batch for &'ctx PrefixedRocksDbTransactionContext<'db> {
    type Error = <PrefixedRocksDbTransactionContext<'db> as StorageContext<'db, 'ctx>>::Error;

    fn put<K: AsRef<[u8]>>(&mut self, key: K, value: &[u8]) -> Result<(), Self::Error> {
        StorageContext::put(*self, key, value)
    }

    fn put_aux<K: AsRef<[u8]>>(&mut self, key: K, value: &[u8]) -> Result<(), Self::Error> {
        StorageContext::put_aux(*self, key, value)
    }

    fn put_root<K: AsRef<[u8]>>(&mut self, key: K, value: &[u8]) -> Result<(), Self::Error> {
        StorageContext::put_root(*self, key, value)
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        StorageContext::delete(*self, key)
    }

    fn delete_aux<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        StorageContext::delete_aux(*self, key)
    }

    fn delete_root<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        StorageContext::delete_root(*self, key)
    }
}

/// Implementation of a batch ouside a transaction
impl Batch for PrefixedMultiContextBatchPart {
    type Error = Infallible;

    fn put<K: AsRef<[u8]>>(&mut self, key: K, value: &[u8]) -> Result<(), Self::Error> {
        self.batch
            .put(make_prefixed_key(self.prefix.clone(), key), value.to_vec());
        Ok(())
    }

    fn put_aux<K: AsRef<[u8]>>(&mut self, key: K, value: &[u8]) -> Result<(), Self::Error> {
        self.batch
            .put_aux(make_prefixed_key(self.prefix.clone(), key), value.to_vec());
        Ok(())
    }

    fn put_root<K: AsRef<[u8]>>(&mut self, key: K, value: &[u8]) -> Result<(), Self::Error> {
        self.batch
            .put_root(make_prefixed_key(self.prefix.clone(), key), value.to_vec());
        Ok(())
    }

    fn delete<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        self.batch
            .delete(make_prefixed_key(self.prefix.clone(), key));
        Ok(())
    }

    fn delete_aux<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        self.batch
            .delete_aux(make_prefixed_key(self.prefix.clone(), key));
        Ok(())
    }

    fn delete_root<K: AsRef<[u8]>>(&mut self, key: K) -> Result<(), Self::Error> {
        self.batch
            .delete_root(make_prefixed_key(self.prefix.clone(), key));
        Ok(())
    }
}
