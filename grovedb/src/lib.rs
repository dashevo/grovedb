pub mod batch;
mod operations;
mod query;
mod subtree;
#[cfg(test)]
mod tests;
mod util;
mod visualize;

use std::{collections::BTreeMap, path::Path};

use costs::{
    cost_return_on_error, cost_return_on_error_no_add, CostContext, CostsExt, OperationCost,
};
pub use merk::proofs::{query::QueryItem, Query};
use merk::{self, Merk};
pub use query::{PathQuery, SizedQuery};
use rs_merkle::{algorithms::Sha256, MerkleTree};
pub use storage::{
    rocksdb_storage::{self, RocksDbStorage},
    Storage, StorageContext,
};
pub use subtree::{Element, ElementFlags};

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

    // Support errors
    #[error("not supported: {0}")]
    NotSupported(&'static str),
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
    pub fn root_hash(
        &self,
        transaction: TransactionArg,
    ) -> CostContext<Result<Option<[u8; 32]>, Error>> {
        Self::get_root_tree_internal(&self.db, transaction).map_ok(|x| x.root())
    }

    fn get_root_leaf_keys_internal<'db, S>(
        meta_storage: &S,
    ) -> CostContext<Result<BTreeMap<Vec<u8>, usize>, Error>>
    where
        S: StorageContext<'db>,
        Error: From<<S as StorageContext<'db>>::Error>,
    {
        let mut cost = OperationCost {
            seek_count: 1,
            ..Default::default()
        };

        let root_leaf_keys: BTreeMap<Vec<u8>, usize> = if let Some(root_leaf_keys_serialized) = cost_return_on_error_no_add!(
            &cost,
            meta_storage
                .get_meta(ROOT_LEAFS_SERIALIZED_KEY)
                .map_err(|e| e.into())
        ) {
            cost.loaded_bytes += root_leaf_keys_serialized.len();
            cost_return_on_error_no_add!(
                &cost,
                bincode::deserialize(&root_leaf_keys_serialized).map_err(|_| {
                    Error::CorruptedData(String::from("unable to deserialize root leaves"))
                })
            )
        } else {
            BTreeMap::new()
        };
        Ok(root_leaf_keys).wrap_with_cost(cost)
    }

    fn get_root_leaf_keys(
        &self,
        transaction: TransactionArg,
    ) -> CostContext<Result<BTreeMap<Vec<u8>, usize>, Error>> {
        meta_storage_context_optional_tx!(self.db, transaction, meta_storage, {
            Self::get_root_leaf_keys_internal(&meta_storage)
        })
    }

    fn get_root_tree_internal(
        db: &RocksDbStorage,
        transaction: TransactionArg,
    ) -> CostContext<Result<MerkleTree<Sha256>, Error>> {
        let mut cost = OperationCost::default();

        let root_leaf_keys = meta_storage_context_optional_tx!(db, transaction, meta_storage, {
            cost_return_on_error!(&mut cost, Self::get_root_leaf_keys_internal(&meta_storage))
        });

        let mut leaf_hashes: Vec<[u8; 32]> = vec![[0; 32]; root_leaf_keys.len()];
        for (subtree_path, root_leaf_idx) in root_leaf_keys {
            merk_optional_tx!(
                &mut cost,
                db,
                [subtree_path.as_slice()],
                transaction,
                subtree,
                {
                    leaf_hashes[root_leaf_idx] = subtree.root_hash().unwrap_add_cost(&mut cost);
                }
            );
        }
        Ok(MerkleTree::<Sha256>::from_leaves(&leaf_hashes)).wrap_with_cost(cost)
    }

    pub fn get_root_tree(
        &self,
        transaction: TransactionArg,
    ) -> CostContext<Result<MerkleTree<Sha256>, Error>> {
        Self::get_root_tree_internal(&self.db, transaction)
    }

    /// Method to propagate updated subtree root hashes up to GroveDB root
    fn propagate_changes<'p, P>(
        &self,
        path: P,
        transaction: TransactionArg,
    ) -> CostContext<Result<(), Error>>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: DoubleEndedIterator + ExactSizeIterator + Clone,
    {
        let mut cost = OperationCost::default();

        // Go up until only one element in path, which means a key of a root tree
        let mut path_iter = path.into_iter();

        while path_iter.len() > 1 {
            if let Some(tx) = transaction {
                let subtree_storage = self
                    .db
                    .get_transactional_storage_context(path_iter.clone(), tx);
                let subtree = cost_return_on_error!(
                    &mut cost,
                    Merk::open(subtree_storage)
                        .map_err(|_| Error::CorruptedData("cannot open a subtree".to_owned()))
                );
                let key = path_iter.next_back().expect("next element is `Some`");
                let parent_storage = self
                    .db
                    .get_transactional_storage_context(path_iter.clone(), tx);
                let mut parent_tree = cost_return_on_error!(
                    &mut cost,
                    Merk::open(parent_storage)
                        .map_err(|_| Error::CorruptedData("cannot open a subtree".to_owned()))
                );
                cost_return_on_error!(
                    &mut cost,
                    Self::update_tree_item_preserve_flag(
                        &mut parent_tree,
                        key,
                        subtree.root_hash().unwrap_add_cost(&mut cost),
                    )
                );
            } else {
                let subtree_storage = self.db.get_storage_context(path_iter.clone());
                let subtree = cost_return_on_error!(
                    &mut cost,
                    Merk::open(subtree_storage)
                        .map_err(|_| Error::CorruptedData("cannot open a subtree".to_owned()))
                );
                let key = path_iter.next_back().expect("next element is `Some`");
                let parent_storage = self.db.get_storage_context(path_iter.clone());
                let mut parent_tree = cost_return_on_error!(
                    &mut cost,
                    Merk::open(parent_storage)
                        .map_err(|_| Error::CorruptedData("cannot open a subtree".to_owned()))
                );
                cost_return_on_error!(
                    &mut cost,
                    Self::update_tree_item_preserve_flag(
                        &mut parent_tree,
                        key,
                        subtree.root_hash().unwrap_add_cost(&mut cost),
                    )
                );
            }
        }

        Ok(()).wrap_with_cost(cost)
    }

    fn update_tree_item_preserve_flag<'db, K: AsRef<[u8]> + Copy, S: StorageContext<'db>>(
        parent_tree: &mut Merk<S>,
        key: K,
        root_hash: [u8; 32],
    ) -> CostContext<Result<(), Error>> {
        Self::get_element_from_subtree(&parent_tree, key).flat_map_ok(|element| {
            if let Element::Tree(_, flag) = element {
                let tree = Element::new_tree_with_flags(root_hash, flag);
                tree.insert(parent_tree, key.as_ref())
            } else {
                Err(Error::InvalidPath("can only propagate on tree items"))
                    .wrap_with_cost(Default::default())
            }
        })
    }

    fn get_element_from_subtree<'db, K: AsRef<[u8]>, S: StorageContext<'db>>(
        subtree: &Merk<S>,
        key: K,
    ) -> CostContext<Result<Element, Error>> {
        subtree
            .get(key.as_ref())
            .map_err(|_| Error::InvalidPath("can't find subtree in parent during propagation"))
            .map_ok(|subtree_opt| {
                subtree_opt.ok_or(Error::InvalidPath(
                    "can't find subtree in parent during propagation",
                ))
            })
            .flatten()
            .map_ok(|element_bytes| {
                Element::deserialize(&element_bytes).map_err(|_| {
                    Error::CorruptedData(
                        "failed to deserialized parent during propagation".to_owned(),
                    )
                })
            })
            .flatten()
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
    /// db.insert([], TEST_LEAF, Element::empty_tree(), None)
    ///     .unwrap()?;
    ///
    /// let tx = db.start_transaction();
    ///
    /// let subtree_key = b"subtree_key";
    /// db.insert([TEST_LEAF], subtree_key, Element::empty_tree(), Some(&tx))
    ///     .unwrap()?;
    ///
    /// // This action exists only inside the transaction for now
    /// let result = db.get([TEST_LEAF], subtree_key, None).unwrap();
    /// assert!(matches!(result, Err(Error::PathKeyNotFound(_))));
    ///
    /// // To access values inside the transaction, transaction needs to be passed to the `db::get`
    /// let result_with_transaction = db.get([TEST_LEAF], subtree_key, Some(&tx)).unwrap()?;
    /// assert_eq!(result_with_transaction, Element::empty_tree());
    ///
    /// // After transaction is committed, the value from it can be accessed normally.
    /// db.commit_transaction(tx);
    /// let result = db.get([TEST_LEAF], subtree_key, None).unwrap()?;
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
