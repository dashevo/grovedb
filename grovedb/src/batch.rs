//! GroveDB batch operations support

use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap, HashSet},
    hash::Hash,
};

use intrusive_collections::{intrusive_adapter, Bound, KeyAdapter, RBTree, RBTreeLink};
use merk::Merk;
use storage::{Storage, StorageBatch, StorageContext};
use visualize::{Drawer, Visualize};

use crate::{Element, Error, GroveDb, TransactionArg, ROOT_LEAFS_SERIALIZED_KEY};

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
enum Op {
    Insert { element: Element },
    Delete,
}

impl PartialOrd for Op {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Op::Delete, Op::Insert { .. }) => Some(Ordering::Less),
            (Op::Insert { .. }, Op::Delete) => Some(Ordering::Greater),
            _ => Some(Ordering::Equal),
        }
    }
}

impl Ord for Op {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).expect("all ops have order")
    }
}

/// Batch operation
#[derive(Clone)]
pub struct GroveDbOp {
    /// Path to a subtree - subject to an operation
    path: Vec<Vec<u8>>,
    /// Key of an element in the subtree
    key: Vec<u8>,
    /// Operation to perform on the key
    op: Op,
    /// Link used in intrusive tree to maintain operations order
    link: RBTreeLink,
}

impl PartialEq for GroveDbOp {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.key == other.key && self.op == other.op
    }
}

impl std::fmt::Debug for GroveDbOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut path_out = Vec::new();
        let mut path_drawer = Drawer::new(&mut path_out);
        for p in &self.path {
            path_drawer = p.visualize(path_drawer).unwrap();
            path_drawer.write(b" ").unwrap();
        }
        let mut key_out = Vec::new();
        let key_drawer = Drawer::new(&mut key_out);
        self.key.visualize(key_drawer).unwrap();

        let op_dbg = match self.op {
            Op::Insert {
                element: Element::Tree(..),
            } => "Insert tree",
            Op::Insert { .. } => "Insert",
            Op::Delete => "Delete",
        };

        f.debug_struct("GroveDbOp")
            .field("path", &String::from_utf8_lossy(&path_out))
            .field("key", &String::from_utf8_lossy(&key_out))
            .field("op", &op_dbg)
            .finish()
    }
}

/// Wrapper struct to put shallow subtrees first
#[derive(Debug, Eq, PartialEq)]
struct PathWrapper<'a>(&'a [Vec<u8>]);

impl<'a> PartialOrd for PathWrapper<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let l = self.0.len().partial_cmp(&other.0.len());
        match l {
            Some(Ordering::Equal) => self.0.partial_cmp(other.0),
            _ => l,
        }
    }
}

impl<'a> Ord for PathWrapper<'a> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other)
            .expect("paths are always comparable")
    }
}

// TODO: keep allocation number small
intrusive_adapter!(GroveDbOpAdapter = Box<GroveDbOp> : GroveDbOp { link: RBTreeLink });

impl<'a> KeyAdapter<'a> for GroveDbOpAdapter {
    type Key = (PathWrapper<'a>, &'a [u8], &'a Op);

    fn get_key(&self, value: &'a GroveDbOp) -> Self::Key {
        (PathWrapper(&value.path), &value.key, &value.op)
    }
}

/// Helper function to keep RBTree values unique on insertions.
fn insert_unique_op(ops: &mut RBTree<GroveDbOpAdapter>, op: Box<GroveDbOp>) {
    let mut cursor =
        ops.lower_bound_mut(Bound::Included(&(PathWrapper(&op.path), &op.key, &op.op)));

    match cursor.get() {
        Some(found_op) if found_op == op.as_ref() => {
            // skip as already inserted
        }
        Some(GroveDbOp {
            path,
            key,
            op: Op::Insert { .. },
            ..
        }) if path == &op.path && key == &op.key => {
            // not found but there is an insertion operation for that
            // path/key which substitutes deletion
        }
        _ => {
            // TODO: possibly unnecessary clone https://github.com/Amanieu/intrusive-rs/issues/70
            cursor.insert_before(op.clone());
        }
    }
}

impl GroveDbOp {
    pub fn insert(path: Vec<Vec<u8>>, key: Vec<u8>, element: Element) -> Self {
        Self {
            path,
            key,
            op: Op::Insert { element },
            link: RBTreeLink::new(),
        }
    }

    pub fn delete(path: Vec<Vec<u8>>, key: Vec<u8>) -> Self {
        Self {
            path,
            key,
            op: Op::Delete,
            link: RBTreeLink::new(),
        }
    }
}

impl GroveDb {
    /// Batch application generic over storage context (whether there is a
    /// transaction or not).
    fn apply_body<'db, S: StorageContext<'db>>(
        &self,
        sorted_operations: &mut RBTree<GroveDbOpAdapter>,
        temp_root_leaves: &mut BTreeMap<Vec<u8>, usize>,
        get_merk_fn: impl Fn(&[Vec<u8>]) -> Result<Merk<S>, Error>,
    ) -> Result<(), Error> {
        let mut temp_subtrees: HashMap<Vec<Vec<u8>>, Merk<_>> = HashMap::new();
        let mut cursor = sorted_operations.back_mut();
        let mut prev_path = cursor.get().expect("batch is not empty").path.clone();

        loop {
            // Run propagation if next operation is on different path or no more operations
            // left
            if cursor.get().map(|op| op.path != prev_path).unwrap_or(true) {
                if let Some((key, path_slice)) = prev_path.split_last() {
                    let hash = temp_subtrees
                        .remove(&prev_path)
                        .expect("subtree was inserted before")
                        .root_hash();

                    cursor.insert(Box::new(GroveDbOp::insert(
                        path_slice.to_vec(),
                        key.to_vec(),
                        Element::Tree(hash),
                    )));
                }
            }

            // Execute next available operation
            // TODO: investigate how not to create a new cursor each time
            cursor = sorted_operations.back_mut();
            if let Some(op) = cursor.remove() {
                if op.path.is_empty() {
                    // Altering root leaves
                    // We don't match operation here as only insertion is supported
                    if temp_root_leaves.get(&op.key).is_none() {
                        temp_root_leaves.insert(op.key, temp_root_leaves.len());
                    }
                } else {
                    // Keep opened Merk instances to accumulate changes before taking final root
                    // hash
                    let mut merk = temp_subtrees
                        .remove(&op.path)
                        .map(Ok)
                        .unwrap_or_else(|| get_merk_fn(&op.path))?;

                    // On subtree deletion/overwrite we need to do Merk's cleanup
                    match Element::get(&merk, &op.key) {
                        Ok(Element::Tree(_)) => {
                            let mut path = op.path.clone();
                            path.push(op.key.clone());
                            let mut sub = temp_subtrees
                                .remove(&path)
                                .map(Ok)
                                .unwrap_or_else(|| get_merk_fn(&path))?;
                            sub.clear()
                                .map_err(|_| Error::InternalError("cannot clear a Merk"))?;
                        }
                        Err(Error::PathKeyNotFound(_) | Error::PathNotFound(_)) => {
                            // TODO: the case when key is scheduled for deletion
                            // but cannot be found is weird and requires some
                            // investigation
                        }
                        e => {
                            e?;
                        }
                    }
                    match op.op {
                        Op::Insert { element } => {
                            element.insert(&mut merk, op.key)?;
                            temp_subtrees.insert(op.path.clone(), merk);
                        }
                        Op::Delete => {
                            Element::delete(&mut merk, op.key)?;
                            temp_subtrees.insert(op.path.clone(), merk);
                        }
                    }
                }
                prev_path = op.path;
            } else {
                break;
            }
        }
        Ok(())
    }

    /// Validates batch using a set of rules:
    /// 1. Subtree must exist to perform operations on it;
    /// 2. Subtree is treated as exising if it can be found in storage;
    /// 3. Subtree is treated as exising if it is created within the same batch;
    /// 4. Subtree is treated as not existing otherwise or if there is a delete
    ///    operation with no subtree insertion counterpart;
    /// 5. Subtree overwrite/deletion produces explicit delete operations for
    ///    every descendant subtree
    /// 6. Operations are unique
    fn validate_batch(
        &self,
        mut ops: RBTree<GroveDbOpAdapter>,
        root_leaves: &BTreeMap<Vec<u8>, usize>,
        transaction: TransactionArg,
    ) -> Result<RBTree<GroveDbOpAdapter>, Error> {
        // To ensure that batch `[insert([a, b], c, t), insert([a, b, c], k, v)]` is
        // valid we need to check that subtree `[a, b]` exists;
        // If we add `insert([a], b, t)` we need to check (query the DB) only `[a]`
        // subtree as all operations form a chain and we check only head to exist.
        //
        // `valid_subtrees` is used to cache check results for these chains
        let mut valid_subtrees: HashSet<Vec<Vec<u8>>> = HashSet::new();

        // An opposite to `valid_subtrees`, all overwritten and deleted subtrees are
        // cached there; This is required as data might be staged for deletion
        // and subtree will become invalid to insert to even if it exists in
        // pre-batch database state.
        let mut removed_subtrees: HashSet<Vec<Vec<u8>>> = HashSet::new();

        // First pass is required to expand recursive deletions and possible subtree
        // overwrites.
        let mut delete_ops = Vec::new();
        for op in ops.iter() {
            let delete_paths = self.find_subtrees(
                op.path
                    .iter()
                    .map(|x| x.as_slice())
                    .chain(std::iter::once(op.key.as_slice())),
                transaction,
            )?;
            delete_ops.extend(delete_paths.iter().map(|p| {
                let (key, path) = p.split_last().expect("no empty paths expected");
                Box::new(GroveDbOp::delete(path.to_vec(), key.to_vec()))
            }));
            for p in delete_paths {
                removed_subtrees.insert(p);
            }
        }
        for op in delete_ops {
            insert_unique_op(&mut ops, op);
        }

        // Insertion to root tree is valid as root tree always exists
        valid_subtrees.insert(Vec::new());

        // Validation goes from top to bottom so each operation will be in context of
        // what happened to ancestors of a subject subtree.
        for op in ops.iter() {
            let path: &[Vec<u8>] = &op.path;

            // Insertion into subtree that was deleted in this batch is invalid
            if matches!(op.op, Op::Insert { .. }) && removed_subtrees.contains(path) {
                return Err(Error::InvalidPath("attempt to insert into deleted subtree"));
            }

            // Attempt to subtrees cache to see if subtree exists or will exists within the
            // batch
            if !valid_subtrees.contains(path) {
                // Tree wasn't checked before and won't be inserted within the batch, need to
                // access pre-batch database state:
                if path.len() == 0 {
                    // We're working with root leaf subtree there
                    if !root_leaves.contains_key(&op.key) {
                        return Err(Error::PathNotFound("missing root leaf"));
                    }
                    if let Op::Delete = op.op {
                        return Err(Error::InvalidPath(
                            "deletion for root leafs is not supported",
                        ));
                    }
                } else {
                    // Dealing with a deeper subtree (not a root leaf so to say)
                    let (parent_key, parent_path) =
                        path.split_last().expect("empty path already checked");
                    let subtree = self.get(
                        parent_path.iter().map(|x| x.as_slice()),
                        parent_key,
                        transaction,
                    )?;
                    if !matches!(subtree, Element::Tree(_)) {
                        // There is an attempt to insert into a scalar
                        return Err(Error::InvalidPath("must be a tree"));
                    }
                }
            }

            match *op {
                // Insertion of a tree makes this subtree valid
                GroveDbOp {
                    ref path,
                    ref key,
                    op:
                        Op::Insert {
                            element: Element::Tree(_),
                        },
                    ..
                } => {
                    let mut new_path = path.to_vec();
                    new_path.push(key.to_vec());
                    removed_subtrees.remove(&new_path);
                    valid_subtrees.insert(new_path);
                }
                // Deletion of a tree makes a subtree unavailable
                GroveDbOp {
                    ref path,
                    ref key,
                    op: Op::Delete,
                    ..
                } => {
                    let mut new_path = path.to_vec();
                    new_path.push(key.to_vec());
                    valid_subtrees.remove(&new_path);
                    removed_subtrees.insert(new_path);
                }
                _ => {}
            }
        }

        Ok(ops)
    }

    /// Applies batch of operations on GroveDB
    pub fn apply_batch(
        &self,
        ops: Vec<GroveDbOp>,
        transaction: TransactionArg,
    ) -> Result<(), Error> {
        // Helper function to store updated root leaves
        fn save_root_leaves<'db, S>(
            storage: S,
            temp_root_leaves: &BTreeMap<Vec<u8>, usize>,
        ) -> Result<(), Error>
        where
            S: StorageContext<'db>,
            Error: From<<S as storage::StorageContext<'db>>::Error>,
        {
            let root_leaves_serialized = bincode::serialize(&temp_root_leaves).map_err(|_| {
                Error::CorruptedData(String::from("unable to serialize root leaves data"))
            })?;
            Ok(storage.put_meta(ROOT_LEAFS_SERIALIZED_KEY, &root_leaves_serialized)?)
        }

        if ops.is_empty() {
            return Ok(());
        }

        let mut temp_root_leaves = self.get_root_leaf_keys(transaction)?;

        // 1. Collect all batch operations into RBTree to keep them sorted and validated
        let mut sorted_operations = RBTree::new(GroveDbOpAdapter::new());
        for op in ops {
            insert_unique_op(&mut sorted_operations, Box::new(op));
        }

        let mut validated_operations =
            self.validate_batch(sorted_operations, &temp_root_leaves, transaction)?;

        // `StorageBatch` allows us to collect operations on different subtrees before
        // execution
        let storage_batch = StorageBatch::new();

        // With the only one difference (if there is a transaction) do the following:
        // 2. If nothing left to do and we were on a non-leaf subtree or we're done with
        //    one subtree and moved to another then add propagation operation to the
        //    operations tree and drop Merk handle;
        // 3. Take Merk from temp subtrees or open a new one with batched storage
        //    context;
        // 4. Apply operation to the Merk;
        // 5. Remove operation from the tree, repeat until there are operations to do;
        // 6. Add root leaves save operation to the batch
        // 7. Apply storage batch
        if let Some(tx) = transaction {
            self.apply_body(&mut validated_operations, &mut temp_root_leaves, |path| {
                let storage = self.db.get_batch_transactional_storage_context(
                    path.iter().map(|x| x.as_slice()),
                    &storage_batch,
                    tx,
                );
                Merk::open(storage)
                    .map_err(|_| Error::CorruptedData("cannot open a subtree".to_owned()))
            })?;

            let meta_storage = self.db.get_batch_transactional_storage_context(
                std::iter::empty(),
                &storage_batch,
                tx,
            );
            save_root_leaves(meta_storage, &temp_root_leaves)?;
            self.db
                .commit_multi_context_batch_with_transaction(storage_batch, tx)?;
        } else {
            self.apply_body(&mut validated_operations, &mut temp_root_leaves, |path| {
                let storage = self
                    .db
                    .get_batch_storage_context(path.iter().map(|x| x.as_slice()), &storage_batch);
                Merk::open(storage)
                    .map_err(|_| Error::CorruptedData("cannot open a subtree".to_owned()))
            })?;

            let meta_storage = self
                .db
                .get_batch_storage_context(std::iter::empty(), &storage_batch);
            save_root_leaves(meta_storage, &temp_root_leaves)?;
            self.db.commit_multi_context_batch(storage_batch)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{make_grovedb, ANOTHER_TEST_LEAF, TEST_LEAF};

    #[test]
    fn test_batch_validation_ok() {
        let db = make_grovedb();
        let element = Element::new_item(b"ayy".to_vec());
        let element2 = Element::new_item(b"ayy2".to_vec());
        let ops = vec![
            GroveDbOp::insert(vec![], b"key1".to_vec(), Element::empty_tree()),
            GroveDbOp::insert(
                vec![b"key1".to_vec(), b"key2".to_vec(), b"key3".to_vec()],
                b"key4".to_vec(),
                element.clone(),
            ),
            GroveDbOp::insert(
                vec![b"key1".to_vec(), b"key2".to_vec()],
                b"key3".to_vec(),
                Element::empty_tree(),
            ),
            GroveDbOp::insert(
                vec![b"key1".to_vec()],
                b"key2".to_vec(),
                Element::empty_tree(),
            ),
            GroveDbOp::insert(
                vec![TEST_LEAF.to_vec()],
                b"key1".to_vec(),
                Element::empty_tree(),
            ),
            GroveDbOp::insert(
                vec![TEST_LEAF.to_vec(), b"key1".to_vec()],
                b"key2".to_vec(),
                element2.clone(),
            ),
        ];
        db.apply_batch(ops, None).expect("cannot apply batch");

        db.get([], b"key1", None).expect("cannot get element");
        db.get([b"key1".as_ref()], b"key2", None)
            .expect("cannot get element");
        db.get([b"key1".as_ref(), b"key2"], b"key3", None)
            .expect("cannot get element");
        db.get([b"key1".as_ref(), b"key2", b"key3"], b"key4", None)
            .expect("cannot get element");

        assert_eq!(
            db.get([b"key1".as_ref(), b"key2", b"key3"], b"key4", None)
                .expect("cannot get element"),
            element
        );
        assert_eq!(
            db.get([TEST_LEAF, b"key1"], b"key2", None)
                .expect("cannot get element"),
            element2
        );
    }

    #[test]
    fn test_batch_validation_broken_chain() {
        let db = make_grovedb();
        let element = Element::new_item(b"ayy".to_vec());
        let ops = vec![
            GroveDbOp::insert(vec![], b"key1".to_vec(), Element::empty_tree()),
            GroveDbOp::insert(
                vec![b"key1".to_vec(), b"key2".to_vec(), b"key3".to_vec()],
                b"key4".to_vec(),
                element.clone(),
            ),
            GroveDbOp::insert(
                vec![b"key1".to_vec()],
                b"key2".to_vec(),
                Element::empty_tree(),
            ),
        ];
        assert!(db.apply_batch(ops, None).is_err());
        assert!(db.get([b"key1".as_ref()], b"key2", None).is_err());
    }

    #[test]
    fn test_batch_validation_broken_chain_aborts_whole_batch() {
        let db = make_grovedb();
        let element = Element::new_item(b"ayy".to_vec());
        let ops = vec![
            GroveDbOp::insert(
                vec![TEST_LEAF.to_vec()],
                b"key1".to_vec(),
                Element::empty_tree(),
            ),
            GroveDbOp::insert(
                vec![TEST_LEAF.to_vec(), b"key1".to_vec()],
                b"key2".to_vec(),
                element.clone(),
            ),
            GroveDbOp::insert(vec![], b"key1".to_vec(), Element::empty_tree()),
            GroveDbOp::insert(
                vec![b"key1".to_vec(), b"key2".to_vec(), b"key3".to_vec()],
                b"key4".to_vec(),
                element.clone(),
            ),
            GroveDbOp::insert(
                vec![b"key1".to_vec()],
                b"key2".to_vec(),
                Element::empty_tree(),
            ),
        ];
        assert!(db.apply_batch(ops, None).is_err());
        assert!(db.get([b"key1".as_ref()], b"key2", None).is_err());
        assert!(db.get([TEST_LEAF, b"key1"], b"key2", None).is_err(),);
    }

    #[test]
    fn test_batch_validation_deletion_brokes_chain() {
        let db = make_grovedb();
        let element = Element::new_item(b"ayy".to_vec());

        db.insert([], b"key1", Element::empty_tree(), None)
            .expect("cannot insert a subtree");
        db.insert([], b"key2", Element::empty_tree(), None)
            .expect("cannot insert a subtree");

        let ops = vec![
            GroveDbOp::insert(
                vec![b"key1".to_vec(), b"key2".to_vec(), b"key3".to_vec()],
                b"key4".to_vec(),
                element.clone(),
            ),
            GroveDbOp::insert(
                vec![b"key1".to_vec(), b"key2".to_vec()],
                b"key3".to_vec(),
                Element::empty_tree(),
            ),
            GroveDbOp::delete(vec![b"key1".to_vec()], b"key2".to_vec()),
        ];
        assert!(db.apply_batch(ops, None).is_err());
    }

    #[test]
    fn test_batch_validation_deletion_and_insertion_restore_chain() {
        let db = make_grovedb();
        let element = Element::new_item(b"ayy".to_vec());
        let ops = vec![
            GroveDbOp::insert(vec![], b"key1".to_vec(), Element::empty_tree()),
            GroveDbOp::insert(
                vec![b"key1".to_vec(), b"key2".to_vec(), b"key3".to_vec()],
                b"key4".to_vec(),
                element.clone(),
            ),
            GroveDbOp::insert(
                vec![b"key1".to_vec(), b"key2".to_vec()],
                b"key3".to_vec(),
                Element::empty_tree(),
            ),
            GroveDbOp::insert(
                vec![b"key1".to_vec()],
                b"key2".to_vec(),
                Element::empty_tree(),
            ),
            GroveDbOp::delete(vec![b"key1".to_vec()], b"key2".to_vec()),
        ];
        db.apply_batch(ops, None).expect("cannot apply batch");
        assert_eq!(
            db.get([b"key1".as_ref(), b"key2", b"key3"], b"key4", None)
                .expect("cannot get element"),
            element
        );
    }

    #[test]
    fn test_batch_validation_insert_into_existing_tree() {
        let db = make_grovedb();
        let element = Element::new_item(b"ayy".to_vec());

        db.insert([TEST_LEAF], b"invalid", element.clone(), None)
            .expect("cannot insert value");
        db.insert([TEST_LEAF], b"valid", Element::empty_tree(), None)
            .expect("cannot insert value");

        // Insertion into scalar is invalid
        let ops = vec![GroveDbOp::insert(
            vec![TEST_LEAF.to_vec(), b"invalid".to_vec()],
            b"key1".to_vec(),
            element.clone(),
        )];
        assert!(db.apply_batch(ops, None).is_err());

        // Insertion into a tree is correct
        let ops = vec![GroveDbOp::insert(
            vec![TEST_LEAF.to_vec(), b"valid".to_vec()],
            b"key1".to_vec(),
            element.clone(),
        )];
        db.apply_batch(ops, None).expect("cannot apply batch");
        assert_eq!(
            db.get([TEST_LEAF, b"valid"], b"key1", None)
                .expect("cannot get element"),
            element
        );
    }

    #[test]
    fn test_batch_validation_nested_subtree_overwrite() {
        let db = make_grovedb();
        let element = Element::new_item(b"ayy".to_vec());
        let element2 = Element::new_item(b"ayy2".to_vec());
        db.insert([TEST_LEAF], b"key_subtree", Element::empty_tree(), None)
            .expect("cannot insert a subtree");
        db.insert([TEST_LEAF, b"key_subtree"], b"key2", element, None)
            .expect("cannot insert an item");

        // TEST_LEAF will be overwritten thus nested subtrees will be deleted and it is
        // invalid to insert into them
        let ops = vec![
            GroveDbOp::insert(vec![], TEST_LEAF.to_vec(), element2.clone()),
            GroveDbOp::insert(
                vec![TEST_LEAF.to_vec(), b"key_subtree".to_vec()],
                b"key1".to_vec(),
                Element::empty_tree(),
            ),
        ];
        assert!(db.apply_batch(ops, None).is_err());

        // TEST_LEAF will became a scalar, insertion into scalar is also invalid
        let ops = vec![
            GroveDbOp::insert(vec![], TEST_LEAF.to_vec(), element2.clone()),
            GroveDbOp::insert(
                vec![TEST_LEAF.to_vec()],
                b"key1".to_vec(),
                Element::empty_tree(),
            ),
        ];
        assert!(db.apply_batch(ops, None).is_err());

        // Here TEST_LEAF is overwritten and new data should be available why older data
        // shouldn't
        let ops = vec![
            GroveDbOp::insert(vec![], TEST_LEAF.to_vec(), Element::empty_tree()),
            GroveDbOp::insert(vec![TEST_LEAF.to_vec()], b"key1".to_vec(), element2.clone()),
        ];
        assert!(db.apply_batch(ops, None).is_ok());

        assert_eq!(
            db.get([TEST_LEAF], b"key1", None).expect("cannot get data"),
            element2
        );
        assert!(db.get([TEST_LEAF, b"key_subtree"], b"key1", None).is_err());
    }

    #[test]
    fn test_batch_validation_root_leaf_removal() {
        let db = make_grovedb();
        let ops = vec![
            GroveDbOp::insert(
                vec![],
                TEST_LEAF.to_vec(),
                Element::new_item(b"ayy".to_vec()),
            ),
            GroveDbOp::insert(
                vec![TEST_LEAF.to_vec()],
                b"key1".to_vec(),
                Element::empty_tree(),
            ),
        ];
        assert!(db.apply_batch(ops, None).is_err());
    }

    #[test]
    fn test_merk_data_is_deleted() {
        let db = make_grovedb();
        let element = Element::new_item(b"ayy".to_vec());

        db.insert([TEST_LEAF], b"key1", Element::empty_tree(), None)
            .expect("cannot insert a subtree");
        db.insert([TEST_LEAF, b"key1"], b"key2", element.clone(), None)
            .expect("cannot insert an item");
        let ops = vec![GroveDbOp::insert(
            vec![TEST_LEAF.to_vec()],
            b"key1".to_vec(),
            Element::new_item(b"ayy2".to_vec()),
        )];

        assert_eq!(
            db.get([TEST_LEAF, b"key1"], b"key2", None)
                .expect("cannot get item"),
            element
        );
        db.apply_batch(ops, None).expect("cannot apply batch");
        assert!(db.get([TEST_LEAF, b"key1"], b"key2", None).is_err());
    }

    #[test]
    fn test_multi_tree_insertion_deletion_with_propagation_no_tx() {
        let db = make_grovedb();
        db.insert([], b"key1", Element::empty_tree(), None)
            .expect("cannot insert root leaf");
        db.insert([], b"key2", Element::empty_tree(), None)
            .expect("cannot insert root leaf");
        db.insert([ANOTHER_TEST_LEAF], b"key1", Element::empty_tree(), None)
            .expect("cannot insert root leaf");

        let hash = db
            .root_hash(None)
            .ok()
            .flatten()
            .expect("cannot get root hash");
        let element = Element::new_item(b"ayy".to_vec());
        let element2 = Element::new_item(b"ayy2".to_vec());

        let ops = vec![
            GroveDbOp::insert(
                vec![b"key1".to_vec(), b"key2".to_vec(), b"key3".to_vec()],
                b"key4".to_vec(),
                element.clone(),
            ),
            GroveDbOp::insert(
                vec![b"key1".to_vec(), b"key2".to_vec()],
                b"key3".to_vec(),
                Element::empty_tree(),
            ),
            GroveDbOp::insert(
                vec![b"key1".to_vec()],
                b"key2".to_vec(),
                Element::empty_tree(),
            ),
            GroveDbOp::insert(vec![TEST_LEAF.to_vec()], b"key".to_vec(), element2.clone()),
            GroveDbOp::delete(vec![ANOTHER_TEST_LEAF.to_vec()], b"key1".to_vec()),
        ];
        db.apply_batch(ops, None).expect("cannot apply batch");

        assert!(db.get([ANOTHER_TEST_LEAF], b"key1", None).is_err());

        assert_eq!(
            db.get([b"key1".as_ref(), b"key2", b"key3"], b"key4", None)
                .expect("cannot get element"),
            element
        );
        assert_eq!(
            db.get([TEST_LEAF], b"key", None)
                .expect("cannot get element"),
            element2
        );
        assert_ne!(
            db.root_hash(None)
                .ok()
                .flatten()
                .expect("cannot get root hash"),
            hash
        );
        let mut root_leafs = BTreeMap::new();
        root_leafs.insert(TEST_LEAF.to_vec(), 0);
        root_leafs.insert(ANOTHER_TEST_LEAF.to_vec(), 1);
        root_leafs.insert(b"key1".to_vec(), 2);
        root_leafs.insert(b"key2".to_vec(), 3);

        assert_eq!(
            db.get_root_leaf_keys(None).expect("cannot get root leafs"),
            root_leafs
        );
    }

    #[test]
    fn test_nested_batch_insertion_corrupts_state() {
        let db = make_grovedb();
        let full_path = vec![
            b"leaf1".to_vec(),
            b"sub1".to_vec(),
            b"sub2".to_vec(),
            b"sub3".to_vec(),
            b"sub4".to_vec(),
            b"sub5".to_vec(),
        ];
        let mut acc_path: Vec<Vec<u8>> = vec![];
        for p in full_path.into_iter() {
            db.insert(
                acc_path.iter().map(|x| x.as_slice()),
                &p,
                Element::empty_tree(),
                None,
            )
            .unwrap();
            acc_path.push(p);
        }

        let element = Element::new_item(b"ayy".to_vec());
        let batch = vec![GroveDbOp::insert(
            acc_path.clone(),
            b"key".to_vec(),
            element.clone(),
        )];
        db.apply_batch(batch, None).expect("cannot apply batch");

        let batch = vec![GroveDbOp::insert(
            acc_path,
            b"key".to_vec(),
            element.clone(),
        )];
        db.apply_batch(batch, None)
            .expect("cannot apply same batch twice");
    }
}
