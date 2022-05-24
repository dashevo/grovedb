mod batch;
mod operations;
mod subtree;
#[cfg(test)]
mod tests;
mod util;
mod visualize;
use std::{collections::BTreeMap, path::Path};

pub use merk::proofs::{query::QueryItem, Query};
use merk::{self, Merk};
use rs_merkle::{algorithms::Sha256, MerkleTree};
pub use storage::{
    rocksdb_storage::{self, RocksDbStorage},
    Storage, StorageContext,
};
pub use subtree::Element;

use crate::util::{merk_optional_tx, meta_storage_context_optional_tx};

/// A key to store serialized data about subtree prefixes to restore HADS
/// structure
/// A key to store serialized data about root tree leaves keys and order
const ROOT_LEAFS_SERIALIZED_KEY: &[u8] = b"rootLeafsSerialized";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // Input data errors
    #[error("cyclic reference path")]
    CyclicReference,
    #[error("reference hops limit exceeded")]
    ReferenceLimit,
    #[error("internal error: {0}")]
    InternalError(&'static str),
    #[error("invalid proof: {0}")]
    InvalidProof(&'static str),

    // Path errors

    // The path key not found could represent a valid query, just where the path key isn't there
    #[error("path key not found: {0}")]
    PathKeyNotFound(String),
    // The path not found could represent a valid query, just where the path isn't there
    #[error("path not found: {0}")]
    PathNotFound(&'static str),
    // The invalid path represents a logical error from the client library
    #[error("invalid path: {0}")]
    InvalidPath(&'static str),
    // The corrupted path represents a consistency error in internal groveDB logic
    #[error("corrupted path: {0}")]
    CorruptedPath(&'static str),

    // Query errors
    #[error("invalid query: {0}")]
    InvalidQuery(&'static str),
    #[error("missing parameter: {0}")]
    MissingParameter(&'static str),
    // Irrecoverable errors
    #[error("storage error: {0}")]
    StorageError(#[from] rocksdb_storage::Error),
    #[error("data corruption error: {0}")]
    CorruptedData(String),
}

#[derive(Debug, Clone)]
pub struct PathQuery {
    // TODO: Make generic over path type
    path: Vec<Vec<u8>>,
    query: SizedQuery,
}

// If a subquery exists :
// limit should be applied to the elements returned by the subquery
// offset should be applied to the first item that will subqueried (first in the
// case of a range)
#[derive(Debug, Clone)]
pub struct SizedQuery {
    query: Query,
    limit: Option<u16>,
    offset: Option<u16>,
}

impl SizedQuery {
    pub const fn new(query: Query, limit: Option<u16>, offset: Option<u16>) -> Self {
        Self {
            query,
            limit,
            offset,
        }
    }
}

impl PathQuery {
    pub const fn new(path: Vec<Vec<u8>>, query: SizedQuery) -> Self {
        Self { path, query }
    }

    pub const fn new_unsized(path: Vec<Vec<u8>>, query: Query) -> Self {
        let query = SizedQuery::new(query, None, None);
        Self { path, query }
    }
}

pub struct GroveDb {
    db: RocksDbStorage,
}

pub type Transaction<'db> = <RocksDbStorage as Storage<'db>>::Transaction;
pub type TransactionArg<'db, 'a> = Option<&'a Transaction<'db>>;

impl GroveDb {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let db = RocksDbStorage::default_rocksdb_with_path(path)?;
        Ok(GroveDb { db })
    }

    // TODO: Checkpoints are currently not implemented for the transactional DB
    // pub fn checkpoint<P: AsRef<Path>>(&self, path: P) -> Result<GroveDb, Error> {
    //     // let snapshot = self.db.transaction().snapshot();
    //
    //     storage::rocksdb_storage::Checkpoint::new(&self.db)
    //         .and_then(|x| x.create_checkpoint(&path))
    //         .map_err(PrefixedRocksDbStorageError::RocksDbError)?;
    //     GroveDb::open(path)
    // }

    /// Returns root hash of GroveDb.
    /// Will be `None` if GroveDb is empty.
    pub fn root_hash(&self, transaction: TransactionArg) -> Result<Option<[u8; 32]>, Error> {
        Ok(Self::get_root_tree_internal(&self.db, transaction)?.root())
    }

    fn get_root_leaf_keys_internal<'db, S>(
        meta_storage: &S,
    ) -> Result<BTreeMap<Vec<u8>, usize>, Error>
    where
        S: StorageContext<'db>,
        Error: From<<S as StorageContext<'db>>::Error>,
    {
        let root_leaf_keys: BTreeMap<Vec<u8>, usize> = if let Some(root_leaf_keys_serialized) =
            meta_storage.get_meta(ROOT_LEAFS_SERIALIZED_KEY)?
        {
            bincode::deserialize(&root_leaf_keys_serialized).map_err(|_| {
                Error::CorruptedData(String::from("unable to deserialize root leaves"))
            })?
        } else {
            BTreeMap::new()
        };
        Ok(root_leaf_keys)
    }

    fn get_root_leaf_keys(
        &self,
        transaction: TransactionArg,
    ) -> Result<BTreeMap<Vec<u8>, usize>, Error> {
        meta_storage_context_optional_tx!(self.db, transaction, meta_storage, {
            Self::get_root_leaf_keys_internal(&meta_storage)
        })
    }

    fn get_root_tree_internal(
        db: &RocksDbStorage,
        transaction: TransactionArg,
    ) -> Result<MerkleTree<Sha256>, Error> {
        let root_leaf_keys = meta_storage_context_optional_tx!(db, transaction, meta_storage, {
            Self::get_root_leaf_keys_internal(&meta_storage)?
        });

        let mut leaf_hashes: Vec<[u8; 32]> = vec![[0; 32]; root_leaf_keys.len()];
        for (subtree_path, root_leaf_idx) in root_leaf_keys {
            merk_optional_tx!(db, [subtree_path.as_slice()], transaction, subtree, {
                leaf_hashes[root_leaf_idx] = subtree.root_hash();
            });
        }
        Ok(MerkleTree::<Sha256>::from_leaves(&leaf_hashes))
    }

    pub fn get_root_tree(&self, transaction: TransactionArg) -> Result<MerkleTree<Sha256>, Error> {
        Self::get_root_tree_internal(&self.db, transaction)
    }

    /// Method to propagate updated subtree root hashes up to GroveDB root
    fn propagate_changes<'p, P>(&self, path: P, transaction: TransactionArg) -> Result<(), Error>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: DoubleEndedIterator + ExactSizeIterator + Clone,
    {
        // Go up until only one element in path, which means a key of a root tree
        let mut path_iter = path.into_iter();

        while path_iter.len() > 1 {
            if let Some(tx) = transaction {
                let subtree_storage = self
                    .db
                    .get_transactional_storage_context(path_iter.clone(), tx);
                let subtree = Merk::open(subtree_storage)
                    .map_err(|_| Error::CorruptedData("cannot open a subtree".to_owned()))?;
                let element = Element::Tree(subtree.root_hash());
                let key = path_iter.next_back().expect("next element is `Some`");
                let parent_storage = self
                    .db
                    .get_transactional_storage_context(path_iter.clone(), tx);
                let mut parent_tree = Merk::open(parent_storage)
                    .map_err(|_| Error::CorruptedData("cannot open a subtree".to_owned()))?;
                element.insert(&mut parent_tree, key.as_ref())?;
            } else {
                let subtree_storage = self.db.get_storage_context(path_iter.clone());
                let subtree = Merk::open(subtree_storage)
                    .map_err(|_| Error::CorruptedData("cannot open a subtree".to_owned()))?;
                let element = Element::Tree(subtree.root_hash());
                let key = path_iter.next_back().expect("next element is `Some`");
                let parent_storage = self.db.get_storage_context(path_iter.clone());
                let mut parent_tree = Merk::open(parent_storage)
                    .map_err(|_| Error::CorruptedData("cannot open a subtree".to_owned()))?;
                element.insert(&mut parent_tree, key.as_ref())?;
            }
        }

        Ok(())
    }

    pub fn flush(&self) -> Result<(), Error> {
        Ok(self.db.flush()?)
    }

    /// Starts database transaction. Please note that you have to start
    /// underlying storage transaction manually.
    ///
    /// ## Examples:
    /// ```
    /// # use grovedb::{Element, Error, GroveDb};
    /// # use rs_merkle::{MerkleTree, MerkleProof, algorithms::Sha256, Hasher, utils};
    /// # use std::convert::TryFrom;
    /// # use tempfile::TempDir;
    /// #
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// const TEST_LEAF: &[u8] = b"test_leaf";
    ///
    /// let tmp_dir = TempDir::new().unwrap();
    /// let mut db = GroveDb::open(tmp_dir.path())?;
    /// db.insert([], TEST_LEAF, Element::empty_tree(), None)?;
    ///
    /// let tx = db.start_transaction();
    ///
    /// let subtree_key = b"subtree_key";
    /// db.insert([TEST_LEAF], subtree_key, Element::empty_tree(), Some(&tx))?;
    ///
    /// // This action exists only inside the transaction for now
    /// let result = db.get([TEST_LEAF], subtree_key, None);
    /// assert!(matches!(result, Err(Error::PathKeyNotFound(_))));
    ///
    /// // To access values inside the transaction, transaction needs to be passed to the `db::get`
    /// let result_with_transaction = db.get([TEST_LEAF], subtree_key, Some(&tx))?;
    /// assert_eq!(result_with_transaction, Element::empty_tree());
    ///
    /// // After transaction is committed, the value from it can be accessed normally.
    /// db.commit_transaction(tx);
    /// let result = db.get([TEST_LEAF], subtree_key, None)?;
    /// assert_eq!(result, Element::empty_tree());
    ///
    /// # Ok(())
    /// # }
    /// ```
    pub fn start_transaction(&self) -> Transaction {
        self.db.start_transaction()
    }

    /// Commits previously started db transaction. For more details on the
    /// transaction usage, please check [`GroveDb::start_transaction`]
    pub fn commit_transaction(&self, transaction: Transaction) -> Result<(), Error> {
        Ok(self.db.commit_transaction(transaction)?)
    }

    /// Rollbacks previously started db transaction to initial state.
    /// For more details on the transaction usage, please check
    /// [`GroveDb::start_transaction`]
    pub fn rollback_transaction(&self, transaction: &Transaction) -> Result<(), Error> {
        Ok(self.db.rollback_transaction(transaction)?)
    }
}
