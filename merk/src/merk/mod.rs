pub mod chunks;

// TODO
// pub mod restore;
use std::{
    cell::Cell,
    cmp::Ordering,
    collections::{BTreeSet, LinkedList},
    fmt,
};

use anyhow::{anyhow, Result};
use costs::{cost_return_on_error, CostContext, CostsExt, OperationCost};
use storage::{self, Batch, RawIterator, StorageContext};

use crate::{
    proofs::{encode_into, query::QueryItem, Op as ProofOp, Query},
    tree::{Commit, Fetch, Hash, Link, MerkBatch, Op, RefWalker, Tree, Walker, NULL_HASH},
};

pub const ROOT_KEY_KEY: &[u8] = b"root";

type Proof = (LinkedList<ProofOp>, Option<u16>, Option<u16>);

pub struct ProofConstructionResult {
    pub proof: Vec<u8>,
    pub limit: Option<u16>,
    pub offset: Option<u16>,
}

impl ProofConstructionResult {
    pub fn new(proof: Vec<u8>, limit: Option<u16>, offset: Option<u16>) -> Self {
        Self {
            proof,
            limit,
            offset,
        }
    }
}

pub struct ProofWithoutEncodingResult {
    pub proof: LinkedList<ProofOp>,
    pub limit: Option<u16>,
    pub offset: Option<u16>,
}

impl ProofWithoutEncodingResult {
    pub fn new(proof: LinkedList<ProofOp>, limit: Option<u16>, offset: Option<u16>) -> Self {
        Self {
            proof,
            limit,
            offset,
        }
    }
}

/// KVIterator allows you to lazily iterate over each kv pair of a subtree
pub struct KVIterator<'a, I: RawIterator> {
    raw_iter: I,
    _query: &'a Query,
    left_to_right: bool,
    query_iterator: Box<dyn Iterator<Item = &'a QueryItem> + 'a>,
    current_query_item: Option<&'a QueryItem>,
}

impl<'a, I: RawIterator> KVIterator<'a, I> {
    pub fn new(raw_iter: I, query: &'a Query) -> CostContext<Self> {
        let mut cost = OperationCost::default();
        let mut iterator = KVIterator {
            raw_iter,
            _query: query,
            left_to_right: query.left_to_right,
            current_query_item: None,
            query_iterator: query.directional_iter(query.left_to_right),
        };
        iterator.seek().unwrap_add_cost(&mut cost);
        iterator.wrap_with_cost(cost)
    }

    /// Returns the current node the iter points to if it's valid for the given
    /// query item returns None otherwise
    fn get_kv(&mut self, query_item: &QueryItem) -> CostContext<Option<(Vec<u8>, Vec<u8>)>> {
        let mut cost = OperationCost::default();

        if query_item
            .iter_is_valid_for_type(&self.raw_iter, None, self.left_to_right)
            .unwrap_add_cost(&mut cost)
        {
            let kv = (
                self.raw_iter
                    .key()
                    .unwrap_add_cost(&mut cost)
                    .expect("key must exist as iter is valid")
                    .to_vec(),
                self.raw_iter
                    .value()
                    .unwrap_add_cost(&mut cost)
                    .expect("value must exists as iter is valid")
                    .to_vec(),
            );
            if self.left_to_right {
                self.raw_iter.next().unwrap_add_cost(&mut cost)
            } else {
                self.raw_iter.prev().unwrap_add_cost(&mut cost)
            }
            Some(kv).wrap_with_cost(cost)
        } else {
            None.wrap_with_cost(cost)
        }
    }

    /// Moves the iter to the start of the next query item
    fn seek(&mut self) -> CostContext<()> {
        let mut cost = OperationCost::default();

        self.current_query_item = self.query_iterator.next();
        if let Some(query_item) = self.current_query_item {
            query_item
                .seek_for_iter(&mut self.raw_iter, self.left_to_right)
                .unwrap_add_cost(&mut cost);
        }

        ().wrap_with_cost(cost)
    }
}

// Cannot be an Iterator as it should return cost
impl<'a, I: RawIterator> KVIterator<'a, I> {
    pub fn next(&mut self) -> CostContext<Option<(Vec<u8>, Vec<u8>)>> {
        let mut cost = OperationCost::default();

        if let Some(query_item) = self.current_query_item {
            let kv_pair = self.get_kv(query_item).unwrap_add_cost(&mut cost);

            if kv_pair.is_some() {
                kv_pair.wrap_with_cost(cost)
            } else {
                self.seek().unwrap_add_cost(&mut cost);
                self.next().add_cost(cost)
            }
        } else {
            None.wrap_with_cost(cost)
        }
    }
}

/// A handle to a Merkle key/value store backed by RocksDB.
pub struct Merk<S> {
    pub(crate) tree: Cell<Option<Tree>>,
    pub storage: S,
}

impl<S> fmt::Debug for Merk<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Merk").finish()
    }
}

pub type UseTreeMutResult = CostContext<Result<Vec<(Vec<u8>, Option<Vec<u8>>)>>>;

impl<'db, S> Merk<S>
where
    S: StorageContext<'db>,
    <S as StorageContext<'db>>::Error: std::error::Error,
{
    pub fn open(storage: S) -> CostContext<Result<Self>> {
        let mut merk = Self {
            tree: Cell::new(None),
            storage,
        };

        merk.load_root().map_ok(|_| merk)
    }

    /// Deletes tree data
    pub fn clear(&mut self) -> CostContext<Result<()>> {
        let mut cost = OperationCost::default();

        let mut iter = self.storage.raw_iter();
        iter.seek_to_first().unwrap_add_cost(&mut cost);

        let mut to_delete = self.storage.new_batch();
        while iter.valid().unwrap_add_cost(&mut cost) {
            if let Some(key) = iter.key().unwrap_add_cost(&mut cost) {
                to_delete.delete(key);
            }
            iter.next().unwrap_add_cost(&mut cost);
        }
        cost_return_on_error!(
            &mut cost,
            self.storage.commit_batch(to_delete).map_err(|e| e.into())
        );
        self.tree.set(None);
        Ok(()).wrap_with_cost(cost)
    }

    /// Gets an auxiliary value.
    pub fn get_aux(&self, key: &[u8]) -> CostContext<Result<Option<Vec<u8>>>> {
        self.storage.get_aux(key).map_err(|e| e.into())
    }

    /// Returns if the value at the given key exists
    ///
    /// Note that this is essentially the same as a normal RocksDB `get`, so
    /// should be a fast operation and has almost no tree overhead.
    pub fn exists(&self, key: &[u8]) -> CostContext<Result<bool>> {
        self.has_node(key)
    }

    /// Gets a value for the given key. If the key is not found, `None` is
    /// returned.
    ///
    /// Note that this is essentially the same as a normal RocksDB `get`, so
    /// should be a fast operation and has almost no tree overhead.
    pub fn get(&self, key: &[u8]) -> CostContext<Result<Option<Vec<u8>>>> {
        self.get_node_fn(key, |node| {
            node.value().to_vec().wrap_with_cost(Default::default())
        })
    }

    /// Gets a hash of a node by a given key, `None` is returned in case
    /// when node not found by the key.
    pub fn get_hash(&self, key: &[u8]) -> CostContext<Result<Option<[u8; 32]>>> {
        self.get_node_fn(key, |node| node.hash())
    }

    /// See if a node's field exists
    fn has_node(&self, key: &[u8]) -> CostContext<Result<bool>> {
        self.use_tree(move |maybe_tree| {
            let mut cursor = match maybe_tree {
                None => return Ok(false).wrap_with_cost(Default::default()), // empty tree
                Some(tree) => tree,
            };

            loop {
                if key == cursor.key() {
                    return Ok(true).wrap_with_cost(OperationCost::default());
                }

                let left = key < cursor.key();
                let link = match cursor.link(left) {
                    None => return Ok(false).wrap_with_cost(Default::default()), // not found
                    Some(link) => link,
                };

                let maybe_child = link.tree();
                match maybe_child {
                    None => {
                        // fetch from RocksDB
                        break Tree::get(&self.storage, key).map_ok(|x| x.is_some());
                    }
                    Some(child) => cursor = child, // traverse to child
                }
            }
        })
    }

    /// Generic way to get a node's field
    fn get_node_fn<T, F>(&self, key: &[u8], f: F) -> CostContext<Result<Option<T>>>
    where
        F: FnOnce(&Tree) -> CostContext<T>,
    {
        self.use_tree(move |maybe_tree| {
            let mut cursor = match maybe_tree {
                None => return Ok(None).wrap_with_cost(Default::default()), // empty tree
                Some(tree) => tree,
            };

            loop {
                if key == cursor.key() {
                    return f(cursor).map(|x| Ok(Some(x)));
                }

                let left = key < cursor.key();
                let link = match cursor.link(left) {
                    None => return Ok(None).wrap_with_cost(Default::default()), // not found
                    Some(link) => link,
                };

                let maybe_child = link.tree();
                match maybe_child {
                    None => {
                        // fetch from RocksDB
                        break Tree::get(&self.storage, key).flat_map_ok(|maybe_node| {
                            let mut cost = OperationCost::default();
                            Ok(maybe_node.map(|node| f(&node).unwrap_add_cost(&mut cost)))
                                .wrap_with_cost(cost)
                        });
                    }
                    Some(child) => cursor = child, // traverse to child
                }
            }
        })
    }

    /// Returns the root hash of the tree (a digest for the entire store which
    /// proofs can be checked against). If the tree is empty, returns the null
    /// hash (zero-filled).
    pub fn root_hash(&self) -> CostContext<Hash> {
        self.use_tree(|tree| {
            tree.map_or(NULL_HASH.wrap_with_cost(Default::default()), |tree| {
                tree.hash()
            })
        })
    }

    /// Applies a batch of operations (puts and deletes) to the tree.
    ///
    /// This will fail if the keys in `batch` are not sorted and unique. This
    /// check creates some overhead, so if you are sure your batch is sorted and
    /// unique you can use the unsafe `apply_unchecked` for a small performance
    /// gain.
    ///
    /// # Example
    /// ```
    /// # let mut store = merk::test_utils::TempMerk::new();
    /// # store.apply::<_, Vec<_>>(&[(vec![4,5,6], Op::Put(vec![0]))], &[]).unwrap();
    ///
    /// use merk::Op;
    ///
    /// let batch = &[
    ///     (vec![1, 2, 3], Op::Put(vec![4, 5, 6])), // puts value [4,5,6] to key[1,2,3]
    ///     (vec![4, 5, 6], Op::Delete),             // deletes key [4,5,6]
    /// ];
    /// store.apply::<_, Vec<_>>(batch, &[]).unwrap();
    /// ```
    pub fn apply<KB, KA>(
        &mut self,
        batch: &MerkBatch<KB>,
        aux: &MerkBatch<KA>,
    ) -> CostContext<Result<()>>
    where
        KB: AsRef<[u8]>,
        KA: AsRef<[u8]>,
    {
        // ensure keys in batch are sorted and unique
        let mut maybe_prev_key: Option<&KB> = None;
        for (key, _) in batch.iter() {
            if let Some(prev_key) = maybe_prev_key {
                match prev_key.as_ref().cmp(key.as_ref()) {
                    Ordering::Greater => {
                        return Err(anyhow!("Keys in batch must be sorted"))
                            .wrap_with_cost(Default::default())
                    }
                    Ordering::Equal => {
                        return Err(anyhow!("Keys in batch must be unique"))
                            .wrap_with_cost(Default::default())
                    }
                    _ => (),
                }
            }
            maybe_prev_key = Some(key);
        }

        unsafe { self.apply_unchecked(batch, aux) }
    }

    /// Applies a batch of operations (puts and deletes) to the tree.
    ///
    /// # Safety
    /// This is unsafe because the keys in `batch` must be sorted and unique -
    /// if they are not, there will be undefined behavior. For a safe version of
    /// this method which checks to ensure the batch is sorted and unique, see
    /// `apply`.
    ///
    /// # Example
    /// ```
    /// # let mut store = merk::test_utils::TempMerk::new();
    /// # store.apply::<_, Vec<_>>(&[(vec![4,5,6], Op::Put(vec![0]))], &[]).unwrap();
    ///
    /// use merk::Op;
    ///
    /// let batch = &[
    ///     (vec![1, 2, 3], Op::Put(vec![4, 5, 6])), // puts value [4,5,6] to key [1,2,3]
    ///     (vec![4, 5, 6], Op::Delete),             // deletes key [4,5,6]
    /// ];
    /// unsafe { store.apply_unchecked::<_, Vec<_>>(batch, &[]).unwrap() };
    /// ```
    pub unsafe fn apply_unchecked<KB, KA>(
        &mut self,
        batch: &MerkBatch<KB>,
        aux: &MerkBatch<KA>,
    ) -> CostContext<Result<()>>
    where
        KB: AsRef<[u8]>,
        KA: AsRef<[u8]>,
    {
        let maybe_walker = self
            .tree
            .take()
            .take()
            .map(|tree| Walker::new(tree, self.source()));

        Walker::apply_to(maybe_walker, batch, self.source()).flat_map_ok(
            |(maybe_tree, deleted_keys)| {
                self.tree.set(maybe_tree);
                // commit changes to db
                self.commit(deleted_keys, aux)
            },
        )
    }

    /// Creates a Merkle proof for the list of queried keys. For each key in the
    /// query, if the key is found in the store then the value will be proven to
    /// be in the tree. For each key in the query that does not exist in the
    /// tree, its absence will be proven by including boundary keys.
    ///
    /// The proof returned is in an encoded format which can be verified with
    /// `merk::verify`.
    ///
    /// This will fail if the keys in `query` are not sorted and unique. This
    /// check adds some overhead, so if you are sure your batch is sorted and
    /// unique you can use the unsafe `prove_unchecked` for a small performance
    /// gain.
    pub fn prove(
        &self,
        query: Query,
        limit: Option<u16>,
        offset: Option<u16>,
    ) -> CostContext<Result<ProofConstructionResult>> {
        let left_to_right = query.left_to_right;
        self.prove_unchecked(query, limit, offset, left_to_right)
            .map_ok(|(proof, limit, offset)| {
                let mut bytes = Vec::with_capacity(128);
                encode_into(proof.iter(), &mut bytes);
                ProofConstructionResult::new(bytes, limit, offset)
            })
    }

    /// Creates a Merkle proof for the list of queried keys. For each key in the
    /// query, if the key is found in the store then the value will be proven to
    /// be in the tree. For each key in the query that does not exist in the
    /// tree, its absence will be proven by including boundary keys.
    ///
    /// The proof returned is in an intermediate format to be later encoded
    ///
    /// This will fail if the keys in `query` are not sorted and unique. This
    /// check adds some overhead, so if you are sure your batch is sorted and
    /// unique you can use the unsafe `prove_unchecked` for a small performance
    /// gain.
    pub fn prove_without_encoding(
        &self,
        query: Query,
        limit: Option<u16>,
        offset: Option<u16>,
    ) -> CostContext<Result<ProofWithoutEncodingResult>> {
        let left_to_right = query.left_to_right;
        self.prove_unchecked(query, limit, offset, left_to_right)
            .map_ok(|(proof, limit, offset)| ProofWithoutEncodingResult::new(proof, limit, offset))
    }

    /// Creates a Merkle proof for the list of queried keys. For each key in
    /// the query, if the key is found in the store then the value will be
    /// proven to be in the tree. For each key in the query that does not
    /// exist in the tree, its absence will be proven by including
    /// boundary keys.
    /// The proof returned is in an encoded format which can be verified with
    /// `merk::verify`.
    ///
    /// This is unsafe because the keys in `query` must be sorted and unique -
    /// if they are not, there will be undefined behavior. For a safe version
    /// of this method which checks to ensure the batch is sorted and
    /// unique, see `prove`.
    pub fn prove_unchecked<Q, I>(
        &self,
        query: I,
        limit: Option<u16>,
        offset: Option<u16>,
        left_to_right: bool,
    ) -> CostContext<Result<Proof>>
    where
        Q: Into<QueryItem>,
        I: IntoIterator<Item = Q>,
    {
        let query_vec: Vec<QueryItem> = query.into_iter().map(Into::into).collect();

        self.use_tree_mut(|maybe_tree| {
            maybe_tree
                .ok_or_else(|| anyhow!("Cannot create proof for empty tree"))
                .wrap_with_cost(Default::default())
                .flat_map_ok(|tree| {
                    let mut ref_walker = RefWalker::new(tree, self.source());
                    ref_walker.create_proof(query_vec.as_slice(), limit, offset, left_to_right)
                })
                .map_ok(|(proof, _, limit, offset, ..)| (proof, limit, offset))
        })
    }

    pub fn commit<K>(
        &mut self,
        deleted_keys: LinkedList<Vec<u8>>,
        aux: &MerkBatch<K>,
    ) -> CostContext<Result<()>>
    where
        K: AsRef<[u8]>,
    {
        let mut cost = OperationCost::default();

        let mut batch = self.storage.new_batch();
        let to_batch_wrapped = self.use_tree_mut(|maybe_tree| -> UseTreeMutResult {
            // TODO: concurrent commit
            let mut inner_cost = OperationCost::default();

            if let Some(tree) = maybe_tree {
                // TODO: configurable committer
                let mut committer = MerkCommitter::new(tree.height(), 100);
                cost_return_on_error!(&mut inner_cost, tree.commit(&mut committer));
                // update pointer to root node
                batch.put_root(ROOT_KEY_KEY, tree.key());

                Ok(committer.batch)
            } else {
                // empty tree, delete pointer to root
                batch.delete_root(ROOT_KEY_KEY);

                Ok(vec![])
            }
            .wrap_with_cost(inner_cost)
        });

        let mut to_batch = cost_return_on_error!(&mut cost, to_batch_wrapped);

        // TODO: move this to MerkCommitter impl?
        for key in deleted_keys {
            to_batch.push((key, None));
        }
        to_batch.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, maybe_value) in to_batch {
            if let Some(value) = maybe_value {
                batch.put(&key, &value);
            } else {
                batch.delete(&key);
            }
        }

        for (key, value) in aux {
            match value {
                Op::Put(value) => batch.put_aux(key, value),
                Op::PutReference(value, _) => batch.put_aux(key, value),
                Op::Delete => batch.delete_aux(key),
            };
        }

        // write to db
        self.storage
            .commit_batch(batch)
            .map_err(|e| e.into())
            .add_cost(cost)
    }

    pub fn walk<'s, T>(&'s self, f: impl FnOnce(Option<RefWalker<MerkSource<'s, S>>>) -> T) -> T {
        let mut tree = self.tree.take();
        let maybe_walker = tree
            .as_mut()
            .map(|tree| RefWalker::new(tree, self.source()));
        let res = f(maybe_walker);
        self.tree.set(tree);
        res
    }

    pub fn is_empty_tree(&self) -> CostContext<bool> {
        let mut iter = self.storage.raw_iter();
        iter.seek_to_first().flat_map(|_| iter.valid().map(|x| !x))
    }

    pub fn is_empty_tree_except(&self, mut except_keys: BTreeSet<&[u8]>) -> CostContext<bool> {
        let mut cost = OperationCost::default();

        let mut iter = self.storage.raw_iter();
        iter.seek_to_first().unwrap_add_cost(&mut cost);
        while let Some(key) = iter.key().unwrap_add_cost(&mut cost) {
            if except_keys.take(key).is_none() {
                return false.wrap_with_cost(cost);
            }
            iter.next().unwrap_add_cost(&mut cost)
        }
        true.wrap_with_cost(cost)
    }

    fn source(&self) -> MerkSource<S> {
        MerkSource {
            storage: &self.storage,
        }
    }

    fn use_tree<T>(&self, f: impl FnOnce(Option<&Tree>) -> T) -> T {
        let tree = self.tree.take();
        let res = f(tree.as_ref());
        self.tree.set(tree);
        res
    }

    fn use_tree_mut<T>(&self, mut f: impl FnMut(Option<&mut Tree>) -> T) -> T {
        let mut tree = self.tree.take();
        let res = f(tree.as_mut());
        self.tree.set(tree);
        res
    }

    // pub(crate) fn set_root_key(&mut self, key: &[u8]) -> Result<()> {
    //     Ok(self.storage.put_root(ROOT_KEY_KEY, key)?)
    // }

    pub(crate) fn load_root(&mut self) -> CostContext<Result<()>> {
        self.storage
            .get_root(ROOT_KEY_KEY)
            .map(|root_result| root_result.map_err(|e| anyhow!(e)))
            .flat_map_ok(|tree_root_key_opt| {
                // In case of successful seek for root key check if it exists
                if let Some(tree_root_key) = tree_root_key_opt {
                    // Trying to build a tree out of it, costs will be accumulated because
                    // `Tree::get` returns `CostContext` and this call happens inside `flat_map_ok`.
                    Tree::get(&self.storage, &tree_root_key).map_ok(|tree| {
                        self.tree = Cell::new(tree);
                    })
                } else {
                    Ok(()).wrap_with_cost(Default::default())
                }
            })
    }
}

// impl Clone for Merk<S> {
//     fn clone(&self) -> Self {
//         let tree_clone = match self.tree.take() {
//             None => None,
//             Some(tree) => {
//                 let clone = tree.clone();
//                 self.tree.set(Some(tree));
//                 Some(clone)
//             }
//         };
//         Self {
//             tree: Cell::new(tree_clone),
//             storage: self.storage.clone(),
//         }
//     }
// }

// // TODO: get rid of Fetch/source and use GroveDB storage abstraction
#[derive(Debug)]
pub struct MerkSource<'s, S> {
    storage: &'s S,
}

impl<'s, S> Clone for MerkSource<'s, S> {
    fn clone(&self) -> Self {
        MerkSource {
            storage: self.storage,
        }
    }
}

impl<'s, 'db, S> Fetch for MerkSource<'s, S>
where
    S: StorageContext<'db>,
{
    fn fetch(&self, link: &Link) -> CostContext<Result<Tree>> {
        Tree::get(self.storage, link.key())
            .map_ok(|x| x.ok_or_else(|| anyhow!("Key not found")))
            .flatten()
    }
}

struct MerkCommitter {
    batch: Vec<(Vec<u8>, Option<Vec<u8>>)>,
    height: u8,
    levels: u8,
}

impl MerkCommitter {
    fn new(height: u8, levels: u8) -> Self {
        Self {
            batch: Vec::with_capacity(10000),
            height,
            levels,
        }
    }
}

impl Commit for MerkCommitter {
    fn write(&mut self, tree: &Tree) -> Result<()> {
        let mut buf = Vec::with_capacity(tree.encoding_length());
        // dbg!(&tree);
        tree.encode_into(&mut buf);
        // dbg!(buf.len());
        self.batch.push((tree.key().to_vec(), Some(buf)));
        Ok(())
    }

    fn prune(&self, tree: &Tree) -> (bool, bool) {
        // keep N top levels of tree
        let prune = (self.height - tree.height()) >= self.levels;
        (prune, prune)
    }
}

#[cfg(test)]
mod test {
    use std::iter::empty;

    use costs::OperationCost;
    use storage::{
        rocksdb_storage::{PrefixedRocksDbStorageContext, RocksDbStorage},
        RawIterator, Storage, StorageContext,
    };
    use tempfile::TempDir;

    use super::{Merk, MerkSource, RefWalker};
    use crate::{test_utils::*, Op};

    // TODO: Close and then reopen test

    fn assert_invariants(merk: &TempMerk) {
        merk.use_tree(|maybe_tree| {
            let tree = maybe_tree.expect("expected tree");
            assert_tree_invariants(tree);
        })
    }

    #[test]
    fn test_reopen_root_hash() {
        let tmp_dir = TempDir::new().expect("cannot open tempdir");
        let storage = RocksDbStorage::default_rocksdb_with_path(tmp_dir.path())
            .expect("cannot open rocksdb storage");
        let test_prefix = [b"ayy"].into_iter().map(|x| x.as_slice());
        let mut merk = Merk::open(storage.get_storage_context(test_prefix.clone()).unwrap())
            .unwrap()
            .unwrap();

        merk.apply::<_, Vec<_>>(&[(vec![1, 2, 3], Op::Put(vec![4, 5, 6]))], &[])
            .unwrap()
            .expect("apply failed");

        let root_hash = merk.root_hash();
        drop(merk);
        let merk = Merk::open(storage.get_storage_context(test_prefix).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(merk.root_hash(), root_hash);
    }

    #[test]
    fn test_open_fee() {
        let tmp_dir = TempDir::new().expect("cannot open tempdir");
        let storage = RocksDbStorage::default_rocksdb_with_path(tmp_dir.path())
            .expect("cannot open rocksdb storage");
        let test_prefix = [b"ayy"].into_iter().map(|x| x.as_slice());
        let merk_fee_context =
            Merk::open(storage.get_storage_context(test_prefix.clone()).unwrap());

        // Opening not existing merk should cost only root key seek (except context
        // creation)
        assert!(matches!(
            merk_fee_context.cost(),
            OperationCost { seek_count: 1, .. }
        ));

        let mut merk = merk_fee_context.unwrap().unwrap();
        merk.apply::<_, Vec<_>>(&[(vec![1, 2, 3], Op::Put(vec![4, 5, 6]))], &[])
            .unwrap()
            .expect("apply failed");

        drop(merk);

        let merk_fee_context = Merk::open(storage.get_storage_context(test_prefix).unwrap());

        // Opening existing merk should cost two seeks. (except context creation)
        assert!(matches!(
            merk_fee_context.cost(),
            OperationCost { seek_count: 2, .. }
        ));
        assert!(merk_fee_context.cost().storage_loaded_bytes > 0);
    }

    #[test]
    fn simple_insert_apply() {
        let batch_size = 20;
        let mut merk = TempMerk::new();
        let batch = make_batch_seq(0..batch_size);
        merk.apply::<_, Vec<_>>(&batch, &[])
            .unwrap()
            .expect("apply failed");

        assert_invariants(&merk);
        assert_eq!(
            merk.root_hash().unwrap(),
            [
                126, 168, 96, 201, 59, 225, 123, 33, 206, 154, 87, 23, 139, 143, 136, 52, 103, 9,
                218, 90, 71, 153, 240, 47, 227, 168, 1, 104, 239, 237, 140, 147
            ]
        );
    }

    #[test]
    fn insert_uncached() {
        let batch_size = 20;
        let mut merk = TempMerk::new();

        let batch = make_batch_seq(0..batch_size);
        merk.apply::<_, Vec<_>>(&batch, &[])
            .unwrap()
            .expect("apply failed");
        assert_invariants(&merk);

        let batch = make_batch_seq(batch_size..(batch_size * 2));
        merk.apply::<_, Vec<_>>(&batch, &[])
            .unwrap()
            .expect("apply failed");
        assert_invariants(&merk);
    }

    #[test]
    fn test_has_node_with_empty_tree() {
        let mut merk = TempMerk::new();

        let key = b"something";

        let result = merk.has_node(key).unwrap().unwrap();

        assert!(!result);

        let batch_entry = (key, Op::Put(vec![123; 60]));

        let batch = vec![batch_entry];

        merk.apply::<_, Vec<_>>(&batch, &[])
            .unwrap()
            .expect("should ...");

        let result = merk.has_node(key).unwrap().unwrap();

        assert!(result);
    }

    #[test]
    fn insert_rand() {
        let tree_size = 40;
        let batch_size = 4;
        let mut merk = TempMerk::new();

        for i in 0..(tree_size / batch_size) {
            println!("i:{}", i);
            let batch = make_batch_rand(batch_size, i);
            merk.apply::<_, Vec<_>>(&batch, &[])
                .unwrap()
                .expect("apply failed");
        }
    }

    #[test]
    fn actual_deletes() {
        let mut merk = TempMerk::new();

        let batch = make_batch_rand(10, 1);
        merk.apply::<_, Vec<_>>(&batch, &[])
            .unwrap()
            .expect("apply failed");

        let key = batch.first().unwrap().0.clone();
        merk.apply::<_, Vec<_>>(&[(key.clone(), Op::Delete)], &[])
            .unwrap()
            .unwrap();

        let value = merk.storage.get(key.as_slice()).unwrap().unwrap();
        assert!(value.is_none());
    }

    #[test]
    fn aux_data() {
        let mut merk = TempMerk::new();
        merk.apply::<Vec<_>, _>(&[], &[(vec![1, 2, 3], Op::Put(vec![4, 5, 6]))])
            .unwrap()
            .expect("apply failed");
        let val = merk.get_aux(&[1, 2, 3]).unwrap().unwrap();
        assert_eq!(val, Some(vec![4, 5, 6]));
    }

    #[test]
    fn simulated_crash() {
        let mut merk = CrashMerk::open().expect("failed to open merk");

        merk.apply::<_, Vec<_>>(
            &[(vec![0], Op::Put(vec![1]))],
            &[(vec![2], Op::Put(vec![3]))],
        )
        .unwrap()
        .expect("apply failed");

        // make enough changes so that main column family gets auto-flushed
        for i in 0..250 {
            merk.apply::<_, Vec<_>>(&make_batch_seq(i * 2_000..(i + 1) * 2_000), &[])
                .unwrap()
                .expect("apply failed");
        }
        merk.crash();

        assert_eq!(merk.get_aux(&[2]).unwrap().unwrap(), Some(vec![3]));
    }

    #[test]
    fn get_not_found() {
        let mut merk = TempMerk::new();

        // no root
        assert!(merk.get(&[1, 2, 3]).unwrap().unwrap().is_none());

        // cached
        merk.apply::<_, Vec<_>>(&[(vec![5, 5, 5], Op::Put(vec![]))], &[])
            .unwrap()
            .unwrap();
        assert!(merk.get(&[1, 2, 3]).unwrap().unwrap().is_none());

        // uncached
        merk.apply::<_, Vec<_>>(
            &[
                (vec![0, 0, 0], Op::Put(vec![])),
                (vec![1, 1, 1], Op::Put(vec![])),
                (vec![2, 2, 2], Op::Put(vec![])),
            ],
            &[],
        )
        .unwrap()
        .unwrap();
        assert!(merk.get(&[3, 3, 3]).unwrap().unwrap().is_none());
    }

    #[test]
    fn reopen_check_root_hash() {
        let tmp_dir = TempDir::new().expect("cannot open tempdir");
        let storage = RocksDbStorage::default_rocksdb_with_path(tmp_dir.path())
            .expect("cannot open rocksdb storage");
        let mut merk = Merk::open(storage.get_storage_context(empty()).unwrap())
            .unwrap()
            .expect("cannot open merk");
        let batch = make_batch_seq(1..10);
        merk.apply::<_, Vec<_>>(batch.as_slice(), &[])
            .unwrap()
            .unwrap();
        let batch = make_batch_seq(11..12);
        merk.apply::<_, Vec<_>>(batch.as_slice(), &[])
            .unwrap()
            .unwrap();
    }

    #[test]
    fn test_get_node_cost() {
        let tmp_dir = TempDir::new().expect("cannot open tempdir");
        let storage = RocksDbStorage::default_rocksdb_with_path(tmp_dir.path())
            .expect("cannot open rocksdb storage");
        let mut merk = Merk::open(storage.get_storage_context(empty()).unwrap())
            .unwrap()
            .expect("cannot open merk");
        let batch = make_batch_seq(1..10);
        merk.apply::<_, Vec<_>>(batch.as_slice(), &[])
            .unwrap()
            .unwrap();
        drop(merk);

        let mut merk = Merk::open(storage.get_storage_context(empty()).unwrap())
            .unwrap()
            .expect("cannot open merk");
        let m = merk.get(&9_u64.to_be_bytes());
        let m = merk.get(&8_u64.to_be_bytes());
        // dbg!(m);
    }

    #[test]
    fn reopen() {
        fn collect(
            mut node: RefWalker<MerkSource<PrefixedRocksDbStorageContext>>,
            nodes: &mut Vec<Vec<u8>>,
        ) {
            nodes.push(node.tree().encode());
            if let Some(c) = node.walk(true).unwrap().unwrap() {
                collect(c, nodes);
            }
            if let Some(c) = node.walk(false).unwrap().unwrap() {
                collect(c, nodes);
            }
        }

        let tmp_dir = TempDir::new().expect("cannot open tempdir");

        let original_nodes = {
            let storage = RocksDbStorage::default_rocksdb_with_path(tmp_dir.path())
                .expect("cannot open rocksdb storage");
            let mut merk = Merk::open(storage.get_storage_context(empty()).unwrap())
                .unwrap()
                .expect("cannot open merk");
            let batch = make_batch_seq(1..10_000);
            merk.apply::<_, Vec<_>>(batch.as_slice(), &[])
                .unwrap()
                .unwrap();
            let mut tree = merk.tree.take().unwrap();
            let walker = RefWalker::new(&mut tree, merk.source());

            let mut nodes = vec![];
            collect(walker, &mut nodes);
            nodes
        };

        let storage = RocksDbStorage::default_rocksdb_with_path(tmp_dir.path())
            .expect("cannot open rocksdb storage");
        let merk = Merk::open(storage.get_storage_context(empty()).unwrap())
            .unwrap()
            .expect("cannot open merk");
        let mut tree = merk.tree.take().unwrap();
        let walker = RefWalker::new(&mut tree, merk.source());

        let mut reopen_nodes = vec![];
        collect(walker, &mut reopen_nodes);

        assert_eq!(reopen_nodes, original_nodes);
    }

    type PrefixedStorageIter<'db, 'ctx> =
        &'ctx mut <PrefixedRocksDbStorageContext<'db> as StorageContext<'db>>::RawIterator;

    #[test]
    fn reopen_iter() {
        fn collect<'db, 'ctx>(
            iter: PrefixedStorageIter<'db, 'ctx>,
            nodes: &mut Vec<(Vec<u8>, Vec<u8>)>,
        ) {
            while iter.valid().unwrap() {
                nodes.push((
                    iter.key().unwrap().unwrap().to_vec(),
                    iter.value().unwrap().unwrap().to_vec(),
                ));
                iter.next();
            }
        }
        let tmp_dir = TempDir::new().expect("cannot open tempdir");

        let original_nodes = {
            let storage = RocksDbStorage::default_rocksdb_with_path(tmp_dir.path())
                .expect("cannot open rocksdb storage");
            let mut merk = Merk::open(storage.get_storage_context(empty()).unwrap())
                .unwrap()
                .expect("cannot open merk");
            let batch = make_batch_seq(1..10_000);
            merk.apply::<_, Vec<_>>(batch.as_slice(), &[])
                .unwrap()
                .unwrap();

            let mut nodes = vec![];
            collect(&mut merk.storage.raw_iter(), &mut nodes);
            nodes
        };
        let storage = RocksDbStorage::default_rocksdb_with_path(tmp_dir.path())
            .expect("cannot open rocksdb storage");
        let merk = Merk::open(storage.get_storage_context(empty()).unwrap())
            .unwrap()
            .expect("cannot open merk");

        let mut reopen_nodes = vec![];
        collect(&mut merk.storage.raw_iter(), &mut reopen_nodes);

        assert_eq!(reopen_nodes, original_nodes);
    }
}
