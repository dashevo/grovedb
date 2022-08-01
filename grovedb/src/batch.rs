//! GroveDB batch operations support

use core::fmt;
use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::{btree_map::Entry, BTreeMap, HashMap, HashSet},
    hash::Hash,
};

use costs::{
    cost_return_on_error, cost_return_on_error_no_add, CostResult, CostsExt, OperationCost,
};
use merk::Merk;
use nohash_hasher::IntMap;
use storage::{rocksdb_storage::RocksDbStorage, Storage, StorageBatch, StorageContext};
use visualize::{DebugByteVectors, DebugBytes, Drawer, Visualize};

use crate::{
    operations::get::MAX_REFERENCE_HOPS, worst_case_costs::MerkWorstCaseInput, Element, Error,
    GroveDb, TransactionArg, MAX_ELEMENTS_NUMBER, MAX_ELEMENT_SIZE,
};

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum Op {
    ReplaceTreeHash { hash: [u8; 32] },
    Insert { element: Element },
    Delete,
}

impl Op {
    fn worst_case_cost(&self, key: Vec<u8>, input: MerkWorstCaseInput) -> OperationCost {
        match self {
            Op::ReplaceTreeHash { .. } => OperationCost {
                seek_count: 1,
                storage_written_bytes: 32,
                ..Default::default()
            },
            Op::Insert { element } => {
                let mut cost = OperationCost::default();
                GroveDb::add_worst_case_merk_insert(&mut cost, &key, &element, input);
                cost
            }
            Op::Delete => {
                let mut cost = OperationCost::default();
                GroveDb::add_worst_case_merk_propagate(&mut cost, input);
                cost
            }
        }
    }
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
    pub path: Vec<Vec<u8>>,
    /// Key of an element in the subtree
    pub key: Vec<u8>,
    /// Operation to perform on the key
    pub op: Op,
}

impl PartialEq for GroveDbOp {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.key == other.key && self.op == other.op
    }
}

impl fmt::Debug for GroveDbOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut path_out = Vec::new();
        let mut path_drawer = Drawer::new(&mut path_out);
        for p in &self.path {
            path_drawer = p.visualize(path_drawer).unwrap();
            path_drawer.write(b" ").unwrap();
        }
        let mut key_out = Vec::new();
        let key_drawer = Drawer::new(&mut key_out);
        self.key.visualize(key_drawer).unwrap();

        let op_dbg = match &self.op {
            Op::Insert { element } => match element {
                Element::Item(..) => "Insert Item",
                Element::Reference(..) => "Insert Ref",
                Element::Tree(..) => "Insert Tree",
            },
            Op::Delete => "Delete",
            Op::ReplaceTreeHash { .. } => "Replace Tree Hash",
        };

        f.debug_struct("GroveDbOp")
            .field("path", &String::from_utf8_lossy(&path_out))
            .field("key", &String::from_utf8_lossy(&key_out))
            .field("op", &op_dbg)
            .finish()
    }
}

impl GroveDbOp {
    pub fn insert(path: Vec<Vec<u8>>, key: Vec<u8>, element: Element) -> Self {
        Self {
            path,
            key,
            op: Op::Insert { element },
        }
    }

    pub fn delete(path: Vec<Vec<u8>>, key: Vec<u8>) -> Self {
        Self {
            path,
            key,
            op: Op::Delete,
        }
    }

    pub fn verify_consistency_of_operations(ops: &Vec<GroveDbOp>) -> GroveDbOpConsistencyResults {
        let ops_len = ops.len();
        // operations should not have any duplicates
        let mut repeated_ops = vec![];
        for (i, op) in ops.iter().enumerate() {
            if i == ops_len {
                continue;
            } // Don't do last one
            let count = ops
                .split_at(i + 1)
                .1
                .iter()
                .filter(|&current_op| current_op == op)
                .count() as u16;
            if count > 1 {
                repeated_ops.push((op.clone(), count));
            }
        }

        let mut same_path_key_ops = vec![];

        // No double insert or delete of same key in same path
        for (i, op) in ops.iter().enumerate() {
            if i == ops_len {
                continue;
            } // Don't do last one
            let mut doubled_ops = ops
                .split_at(i + 1)
                .1
                .iter()
                .filter_map(|current_op| {
                    if current_op.path == op.path && current_op.key == op.key {
                        Some(op.op.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<Op>>();
            if doubled_ops.len() > 0 {
                doubled_ops.push(op.op.clone());
                same_path_key_ops.push((op.path.clone(), op.key.clone(), doubled_ops));
            }
        }

        let inserts = ops
            .iter()
            .filter_map(|current_op| {
                if let Op::Insert { .. } = current_op.op {
                    Some(current_op.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<GroveDbOp>>();

        let deletes = ops
            .iter()
            .filter_map(|current_op| {
                if let Op::Delete = current_op.op {
                    Some(current_op.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<GroveDbOp>>();

        let mut insert_ops_below_deleted_ops = vec![];

        // No inserts under a deleted path
        for deleted_op in deletes.iter() {
            let mut deleted_qualified_path = deleted_op.path.clone();
            deleted_qualified_path.push(deleted_op.key.clone());
            let inserts_with_deleted_ops_above = inserts
                .iter()
                .filter_map(|inserted_op| {
                    if deleted_op.path.len() < inserted_op.path.len()
                        && deleted_qualified_path
                            .iter()
                            .zip(inserted_op.path.iter())
                            .all(|(a, b)| a == b)
                    {
                        Some(inserted_op.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<GroveDbOp>>();
            if inserts_with_deleted_ops_above.len() > 0 {
                insert_ops_below_deleted_ops
                    .push((deleted_op.clone(), inserts_with_deleted_ops_above));
            }
        }

        GroveDbOpConsistencyResults {
            repeated_ops,
            same_path_key_ops,
            insert_ops_below_deleted_ops,
        }
    }
}

#[derive(Debug)]
pub struct GroveDbOpConsistencyResults {
    repeated_ops: Vec<(GroveDbOp, u16)>, // the u16 is count
    same_path_key_ops: Vec<(Vec<Vec<u8>>, Vec<u8>, Vec<Op>)>,
    insert_ops_below_deleted_ops: Vec<(GroveDbOp, Vec<GroveDbOp>)>, /* the deleted op first,
                                                                     * then inserts under */
}

impl GroveDbOpConsistencyResults {
    pub fn is_empty(&self) -> bool {
        self.repeated_ops.is_empty()
            && self.same_path_key_ops.is_empty()
            && self.insert_ops_below_deleted_ops.is_empty()
    }
}

/// Cache for Merk trees by their paths.
struct TreeCacheMerkByPath<S, F> {
    merks: HashMap<Vec<Vec<u8>>, Merk<S>>,
    get_merk_fn: F,
}

/// Cache for subtee paths for worst case scenario costs.
#[derive(Default)]
struct TreeCacheKnownPaths {
    paths: HashSet<Vec<Vec<u8>>>,
}

impl<S, F> fmt::Debug for TreeCacheMerkByPath<S, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TreeCacheMerkByPath").finish()
    }
}

impl fmt::Debug for TreeCacheKnownPaths {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TreeCacheKnownPaths").finish()
    }
}

trait TreeCache {
    fn insert(&mut self, op: &GroveDbOp) -> CostResult<(), Error>;

    fn execute_ops_on_path(
        &mut self,
        path: &[Vec<u8>],
        ops_at_path_by_key: BTreeMap<Vec<u8>, Op>,
        ops_by_qualified_paths: &HashMap<Vec<Vec<u8>>, Op>,
        batch_apply_options: &BatchApplyOptions,
    ) -> CostResult<[u8; 32], Error>;
}

impl<'db, S, F> TreeCacheMerkByPath<S, F>
where
    F: FnMut(&[Vec<u8>]) -> CostResult<Merk<S>, Error>,
    S: StorageContext<'db>,
{
    fn follow_reference<'a>(
        &'a mut self,
        qualified_path: &[Vec<u8>],
        ops_by_qualified_paths: &'a HashMap<Vec<Vec<u8>>, Op>,
        recursions_allowed: u8,
    ) -> CostResult<Cow<Element>, Error> {
        let mut cost = OperationCost::default();
        if recursions_allowed == 0 {
            return Err(Error::ReferenceLimit).wrap_with_cost(cost);
        }
        if let Some(op) = ops_by_qualified_paths.get(qualified_path) {
            // the path is being modified, inserted or deleted in the batch of operations
            match op {
                Op::ReplaceTreeHash { .. } => {
                    return Err(Error::InvalidBatchOperation(
                        "references can not point to trees being updated",
                    ))
                    .wrap_with_cost(cost);
                }
                Op::Insert { element } => match element {
                    Element::Item(..) => Ok(Cow::Borrowed(element)).wrap_with_cost(cost),
                    Element::Reference(path, _) => {
                        self.follow_reference(path, ops_by_qualified_paths, recursions_allowed - 1)
                    }
                    Element::Tree(..) => {
                        return Err(Error::InvalidBatchOperation(
                            "references can not point to trees being updated",
                        ))
                        .wrap_with_cost(cost);
                    }
                },
                Op::Delete => {
                    return Err(Error::InvalidBatchOperation(
                        "references can not point to something currently being deleted",
                    ))
                    .wrap_with_cost(cost);
                }
            }
        } else {
            let (key, reference_path) = qualified_path.split_last().unwrap(); // already checked
            let reference_merk_wrapped = self
                .merks
                .remove(reference_path)
                .map(|x| Ok(x).wrap_with_cost(Default::default()))
                .unwrap_or_else(|| (self.get_merk_fn)(reference_path));
            let merk = cost_return_on_error!(&mut cost, reference_merk_wrapped);

            let referenced_element = cost_return_on_error!(
                &mut cost,
                merk.get(key.as_ref())
                    .map_err(|e| Error::CorruptedData(e.to_string()))
            );

            let referenced_element = cost_return_on_error_no_add!(
                &cost,
                referenced_element.ok_or(Error::MissingReference("reference in batch is missing"))
            );

            let element = cost_return_on_error_no_add!(
                &cost,
                Element::deserialize(referenced_element.as_slice()).map_err(|_| {
                    Error::CorruptedData(String::from("unable to deserialize element"))
                })
            );

            match element {
                Element::Item(..) => Ok(Cow::Owned(element)).wrap_with_cost(cost),
                Element::Reference(path, _) => self.follow_reference(
                    path.as_slice(),
                    ops_by_qualified_paths,
                    recursions_allowed - 1,
                ),
                Element::Tree(..) => {
                    return Err(Error::InvalidBatchOperation(
                        "references can not point to trees being updated",
                    ))
                    .wrap_with_cost(cost);
                }
            }
        }
    }
}

impl<'db, S, F> TreeCache for TreeCacheMerkByPath<S, F>
where
    F: FnMut(&[Vec<u8>]) -> CostResult<Merk<S>, Error>,
    S: StorageContext<'db>,
{
    fn insert(&mut self, op: &GroveDbOp) -> CostResult<(), Error> {
        let mut cost = OperationCost::default();

        let mut inserted_path = op.path.clone();
        inserted_path.push(op.key.clone());
        if !self.merks.contains_key(&inserted_path) {
            let merk = cost_return_on_error!(&mut cost, (self.get_merk_fn)(&inserted_path));
            self.merks.insert(inserted_path, merk);
        }

        Ok(()).wrap_with_cost(cost)
    }

    fn execute_ops_on_path(
        &mut self,
        path: &[Vec<u8>],
        ops_at_path_by_key: BTreeMap<Vec<u8>, Op>,
        ops_by_qualified_paths: &HashMap<Vec<Vec<u8>>, Op>,
        batch_apply_options: &BatchApplyOptions,
    ) -> CostResult<[u8; 32], Error> {
        let mut cost = OperationCost::default();

        let merk_wrapped = self
            .merks
            .remove(path)
            .map(|x| Ok(x).wrap_with_cost(Default::default()))
            .unwrap_or_else(|| (self.get_merk_fn)(path));
        let mut merk = cost_return_on_error!(&mut cost, merk_wrapped);

        let mut batch_operations: Vec<(Vec<u8>, _)> = vec![];
        for (key, op) in ops_at_path_by_key.into_iter() {
            match op {
                Op::Insert { element } => match &element {
                    Element::Reference(path_reference, _) => {
                        if path_reference.len() == 0 {
                            return Err(Error::InvalidBatchOperation(
                                "attempting to insert an empty reference",
                            ))
                            .wrap_with_cost(cost);
                        }
                        let referenced_element = cost_return_on_error!(
                            &mut cost,
                            self.follow_reference(
                                path_reference,
                                ops_by_qualified_paths,
                                MAX_REFERENCE_HOPS as u8
                            )
                        );
                        let serialized =
                            cost_return_on_error_no_add!(&cost, referenced_element.serialize());

                        cost_return_on_error!(
                            &mut cost,
                            element.insert_reference_into_batch_operations(
                                key,
                                serialized,
                                &mut batch_operations
                            )
                        );
                    }
                    Element::Item(..) | Element::Tree(..) => {
                        if batch_apply_options.validate_insertion_does_not_override {
                            let inserted = cost_return_on_error!(
                                &mut cost,
                                element.insert_if_not_exists_into_batch_operations(
                                    &mut merk,
                                    key,
                                    &mut batch_operations
                                )
                            );
                            if !inserted {
                                return Err(Error::InvalidBatchOperation(
                                    "attempting to overwrite a tree",
                                ))
                                .wrap_with_cost(cost);
                            }
                        } else {
                            cost_return_on_error!(
                                &mut cost,
                                element.insert_into_batch_operations(key, &mut batch_operations)
                            );
                        }
                    }
                },
                Op::Delete => {
                    cost_return_on_error!(
                        &mut cost,
                        Element::delete_into_batch_operations(key, &mut batch_operations)
                    );
                }
                Op::ReplaceTreeHash { hash } => {
                    cost_return_on_error!(
                        &mut cost,
                        GroveDb::update_tree_item_preserve_flag_into_batch_operations(
                            &merk,
                            key,
                            hash,
                            &mut batch_operations
                        )
                    );
                }
            }
        }
        cost_return_on_error!(&mut cost, unsafe {
            merk.apply_unchecked::<_, Vec<u8>>(&batch_operations, &[])
                .map_err(|e| Error::CorruptedData(e.to_string()))
        });
        merk.root_hash().add_cost(cost).map(Ok)
    }
}

impl TreeCache for TreeCacheKnownPaths {
    fn insert(&mut self, op: &GroveDbOp) -> CostResult<(), Error> {
        let mut inserted_path = op.path.clone();
        inserted_path.push(op.key.clone());
        self.paths.insert(inserted_path);
        let worst_case_cost = OperationCost::default();

        Ok(()).wrap_with_cost(worst_case_cost)
    }

    fn execute_ops_on_path(
        &mut self,
        path: &[Vec<u8>],
        ops_at_path_by_key: BTreeMap<Vec<u8>, Op>,
        _ops_by_qualified_paths: &HashMap<Vec<Vec<u8>>, Op>,
        _batch_apply_options: &BatchApplyOptions,
    ) -> CostResult<[u8; 32], Error> {
        let mut cost = OperationCost::default();

        if !self.paths.remove(path) {
            // Then we have to get the tree
            let path_slices = path.iter().map(|k| k.as_slice()).collect::<Vec<&[u8]>>();
            GroveDb::add_worst_case_get_merk::<_, RocksDbStorage>(
                &mut cost,
                path_slices,
                MAX_ELEMENT_SIZE,
            );
        }
        for (key, op) in ops_at_path_by_key.into_iter() {
            cost += op.worst_case_cost(
                key,
                MerkWorstCaseInput::MaxElementsNumber(MAX_ELEMENTS_NUMBER),
            );
        }
        GroveDb::add_worst_case_merk_propagate(
            &mut cost,
            MerkWorstCaseInput::NumberOfLevels(path.len() as u32),
        );
        Ok([0u8; 32]).wrap_with_cost(cost)
    }
}

///                          LEVEL           PATH                   KEY      OP
type OpsByLevelPath = IntMap<usize, BTreeMap<Vec<Vec<u8>>, BTreeMap<Vec<u8>, Op>>>;

struct BatchStructure<C> {
    /// Operations by level path
    ops_by_level_paths: OpsByLevelPath,
    /// This is for references
    ops_by_qualified_paths: HashMap<Vec<Vec<u8>>, Op>,
    /// Merk trees
    merk_tree_cache: C,
    /// Last level
    last_level: usize,
}

impl<S: fmt::Debug> fmt::Debug for BatchStructure<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut fmt_int_map = IntMap::default();
        for (level, path_map) in self.ops_by_level_paths.iter() {
            let mut fmt_path_map = BTreeMap::default();

            for (path, key_map) in path_map.iter() {
                let mut fmt_key_map = BTreeMap::default();

                for (key, op) in key_map.iter() {
                    fmt_key_map.insert(DebugBytes(key.clone()), op);
                }
                fmt_path_map.insert(DebugByteVectors(path.clone()), fmt_key_map);
            }
            fmt_int_map.insert(*level, fmt_path_map);
        }

        f.debug_struct("BatchStructure")
            .field("ops_by_level_paths", &fmt_int_map)
            .field("merk_tree_cache", &self.merk_tree_cache)
            .field("last_level", &self.last_level)
            .finish()
    }
}

impl<C> BatchStructure<C>
where
    C: TreeCache,
{
    fn from_ops(
        ops: Vec<GroveDbOp>,
        mut merk_tree_cache: C,
    ) -> CostResult<BatchStructure<C>, Error> {
        let mut cost = OperationCost::default();

        let mut ops_by_level_paths: OpsByLevelPath = IntMap::default();
        let mut current_last_level: usize = 0;

        // qualified paths meaning path + key
        let mut ops_by_qualified_paths: HashMap<Vec<Vec<u8>>, Op> = HashMap::new();

        for op in ops.into_iter() {
            let mut path = op.path.clone();
            path.push(op.key.clone());
            ops_by_qualified_paths.insert(path, op.op.clone());
            let op_cost = OperationCost::default();
            let op_result = match &op.op {
                Op::Insert { element } => {
                    if let Element::Tree(..) = element {
                        cost_return_on_error!(&mut cost, merk_tree_cache.insert(&op));
                    }
                    Ok(())
                }
                Op::Delete => Ok(()),
                Op::ReplaceTreeHash { .. } => Err(Error::InvalidBatchOperation(
                    "replace tree hash is an internal operation only",
                )),
            };
            if op_result.is_err() {
                return Err(op_result.err().unwrap()).wrap_with_cost(op_cost);
            }

            let level = op.path.len();
            if let Some(ops_on_level) = ops_by_level_paths.get_mut(&level) {
                if let Some(ops_on_path) = ops_on_level.get_mut(op.path.as_slice()) {
                    ops_on_path.insert(op.key, op.op);
                } else {
                    let mut ops_on_path: BTreeMap<Vec<u8>, Op> = BTreeMap::new();
                    ops_on_path.insert(op.key, op.op);
                    ops_on_level.insert(op.path.clone(), ops_on_path);
                }
            } else {
                let mut ops_on_path: BTreeMap<Vec<u8>, Op> = BTreeMap::new();
                ops_on_path.insert(op.key, op.op);
                let mut ops_on_level: BTreeMap<Vec<Vec<u8>>, BTreeMap<Vec<u8>, Op>> =
                    BTreeMap::new();
                ops_on_level.insert(op.path, ops_on_path);
                ops_by_level_paths.insert(level, ops_on_level);
                if current_last_level < level {
                    current_last_level = level;
                }
            }
        }

        Ok(BatchStructure {
            ops_by_level_paths,
            ops_by_qualified_paths,
            merk_tree_cache,
            last_level: current_last_level,
        })
        .wrap_with_cost(cost)
    }
}

#[derive(Debug, Default)]
pub struct BatchApplyOptions {
    pub validate_insertion_does_not_override: bool,
}

impl GroveDb {
    /// Method to propagate updated subtree root hashes up to GroveDB root
    fn apply_batch_structure<C: TreeCache>(
        &self,
        batch_structure: BatchStructure<C>,
        batch_apply_options: Option<BatchApplyOptions>,
    ) -> CostResult<(), Error> {
        let mut cost = OperationCost::default();
        let BatchStructure {
            mut ops_by_level_paths,
            ops_by_qualified_paths,
            mut merk_tree_cache,
            last_level,
        } = batch_structure;
        let mut current_level = last_level;

        let batch_apply_options = batch_apply_options.unwrap_or_default();

        // We will update up the tree
        while let Some(ops_at_level) = ops_by_level_paths.remove(&current_level) {
            for (path, ops_at_path) in ops_at_level.into_iter() {
                if current_level == 0 {
                    let mut root_tree_ops: BTreeMap<Vec<u8>, Op> = BTreeMap::new();
                    for (key, op) in ops_at_path.into_iter() {
                        match op {
                            Op::Insert { .. } => {
                                root_tree_ops.insert(key, op);
                            }
                            Op::Delete => {
                                return Err(Error::InvalidBatchOperation(
                                    "deletion of root tree not possible",
                                ))
                                .wrap_with_cost(cost);
                            }
                            Op::ReplaceTreeHash { hash } => {
                                root_tree_ops.insert(key, Op::ReplaceTreeHash { hash });
                            }
                        }
                    }
                    // execute the ops at this path
                    cost_return_on_error!(
                        &mut cost,
                        merk_tree_cache.execute_ops_on_path(
                            &path,
                            root_tree_ops,
                            &ops_by_qualified_paths,
                            &batch_apply_options,
                        )
                    );
                } else {
                    let root_hash = cost_return_on_error!(
                        &mut cost,
                        merk_tree_cache.execute_ops_on_path(
                            &path,
                            ops_at_path,
                            &ops_by_qualified_paths,
                            &batch_apply_options,
                        )
                    );

                    if current_level > 0 {
                        // We need to propagate up this root hash, this means adding grove_db
                        // operations up for the level above
                        if let Some((key, parent_path)) = path.split_last() {
                            if let Some(ops_at_level_above) =
                                ops_by_level_paths.get_mut(&(current_level - 1))
                            {
                                if let Some(ops_on_path) = ops_at_level_above.get_mut(parent_path) {
                                    match ops_on_path.entry(key.clone()) {
                                        Entry::Vacant(vacant_entry) => {
                                            vacant_entry
                                                .insert(Op::ReplaceTreeHash { hash: root_hash });
                                        }
                                        Entry::Occupied(occupied_entry) => {
                                            match occupied_entry.into_mut() {
                                                Op::ReplaceTreeHash { hash } => *hash = root_hash,
                                                Op::Insert { element } => {
                                                    if let Element::Tree(hash, _) = element {
                                                        *hash = root_hash
                                                    } else {
                                                        return Err(Error::InvalidBatchOperation(
                                                            "insertion of element under a non tree",
                                                        ))
                                                        .wrap_with_cost(cost);
                                                    }
                                                }
                                                Op::Delete => {
                                                    if root_hash != [0u8; 32] {
                                                        return Err(Error::InvalidBatchOperation(
                                                            "modification of tree when it will be \
                                                             deleted",
                                                        ))
                                                        .wrap_with_cost(cost);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    let mut ops_on_path: BTreeMap<Vec<u8>, Op> = BTreeMap::new();
                                    ops_on_path.insert(
                                        key.clone(),
                                        Op::ReplaceTreeHash { hash: root_hash },
                                    );
                                    ops_at_level_above.insert(parent_path.to_vec(), ops_on_path);
                                }
                            } else {
                                let mut ops_on_path: BTreeMap<Vec<u8>, Op> = BTreeMap::new();
                                ops_on_path
                                    .insert(key.clone(), Op::ReplaceTreeHash { hash: root_hash });
                                let mut ops_on_level: BTreeMap<
                                    Vec<Vec<u8>>,
                                    BTreeMap<Vec<u8>, Op>,
                                > = BTreeMap::new();
                                ops_on_level.insert(parent_path.to_vec(), ops_on_path);
                                ops_by_level_paths.insert(current_level - 1, ops_on_level);
                            }
                        }
                    }
                }
            }
            if current_level > 0 {
                current_level -= 1;
            }
        }
        Ok(()).wrap_with_cost(cost)
    }

    /// Method to propagate updated subtree root hashes up to GroveDB root
    fn apply_body<'db, S: StorageContext<'db>>(
        &self,
        ops: Vec<GroveDbOp>,
        batch_apply_options: Option<BatchApplyOptions>,
        get_merk_fn: impl FnMut(&[Vec<u8>]) -> CostResult<Merk<S>, Error>,
    ) -> CostResult<(), Error> {
        let mut cost = OperationCost::default();
        let batch_structure = cost_return_on_error!(
            &mut cost,
            BatchStructure::from_ops(
                ops,
                TreeCacheMerkByPath {
                    merks: Default::default(),
                    get_merk_fn,
                }
            )
        );
        self.apply_batch_structure(batch_structure, batch_apply_options)
            .add_cost(cost)
    }

    /// Applies operations on GroveDB without batching
    pub fn apply_operations_without_batching(
        &self,
        ops: Vec<GroveDbOp>,
        transaction: TransactionArg,
    ) -> CostResult<(), Error> {
        let mut cost = OperationCost::default();
        for op in ops.into_iter() {
            match op.op {
                Op::Insert { element } => {
                    let path_slices: Vec<&[u8]> = op.path.iter().map(|p| p.as_slice()).collect();
                    cost_return_on_error!(
                        &mut cost,
                        self.insert(path_slices, op.key.as_slice(), element, transaction,)
                    );
                }
                Op::Delete => {
                    let path_slices: Vec<&[u8]> = op.path.iter().map(|p| p.as_slice()).collect();
                    cost_return_on_error!(
                        &mut cost,
                        self.delete(path_slices, op.key.as_slice(), transaction,)
                    );
                }
                _ => {}
            }
        }
        Ok(()).wrap_with_cost(cost)
    }

    /// Applies batch of operations on GroveDB
    pub fn apply_batch(
        &self,
        ops: Vec<GroveDbOp>,
        batch_apply_options: Option<BatchApplyOptions>,
        transaction: TransactionArg,
    ) -> CostResult<(), Error> {
        let mut cost = OperationCost::default();

        if ops.is_empty() {
            return Ok(()).wrap_with_cost(cost);
        }

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
            cost_return_on_error!(
                &mut cost,
                self.apply_body(ops, batch_apply_options, |path| {
                    let storage = self
                        .db
                        .get_batch_transactional_storage_context(
                            path.iter().map(|x| x.as_slice()),
                            &storage_batch,
                            tx,
                        )
                        .unwrap_add_cost(&mut cost);
                    Merk::open(storage)
                        .map_err(|_| Error::CorruptedData("cannot open a subtree".to_owned()))
                })
            );

            // TODO: compute batch costs
            cost_return_on_error_no_add!(
                &cost,
                self.db
                    .commit_multi_context_batch(storage_batch, Some(tx))
                    .unwrap_add_cost(&mut cost)
                    .map_err(|e| e.into())
            );
        } else {
            cost_return_on_error!(
                &mut cost,
                self.apply_body(ops, batch_apply_options, |path| {
                    let storage = self
                        .db
                        .get_batch_storage_context(
                            path.iter().map(|x| x.as_slice()),
                            &storage_batch,
                        )
                        .unwrap_add_cost(&mut cost);
                    Merk::open(storage)
                        .map_err(|_| Error::CorruptedData("cannot open a subtree".to_owned()))
                })
            );

            // TODO: compute batch costs
            cost_return_on_error_no_add!(
                &cost,
                self.db
                    .commit_multi_context_batch(storage_batch, None)
                    .unwrap_add_cost(&mut cost)
                    .map_err(|e| e.into())
            );
        }
        Ok(()).wrap_with_cost(cost)
    }

    pub fn worst_case_operations_for_batch(
        &self,
        ops: Vec<GroveDbOp>,
        batch_apply_options: Option<BatchApplyOptions>,
    ) -> CostResult<(), Error> {
        let mut cost = OperationCost::default();

        if ops.is_empty() {
            return Ok(()).wrap_with_cost(cost);
        }

        let batch_structure = cost_return_on_error!(
            &mut cost,
            BatchStructure::from_ops(ops, TreeCacheKnownPaths::default())
        );
        cost_return_on_error!(
            &mut cost,
            self.apply_batch_structure(batch_structure, batch_apply_options)
        );

        Ok(()).wrap_with_cost(cost)
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
        db.apply_batch(ops, None, None)
            .unwrap()
            .expect("cannot apply batch");

        // visualize_stderr(&db);
        db.get([], b"key1", None)
            .unwrap()
            .expect("cannot get element");
        db.get([b"key1".as_ref()], b"key2", None)
            .unwrap()
            .expect("cannot get element");
        db.get([b"key1".as_ref(), b"key2"], b"key3", None)
            .unwrap()
            .expect("cannot get element");
        db.get([b"key1".as_ref(), b"key2", b"key3"], b"key4", None)
            .unwrap()
            .expect("cannot get element");

        assert_eq!(
            db.get([b"key1".as_ref(), b"key2", b"key3"], b"key4", None)
                .unwrap()
                .expect("cannot get element"),
            element
        );
        assert_eq!(
            db.get([TEST_LEAF, b"key1"], b"key2", None)
                .unwrap()
                .expect("cannot get element"),
            element2
        );
    }

    #[test]
    fn test_batch_validation_ok_on_transaction() {
        let db = make_grovedb();
        let tx = db.start_transaction();

        db.insert(vec![], b"keyb", Element::empty_tree(), Some(&tx))
            .unwrap()
            .expect("successful root tree leaf insert");

        let element = Element::new_item(b"ayy".to_vec());
        let element2 = Element::new_item(b"ayy2".to_vec());
        let ops = vec![
            GroveDbOp::insert(vec![], b"key1".to_vec(), Element::empty_tree()),
            GroveDbOp::insert(
                vec![b"key1".to_vec()],
                b"key2".to_vec(),
                Element::empty_tree(),
            ),
            GroveDbOp::insert(
                vec![b"key1".to_vec(), b"key2".to_vec()],
                b"key3".to_vec(),
                Element::empty_tree(),
            ),
            GroveDbOp::insert(
                vec![b"key1".to_vec(), b"key2".to_vec(), b"key3".to_vec()],
                b"key4".to_vec(),
                element.clone(),
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
        db.apply_batch(ops, None, Some(&tx))
            .unwrap()
            .expect("cannot apply batch");
        db.get([], b"keyb", None)
            .unwrap()
            .expect_err("we should not get an element");
        db.get([], b"keyb", Some(&tx))
            .unwrap()
            .expect("we should get an element");

        db.get([], b"key1", None)
            .unwrap()
            .expect_err("we should not get an element");
        db.get([], b"key1", Some(&tx))
            .unwrap()
            .expect("cannot get element");
        db.get([b"key1".as_ref()], b"key2", Some(&tx))
            .unwrap()
            .expect("cannot get element");
        db.get([b"key1".as_ref(), b"key2"], b"key3", Some(&tx))
            .unwrap()
            .expect("cannot get element");
        db.get([b"key1".as_ref(), b"key2", b"key3"], b"key4", Some(&tx))
            .unwrap()
            .expect("cannot get element");

        assert_eq!(
            db.get([b"key1".as_ref(), b"key2", b"key3"], b"key4", Some(&tx))
                .unwrap()
                .expect("cannot get element"),
            element
        );
        assert_eq!(
            db.get([TEST_LEAF, b"key1"], b"key2", Some(&tx))
                .unwrap()
                .expect("cannot get element"),
            element2
        );
    }

    fn grove_db_ops_for_contract_insert() -> Vec<GroveDbOp> {
        let mut grove_db_ops = vec![];

        grove_db_ops.push(GroveDbOp {
            path: vec![],
            key: b"contract".to_vec(),
            op: Op::Insert {
                element: Element::empty_tree(),
            },
        });
        grove_db_ops.push(GroveDbOp {
            path: vec![b"contract".to_vec()],
            key: (&[0u8]).to_vec(),
            op: Op::Insert {
                element: Element::new_item(b"serialized_contract".to_vec()),
            },
        });
        grove_db_ops.push(GroveDbOp {
            path: vec![b"contract".to_vec()],
            key: (&[1u8]).to_vec(),
            op: Op::Insert {
                element: Element::empty_tree(),
            },
        });
        grove_db_ops.push(GroveDbOp {
            path: vec![b"contract".to_vec(), (&[1u8]).to_vec()],
            key: b"domain".to_vec(),
            op: Op::Insert {
                element: Element::empty_tree(),
            },
        });
        grove_db_ops.push(GroveDbOp {
            path: vec![b"contract".to_vec(), (&[1u8]).to_vec(), b"domain".to_vec()],
            key: (&[0u8]).to_vec(),
            op: Op::Insert {
                element: Element::empty_tree(),
            },
        });
        grove_db_ops.push(GroveDbOp {
            path: vec![b"contract".to_vec(), (&[1u8]).to_vec(), b"domain".to_vec()],
            key: b"normalized_domain_label".to_vec(),
            op: Op::Insert {
                element: Element::empty_tree(),
            },
        });
        grove_db_ops.push(GroveDbOp {
            path: vec![b"contract".to_vec(), (&[1u8]).to_vec(), b"domain".to_vec()],
            key: b"unique_records".to_vec(),
            op: Op::Insert {
                element: Element::empty_tree(),
            },
        });
        grove_db_ops.push(GroveDbOp {
            path: vec![b"contract".to_vec(), (&[1u8]).to_vec(), b"domain".to_vec()],
            key: b"alias_records".to_vec(),
            op: Op::Insert {
                element: Element::empty_tree(),
            },
        });
        grove_db_ops.push(GroveDbOp {
            path: vec![b"contract".to_vec(), (&[1u8]).to_vec()],
            key: b"preorder".to_vec(),
            op: Op::Insert {
                element: Element::empty_tree(),
            },
        });
        grove_db_ops.push(GroveDbOp {
            path: vec![
                b"contract".to_vec(),
                (&[1u8]).to_vec(),
                b"preorder".to_vec(),
            ],
            key: (&[0u8]).to_vec(),
            op: Op::Insert {
                element: Element::empty_tree(),
            },
        });
        grove_db_ops.push(GroveDbOp {
            path: vec![
                b"contract".to_vec(),
                (&[1u8]).to_vec(),
                b"preorder".to_vec(),
            ],
            key: b"salted_domain".to_vec(),
            op: Op::Insert {
                element: Element::empty_tree(),
            },
        });

        grove_db_ops
    }

    fn grove_db_ops_for_contract_document_insert() -> Vec<GroveDbOp> {
        let mut grove_db_ops = vec![];

        grove_db_ops.push(GroveDbOp {
            path: vec![
                b"contract".to_vec(),
                (&[1u8]).to_vec(),
                b"domain".to_vec(),
                (&[0u8]).to_vec(),
            ],
            key: b"serialized_domain_id".to_vec(),
            op: Op::Insert {
                element: Element::new_item(b"serialized_domain".to_vec()),
            },
        });

        grove_db_ops.push(GroveDbOp {
            path: vec![
                b"contract".to_vec(),
                (&[1u8]).to_vec(),
                b"domain".to_vec(),
                b"normalized_domain_label".to_vec(),
            ],
            key: b"dash".to_vec(),
            op: Op::Insert {
                element: Element::empty_tree(),
            },
        });

        grove_db_ops.push(GroveDbOp {
            path: vec![
                b"contract".to_vec(),
                (&[1u8]).to_vec(),
                b"domain".to_vec(),
                b"normalized_domain_label".to_vec(),
                b"dash".to_vec(),
            ],
            key: b"normalized_label".to_vec(),
            op: Op::Insert {
                element: Element::empty_tree(),
            },
        });

        grove_db_ops.push(GroveDbOp {
            path: vec![
                b"contract".to_vec(),
                (&[1u8]).to_vec(),
                b"domain".to_vec(),
                b"normalized_domain_label".to_vec(),
                b"dash".to_vec(),
                b"normalized_label".to_vec(),
            ],
            key: b"sam".to_vec(),
            op: Op::Insert {
                element: Element::empty_tree(),
            },
        });

        grove_db_ops.push(GroveDbOp {
            path: vec![
                b"contract".to_vec(),
                (&[1u8]).to_vec(),
                b"domain".to_vec(),
                b"normalized_domain_label".to_vec(),
                b"dash".to_vec(),
                b"normalized_label".to_vec(),
                b"sam".to_vec(),
            ],
            key: b"sam_id".to_vec(),
            op: Op::Insert {
                element: Element::new_reference(vec![
                    b"contract".to_vec(),
                    (&[1u8]).to_vec(),
                    b"domain".to_vec(),
                    (&[0u8]).to_vec(),
                    b"serialized_domain_id".to_vec(),
                ]),
            },
        });

        grove_db_ops
    }

    // This test no longer works as of version 5, there might be support for it in
    // the future
    #[ignore]
    #[test]
    fn test_batch_produces_same_result() {
        let db = make_grovedb();
        let tx = db.start_transaction();

        let ops = grove_db_ops_for_contract_insert();
        db.apply_batch(ops, None, Some(&tx))
            .unwrap()
            .expect("expected to apply batch");

        db.root_hash(None).unwrap().expect("cannot get root hash");

        let db = make_grovedb();
        let tx = db.start_transaction();

        let ops = grove_db_ops_for_contract_insert();
        db.apply_batch(ops.clone(), None, Some(&tx))
            .unwrap()
            .expect("expected to apply batch");

        let batch_hash = db
            .root_hash(Some(&tx))
            .unwrap()
            .expect("cannot get root hash");

        db.rollback_transaction(&tx).expect("expected to rollback");

        db.apply_operations_without_batching(ops, Some(&tx))
            .unwrap()
            .expect("expected to apply batch");

        let no_batch_hash = db
            .root_hash(Some(&tx))
            .unwrap()
            .expect("cannot get root hash");

        assert_eq!(batch_hash, no_batch_hash);
    }

    #[ignore]
    #[test]
    fn test_batch_contract_with_document_produces_same_result() {
        let db = make_grovedb();
        let tx = db.start_transaction();

        let ops = grove_db_ops_for_contract_insert();
        db.apply_batch(ops, None, Some(&tx))
            .unwrap()
            .expect("expected to apply batch");

        db.root_hash(None).unwrap().expect("cannot get root hash");

        let db = make_grovedb();
        let tx = db.start_transaction();

        let ops = grove_db_ops_for_contract_insert();
        let document_ops = grove_db_ops_for_contract_document_insert();
        db.apply_batch(ops.clone(), None, Some(&tx))
            .unwrap()
            .expect("expected to apply batch");
        db.apply_batch(document_ops.clone(), None, Some(&tx))
            .unwrap()
            .expect("expected to apply batch");

        let batch_hash = db
            .root_hash(Some(&tx))
            .unwrap()
            .expect("cannot get root hash");

        db.rollback_transaction(&tx).expect("expected to rollback");

        db.apply_operations_without_batching(ops, Some(&tx))
            .unwrap()
            .expect("expected to apply batch");
        db.apply_operations_without_batching(document_ops, Some(&tx))
            .unwrap()
            .expect("expected to apply batch");

        let no_batch_hash = db
            .root_hash(Some(&tx))
            .unwrap()
            .expect("cannot get root hash");

        assert_eq!(batch_hash, no_batch_hash);
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
        assert!(db.apply_batch(ops, None, None).unwrap().is_err());
        assert!(db.get([b"key1".as_ref()], b"key2", None).unwrap().is_err());
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
        assert!(db.apply_batch(ops, None, None).unwrap().is_err());
        assert!(db.get([b"key1".as_ref()], b"key2", None).unwrap().is_err());
        assert!(db
            .get([TEST_LEAF, b"key1"], b"key2", None)
            .unwrap()
            .is_err(),);
    }

    #[test]
    fn test_batch_validation_deletion_brokes_chain() {
        let db = make_grovedb();
        let element = Element::new_item(b"ayy".to_vec());

        db.insert([], b"key1", Element::empty_tree(), None)
            .unwrap()
            .expect("cannot insert a subtree");
        db.insert([b"key1".as_ref()], b"key2", Element::empty_tree(), None)
            .unwrap()
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
        assert!(db.apply_batch(ops, None, None).unwrap().is_err());
    }

    #[test]
    fn test_batch_validation_insertion_under_deleted_tree() {
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
        db.apply_batch(ops, None, None)
            .unwrap()
            .expect_err("insertion of element under a deleted tree should not be allowed");
        db.get([b"key1".as_ref(), b"key2", b"key3"], b"key4", None)
            .unwrap()
            .expect_err("nothing should have been inserted");
    }

    #[test]
    fn test_batch_validation_insert_into_existing_tree() {
        let db = make_grovedb();
        let element = Element::new_item(b"ayy".to_vec());

        db.insert([TEST_LEAF], b"invalid", element.clone(), None)
            .unwrap()
            .expect("cannot insert value");
        db.insert([TEST_LEAF], b"valid", Element::empty_tree(), None)
            .unwrap()
            .expect("cannot insert value");

        // Insertion into scalar is invalid
        let ops = vec![GroveDbOp::insert(
            vec![TEST_LEAF.to_vec(), b"invalid".to_vec()],
            b"key1".to_vec(),
            element.clone(),
        )];
        assert!(db.apply_batch(ops, None, None).unwrap().is_err());

        // Insertion into a tree is correct
        let ops = vec![GroveDbOp::insert(
            vec![TEST_LEAF.to_vec(), b"valid".to_vec()],
            b"key1".to_vec(),
            element.clone(),
        )];
        db.apply_batch(ops, None, None)
            .unwrap()
            .expect("cannot apply batch");
        assert_eq!(
            db.get([TEST_LEAF, b"valid"], b"key1", None)
                .unwrap()
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
            .unwrap()
            .expect("cannot insert a subtree");
        db.insert([TEST_LEAF, b"key_subtree"], b"key2", element, None)
            .unwrap()
            .expect("cannot insert an item");

        // TEST_LEAF can not be overwritten
        let ops = vec![
            GroveDbOp::insert(vec![], TEST_LEAF.to_vec(), element2.clone()),
            GroveDbOp::insert(
                vec![TEST_LEAF.to_vec(), b"key_subtree".to_vec()],
                b"key1".to_vec(),
                Element::empty_tree(),
            ),
        ];
        assert!(db
            .apply_batch(
                ops,
                Some(BatchApplyOptions {
                    validate_insertion_does_not_override: true
                }),
                None
            )
            .unwrap()
            .is_err());

        // TEST_LEAF will be deleted so you can not insert underneath it
        let ops = vec![
            GroveDbOp::delete(vec![], TEST_LEAF.to_vec()),
            GroveDbOp::insert(
                vec![TEST_LEAF.to_vec()],
                b"key1".to_vec(),
                Element::empty_tree(),
            ),
        ];
        assert!(db.apply_batch(ops, None, None).unwrap().is_err());

        // TEST_LEAF will be deleted so you can not insert underneath it
        // We are testing with the batch apply option
        // validate_tree_insertion_does_not_override set to true
        let ops = vec![
            GroveDbOp::delete(vec![], TEST_LEAF.to_vec()),
            GroveDbOp::insert(
                vec![TEST_LEAF.to_vec()],
                b"key1".to_vec(),
                Element::empty_tree(),
            ),
        ];
        assert!(db
            .apply_batch(
                ops,
                Some(BatchApplyOptions {
                    validate_insertion_does_not_override: true
                }),
                None
            )
            .unwrap()
            .is_err());
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
        assert!(db
            .apply_batch(
                ops,
                Some(BatchApplyOptions {
                    validate_insertion_does_not_override: true
                }),
                None
            )
            .unwrap()
            .is_err());
    }

    #[test]
    fn test_merk_data_is_deleted() {
        let db = make_grovedb();
        let element = Element::new_item(b"ayy".to_vec());

        db.insert([TEST_LEAF], b"key1", Element::empty_tree(), None)
            .unwrap()
            .expect("cannot insert a subtree");
        db.insert([TEST_LEAF, b"key1"], b"key2", element.clone(), None)
            .unwrap()
            .expect("cannot insert an item");
        let ops = vec![GroveDbOp::insert(
            vec![TEST_LEAF.to_vec()],
            b"key1".to_vec(),
            Element::new_item(b"ayy2".to_vec()),
        )];

        assert_eq!(
            db.get([TEST_LEAF, b"key1"], b"key2", None)
                .unwrap()
                .expect("cannot get item"),
            element
        );
        db.apply_batch(ops, None, None)
            .unwrap()
            .expect("cannot apply batch");
        assert!(db
            .get([TEST_LEAF, b"key1"], b"key2", None)
            .unwrap()
            .is_err());
    }

    #[test]
    fn test_multi_tree_insertion_deletion_with_propagation_no_tx() {
        let db = make_grovedb();
        db.insert([], b"key1", Element::empty_tree(), None)
            .unwrap()
            .expect("cannot insert root leaf");
        db.insert([], b"key2", Element::empty_tree(), None)
            .unwrap()
            .expect("cannot insert root leaf");
        db.insert([ANOTHER_TEST_LEAF], b"key1", Element::empty_tree(), None)
            .unwrap()
            .expect("cannot insert root leaf");

        let hash = db.root_hash(None).unwrap().expect("cannot get root hash");
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
        db.apply_batch(ops, None, None)
            .unwrap()
            .expect("cannot apply batch");

        assert!(db.get([ANOTHER_TEST_LEAF], b"key1", None).unwrap().is_err());

        assert_eq!(
            db.get([b"key1".as_ref(), b"key2", b"key3"], b"key4", None)
                .unwrap()
                .expect("cannot get element"),
            element
        );
        assert_eq!(
            db.get([TEST_LEAF], b"key", None)
                .unwrap()
                .expect("cannot get element"),
            element2
        );
        assert_ne!(
            db.root_hash(None).unwrap().expect("cannot get root hash"),
            hash
        );

        // verify root leaves
        assert!(db.get([], TEST_LEAF, None).unwrap().is_ok());
        assert!(db.get([], ANOTHER_TEST_LEAF, None).unwrap().is_ok());
        assert!(db.get([], b"key1", None).unwrap().is_ok());
        assert!(db.get([], b"key2", None).unwrap().is_ok());
        assert!(db.get([], b"key3", None).unwrap().is_err());
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
            .unwrap()
            .expect("expected to insert");
            acc_path.push(p);
        }

        let element = Element::new_item(b"ayy".to_vec());
        let batch = vec![GroveDbOp::insert(
            acc_path.clone(),
            b"key".to_vec(),
            element.clone(),
        )];
        db.apply_batch(batch, None, None)
            .unwrap()
            .expect("cannot apply batch");

        let batch = vec![GroveDbOp::insert(
            acc_path,
            b"key".to_vec(),
            element.clone(),
        )];
        db.apply_batch(batch, None, None)
            .unwrap()
            .expect("cannot apply same batch twice");
    }

    #[test]
    fn test_apply_sorted_pre_validated_batch_propagation() {
        let db = make_grovedb();
        let full_path = vec![b"leaf1".to_vec(), b"sub1".to_vec()];
        let mut acc_path: Vec<Vec<u8>> = vec![];
        for p in full_path.into_iter() {
            db.insert(
                acc_path.iter().map(|x| x.as_slice()),
                &p,
                Element::empty_tree(),
                None,
            )
            .unwrap()
            .expect("expected to insert");
            acc_path.push(p);
        }

        let root_hash = db.root_hash(None).unwrap().unwrap();

        let element = Element::new_item(b"ayy".to_vec());
        let batch = vec![GroveDbOp::insert(
            acc_path.clone(),
            b"key".to_vec(),
            element.clone(),
        )];
        db.apply_batch(batch, None, None)
            .unwrap()
            .expect("cannot apply batch");

        assert_ne!(db.root_hash(None).unwrap().unwrap(), root_hash);
    }
}
