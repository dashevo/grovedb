mod subtree;
#[cfg(test)]
mod tests;
mod transaction;

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    rc::Rc,
};

pub use merk::proofs::{query::QueryItem, Query};
use merk::{self, Merk};
use rs_merkle::{algorithms::Sha256, MerkleTree};
use storage::{
    rocksdb_storage::{
        OptimisticTransactionDBTransaction, PrefixedRocksDbStorage, PrefixedRocksDbStorageError,
    },
    Storage, Transaction,
};
pub use subtree::Element;

use crate::transaction::GroveDbTransaction;
// pub use transaction::GroveDbTransaction;

/// Limit of possible indirections
const MAX_REFERENCE_HOPS: usize = 10;
/// A key to store serialized data about subtree prefixes to restore HADS
/// structure
const SUBTRESS_SERIALIZED_KEY: &[u8] = b"subtreesSerialized";
/// A key to store serialized data about root tree leafs keys and order
const ROOT_LEAFS_SERIALIZED_KEY: &[u8] = b"rootLeafsSerialized";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // Input data errors
    #[error("cyclic reference path")]
    CyclicReference,
    #[error("reference hops limit exceeded")]
    ReferenceLimit,
    #[error("invalid path: {0}")]
    InvalidPath(&'static str),
    // Irrecoverable errors
    #[error("storage error: {0}")]
    StorageError(#[from] PrefixedRocksDbStorageError),
    #[error("data corruption error: {0}")]
    CorruptedData(String),
}

pub struct GroveDb {
    pub root_tree: MerkleTree<Sha256>,
    root_leaf_keys: HashMap<Vec<u8>, usize>,
    subtrees: HashMap<Vec<u8>, Merk<PrefixedRocksDbStorage>>,
    meta_storage: PrefixedRocksDbStorage,
    pub(crate) db: Rc<storage::rocksdb_storage::OptimisticTransactionDB>,
    // Locks the database for writes during the transaction
    is_readonly: bool,
    // Temp trees used for writes during transaction
    pub temp_root_tree: MerkleTree<Sha256>,
    temp_root_leaf_keys: HashMap<Vec<u8>, usize>,
    temp_subtrees: HashMap<Vec<u8>, Merk<PrefixedRocksDbStorage>>,
}

impl GroveDb {
    pub fn new(
        root_tree: MerkleTree<Sha256>,
        root_leaf_keys: HashMap<Vec<u8>, usize>,
        subtrees: HashMap<Vec<u8>, Merk<PrefixedRocksDbStorage>>,
        meta_storage: PrefixedRocksDbStorage,
        db: Rc<storage::rocksdb_storage::OptimisticTransactionDB>,
    ) -> Self {
        Self {
            root_tree,
            root_leaf_keys,
            subtrees,
            meta_storage,
            db,
            temp_root_tree: MerkleTree::new(),
            temp_root_leaf_keys: HashMap::new(),
            temp_subtrees: HashMap::new(),
            is_readonly: false,
        }
    }

    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let db = Rc::new(
            storage::rocksdb_storage::OptimisticTransactionDB::open_cf_descriptors(
                &storage::rocksdb_storage::default_db_opts(),
                path,
                storage::rocksdb_storage::column_families(),
            )
            .map_err(Into::<PrefixedRocksDbStorageError>::into)?,
        );
        let meta_storage = PrefixedRocksDbStorage::new(db.clone(), Vec::new())?;

        let mut subtrees = HashMap::new();
        // TODO: owned `get` is not required for deserialization
        if let Some(prefixes_serialized) = meta_storage.get_meta(SUBTRESS_SERIALIZED_KEY)? {
            let subtrees_prefixes: Vec<Vec<u8>> = bincode::deserialize(&prefixes_serialized)
                .map_err(|_| {
                    Error::CorruptedData(String::from("unable to deserialize prefixes"))
                })?;
            for prefix in subtrees_prefixes {
                let subtree_merk =
                    Merk::open(PrefixedRocksDbStorage::new(db.clone(), prefix.to_vec())?)
                        .map_err(|e| Error::CorruptedData(e.to_string()))?;
                subtrees.insert(prefix.to_vec(), subtree_merk);
            }
        }

        // TODO: owned `get` is not required for deserialization
        let root_leaf_keys: HashMap<Vec<u8>, usize> = if let Some(root_leaf_keys_serialized) =
            meta_storage.get_meta(ROOT_LEAFS_SERIALIZED_KEY)?
        {
            bincode::deserialize(&root_leaf_keys_serialized).map_err(|_| {
                Error::CorruptedData(String::from("unable to deserialize root leafs"))
            })?
        } else {
            HashMap::new()
        };

        Ok(GroveDb::new(
            Self::build_root_tree(&subtrees, &root_leaf_keys),
            root_leaf_keys,
            subtrees,
            meta_storage,
            db,
        ))
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

    fn store_subtrees_keys_data(
        &self,
        db_transaction: Option<&OptimisticTransactionDBTransaction>,
    ) -> Result<(), Error> {
        let subtrees = match db_transaction {
            None => &self.subtrees,
            Some(_) => &self.temp_subtrees,
        };

        let prefixes: Vec<Vec<u8>> = subtrees.keys().map(|x| x.clone()).collect();

        // TODO: make StorageOrTransaction which will has the access to either storage
        // or transaction
        match db_transaction {
            None => {
                self.meta_storage.put_meta(
                    SUBTRESS_SERIALIZED_KEY,
                    &bincode::serialize(&prefixes).map_err(|_| {
                        Error::CorruptedData(String::from("unable to serialize prefixes"))
                    })?,
                )?;
                self.meta_storage.put_meta(
                    ROOT_LEAFS_SERIALIZED_KEY,
                    &bincode::serialize(&self.temp_root_leaf_keys).map_err(|_| {
                        Error::CorruptedData(String::from("unable to serialize root leafs"))
                    })?,
                )?;
            }
            Some(tx) => {
                let transaction = self.meta_storage.transaction(tx);
                transaction.put_meta(
                    SUBTRESS_SERIALIZED_KEY,
                    &bincode::serialize(&prefixes).map_err(|_| {
                        Error::CorruptedData(String::from("unable to serialize prefixes"))
                    })?,
                )?;
                transaction.put_meta(
                    ROOT_LEAFS_SERIALIZED_KEY,
                    &bincode::serialize(&self.root_leaf_keys).map_err(|_| {
                        Error::CorruptedData(String::from("unable to serialize root leafs"))
                    })?,
                )?;
            }
        }

        Ok(())
    }

    fn build_root_tree(
        subtrees: &HashMap<Vec<u8>, Merk<PrefixedRocksDbStorage>>,
        root_leaf_keys: &HashMap<Vec<u8>, usize>,
    ) -> MerkleTree<Sha256> {
        let mut leaf_hashes: Vec<[u8; 32]> = vec![[0; 32]; root_leaf_keys.len()];
        for (subtree_path, root_leaf_idx) in root_leaf_keys {
            let subtree_merk = subtrees
                .get(subtree_path)
                .expect("`root_leaf_keys` must be in sync with `subtrees`");
            leaf_hashes[*root_leaf_idx] = subtree_merk.root_hash();
        }
        let res = MerkleTree::<Sha256>::from_leaves(&leaf_hashes);
        res
    }

    // TODO: split the function into smaller ones
    pub fn insert<'a: 'b, 'b>(
        &'a mut self,
        path: &[&[u8]],
        key: Vec<u8>,
        mut element: subtree::Element,
        transaction: Option<&'b <PrefixedRocksDbStorage as Storage>::DBTransaction<'b>>,
    ) -> Result<(), Error> {
        let subtrees = match transaction {
            None => &mut self.subtrees,
            Some(_) => &mut self.temp_subtrees,
        };

        let root_leaf_keys = match transaction {
            None => &mut self.root_leaf_keys,
            Some(_) => &mut self.temp_root_leaf_keys,
        };

        let root_tree = match transaction {
            None => &mut self.root_tree,
            Some(_) => &mut self.temp_root_tree,
        };

        let compressed_path = Self::compress_path(path, None);
        match &mut element {
            Element::Tree(subtree_root_hash) => {
                // Helper closure to create a new subtree under path + key
                let create_subtree_merk =
                    || -> Result<(Vec<u8>, Merk<PrefixedRocksDbStorage>), Error> {
                        let compressed_path_subtree = Self::compress_path(path, Some(&key));
                        Ok((
                            compressed_path_subtree.clone(),
                            Merk::open(PrefixedRocksDbStorage::new(
                                self.db.clone(),
                                compressed_path_subtree,
                            )?)
                            .map_err(|e| Error::CorruptedData(e.to_string()))?,
                        ))
                    };
                if path.is_empty() {
                    // Add subtree to the root tree

                    // Open Merk and put handle into `subtrees` dictionary accessible by its
                    // compressed path
                    let (compressed_path_subtree, subtree_merk) = create_subtree_merk()?;
                    subtrees.insert(compressed_path_subtree.clone(), subtree_merk);

                    // Update root leafs index to persist rs-merkle structure later
                    if root_leaf_keys.get(&compressed_path_subtree).is_none() {
                        root_leaf_keys.insert(compressed_path_subtree, root_tree.leaves_len());
                    }
                    self.propagate_changes(&[&key], transaction)?;
                } else {
                    // Add subtree to another subtree.
                    // First, check if a subtree exists to create a new subtree under it
                    subtrees
                        .get(&compressed_path)
                        .ok_or(Error::InvalidPath("no subtree found under that path"))?;
                    let (compressed_path_subtree, subtree_merk) = create_subtree_merk()?;
                    // Set tree value as a a subtree root hash
                    *subtree_root_hash = subtree_merk.root_hash();
                    subtrees.insert(compressed_path_subtree, subtree_merk);
                    // Had to take merk from `subtrees` once again to solve multiple &mut s
                    let mut merk = subtrees
                        .get_mut(&compressed_path)
                        .expect("merk object must exist in `subtrees`");
                    // need to mark key as taken in the upper tree
                    element.insert(&mut merk, key, transaction)?;
                    self.propagate_changes(path, transaction)?;
                }
                self.store_subtrees_keys_data(transaction)?;
            }
            _ => {
                // If path is empty that means there is an attempt to insert something into a
                // root tree and this branch is for anything but trees
                if path.is_empty() {
                    return Err(Error::InvalidPath(
                        "only subtrees are allowed as root tree's leafs",
                    ));
                }
                // Get a Merk by a path
                let mut merk = subtrees
                    .get_mut(&compressed_path)
                    .ok_or(Error::InvalidPath("no subtree found under that path"))?;
                element.insert(&mut merk, key, transaction)?;
                self.propagate_changes(path, transaction)?;
            }
        }
        Ok(())
    }

    pub fn insert_if_not_exists<'a: 'b, 'b>(
        &mut self,
        path: &[&[u8]],
        key: Vec<u8>,
        element: subtree::Element,
        transaction: Option<&'b <PrefixedRocksDbStorage as Storage>::DBTransaction<'b>>,
    ) -> Result<bool, Error> {
        if self.get(path, &key, transaction).is_ok() {
            return Ok(false);
        }
        match self.insert(path, key, element, transaction) {
            Ok(_) => Ok(true),
            Err(e) => Err(e),
        }
    }

    pub fn get<'a>(
        &self,
        path: &[&[u8]],
        key: &[u8],
        transaction: Option<&OptimisticTransactionDBTransaction>,
    ) -> Result<subtree::Element, Error> {
        match self.get_raw(path, key, transaction)? {
            Element::Reference(reference_path) => {
                self.follow_reference(reference_path, transaction)
            }
            other => Ok(other),
        }
    }

    /// Get tree item without following references
    fn get_raw(
        &self,
        path: &[&[u8]],
        key: &[u8],
        transaction: Option<&OptimisticTransactionDBTransaction>,
    ) -> Result<subtree::Element, Error> {
        let subtrees = match transaction {
            None => &self.subtrees,
            Some(_) => &self.temp_subtrees,
        };
        let merk = subtrees
            .get(&Self::compress_path(path, None))
            .ok_or(Error::InvalidPath("no subtree found under that path"))?;

        Element::get(&merk, key)
    }

    fn follow_reference(
        &self,
        mut path: Vec<Vec<u8>>,
        transaction: Option<&OptimisticTransactionDBTransaction>,
    ) -> Result<subtree::Element, Error> {
        let mut hops_left = MAX_REFERENCE_HOPS;
        let mut current_element;
        let mut visited = HashSet::new();

        while hops_left > 0 {
            if visited.contains(&path) {
                return Err(Error::CyclicReference);
            }
            if let Some((key, path_slice)) = path.split_last() {
                current_element = self.get_raw(
                    path_slice
                        .iter()
                        .map(|x| x.as_slice())
                        .collect::<Vec<_>>()
                        .as_slice(),
                    key,
                    transaction,
                )?;
            } else {
                return Err(Error::InvalidPath("empty path"));
            }
            visited.insert(path);
            match current_element {
                Element::Reference(reference_path) => path = reference_path,
                other => return Ok(other),
            }
            hops_left -= 1;
        }
        Err(Error::ReferenceLimit)
    }

    pub fn proof(&self, path: &[&[u8]], proof_query: Query) -> Result<Vec<Vec<u8>>, Error> {
        let mut proofs: Vec<Vec<u8>> = Vec::new();

        // First prove the query
        proofs.push(self.prove_item(path, proof_query)?);

        // Next prove the query path
        let mut split_path = path.split_last();
        while let Some((key, path_slice)) = split_path {
            if path_slice.is_empty() {
                // Get proof for root tree at current key
                let root_key_index = self
                    .root_leaf_keys
                    .get(*key)
                    .ok_or(Error::InvalidPath("root key not found"))?;
                proofs.push(self.root_tree.proof(&[*root_key_index]).to_bytes());
            } else {
                let mut path_query = Query::new();
                path_query.insert_item(QueryItem::Key(key.to_vec()));
                proofs.push(self.prove_item(path_slice, path_query)?);
            }
            split_path = path_slice.split_last();
        }

        // Append the root leaf keys hash map to proof to provide context when verifying
        // proof
        let aux_data = bincode::serialize(&self.root_leaf_keys)
            .map_err(|_| Error::CorruptedData(String::from("unable to deserialize element")))?;
        proofs.push(aux_data);

        Ok(proofs)
    }

    fn prove_item(&self, path: &[&[u8]], proof_query: Query) -> Result<Vec<u8>, Error> {
        let merk = self
            .subtrees
            .get(&Self::compress_path(path, None))
            .ok_or(Error::InvalidPath("no subtree found under that path"))?;

        let proof_result = merk
            .prove(proof_query)
            .expect("should prove both inclusion and absence");

        Ok(proof_result)
    }

    /// Method to propagate updated subtree root hashes up to GroveDB root
    fn propagate_changes<'a: 'b, 'b>(
        &'a mut self,
        path: &[&[u8]],
        transaction: Option<&'b <PrefixedRocksDbStorage as Storage>::DBTransaction<'b>>,
    ) -> Result<(), Error> {
        let subtrees = match transaction {
            None => &mut self.subtrees,
            Some(_) => &mut self.temp_subtrees,
        };

        let root_leaf_keys = match transaction {
            None => &mut self.root_leaf_keys,
            Some(_) => &mut self.temp_root_leaf_keys,
        };

        let mut split_path = path.split_last();
        // Go up until only one element in path, which means a key of a root tree
        while let Some((key, path_slice)) = split_path {
            if path_slice.is_empty() {
                // Hit the root tree
                match transaction {
                    None => self.root_tree = Self::build_root_tree(&subtrees, &root_leaf_keys),
                    Some(_) => {
                        self.temp_root_tree = Self::build_root_tree(&subtrees, &root_leaf_keys)
                    }
                };
                break;
            } else {
                let compressed_path_upper_tree = Self::compress_path(path_slice, None);
                let compressed_path_subtree = Self::compress_path(path_slice, Some(key));
                let subtree = subtrees
                    .get(&compressed_path_subtree)
                    .ok_or(Error::InvalidPath("no subtree found under that path"))?;
                let element = Element::Tree(subtree.root_hash());
                let upper_tree = subtrees
                    .get_mut(&compressed_path_upper_tree)
                    .ok_or(Error::InvalidPath("no subtree found under that path"))?;
                element.insert(upper_tree, key.to_vec(), transaction)?;
                split_path = path_slice.split_last();
            }
        }
        Ok(())
    }

    /// A helper method to build a prefix to rocksdb keys or identify a subtree
    /// in `subtrees` map by tree path;
    fn compress_path(path: &[&[u8]], key: Option<&[u8]>) -> Vec<u8> {
        let mut res = path.iter().fold(Vec::<u8>::new(), |mut acc, p| {
            acc.extend(p.into_iter());
            acc
        });
        if let Some(k) = key {
            res.extend_from_slice(k);
        }
        res
    }

    pub fn storage(&self) -> Rc<storage::rocksdb_storage::OptimisticTransactionDB> {
        self.db.clone()
    }

    pub fn start_transaction(&mut self) {
        // Locking all writes outside of the transaction
        self.is_readonly = true;

        // Cloning all the trees to maintain original state before the transaction
        self.temp_root_tree = self.root_tree.clone();
        self.temp_root_leaf_keys = self.root_leaf_keys.clone();
        self.temp_subtrees = self.subtrees.clone();
    }

    // pub fn commit_transaction(&mut self) {
    //     // Enabling writes again
    //     self.is_readonly = false;
    //
    //     // Copying all changes that were made during the transaction into the db
    //     self.root_tree = self.temp_root_tree.clone();
    //     self.root_leaf_keys = self.temp_root_leaf_keys.drain().collect();
    //     self.subtrees = self.temp_subtrees.drain().collect();
    //
    //     // TODO: root tree actually does support transactions, no need to do that
    //     self.temp_root_tree = MerkleTree::new();
    // }
}
