use costs::OperationCost;
use storage::Storage;

use super::GroveDb;
use crate::Element;

impl GroveDb {
    // Worst case costs for operations within a single merk
    fn worst_case_encoded_link_size(key_size: u32) -> u32 {
        // Links are optional values that represent the right or left node for a given
        // tree 1 byte to represent the option state
        // 1 byte to represent key_length
        // key_length to represent the actual key
        // 32 bytes for the hash of the node
        // 1 byte for the left child height
        // 1 byte for the right child height
        1 + 1 + key_size + 32 + 1 + 1
    }

    fn worst_case_encoded_kv_node_size(max_element_size: u32) -> u32 {
        // KV holds the state of a node
        // 32 bytes to encode the hash of the node
        // 32 bytes to encode the value hash
        // max_element_size to encode the worst case value size
        32 + 32 + max_element_size
    }

    /// Add worst case for getting a merk node
    pub(crate) fn add_worst_case_get_merk_node(
        cost: &mut OperationCost,
        key_size: u32,
        max_element_size: u32,
    ) {
        // Worst case scenario, the element is not already in memory.
        // One direct seek has to be performed to read the node from storage.
        cost.seek_count += 1;

        // To write a node to disk, the left link, right link and kv nodes are encoded.
        // worst case, the node has both the left and right link present.
        let loaded_storage_bytes = (2 * Self::worst_case_encoded_link_size(key_size))
            + Self::worst_case_encoded_kv_node_size(max_element_size);
        cost.storage_loaded_bytes += loaded_storage_bytes;
    }

    pub(crate) fn add_merk_worst_case_insert_reference(
        cost: &mut OperationCost,
        max_element_size: u32,
        max_element_number: u32,
        max_key_size: u32
    ) {
        // same as insert node but one less hash node call as that is done on the
        // grovedb layer
        Self::add_worst_case_insert_merk_node(cost, max_element_size, max_element_number, max_key_size);
        cost.hash_node_calls -= 1;
    }

    pub(crate) fn add_worst_case_insert_merk_node(
        cost: &mut OperationCost,
        // key: &[u8],
        max_element_size: u32,
        max_element_number: u32,
        max_key_size: u32,
    ) {
        // For worst case conditions:
        // - merk tree was just opened hence only root node and corresponding links are loaded

        // maximum height of a tree
        // 1.44 * log n
        let max_tree_height = (1.44 * (max_element_number as f32).log2()).floor() as u32;

        // to insert a node, we have to walk from the root to some leaf node
        let max_number_of_walks = max_tree_height - 1;

        // for each walk, we have to seek and load from storage (equivalent to a get)
        for _ in 0..max_number_of_walks {
            GroveDb::add_worst_case_get_merk_node(cost, max_key_size, max_element_size)
        }

        // after getting to the point of insertion, we need to build the node
        // this requires computing the value_hash and kv_hash
        cost.hash_node_calls += 2;

        // on insertion, we need to balance the tree
        // worst case, every node up to the root needs to be rebalanced
        // A
        //  B
        //   C
        // effect of a single rotation either left or right
        //  - A becomes child of B and sibiling of C
        //      B
        //    A   C
        // looking at this, B and A get new node_hashes, as their child content has changed.
        // C remains the same.
        //
        //  A
        //    C
        //  B
        // effect of a double rotation
        // - first B goes to the middle, then A becomes the child of B
        // A
        //  B
        //   C
        // then
        //    B
        //  A  C
        // this modifies the child content of all nodes involved
        // hence node_hash has to be recomputed for all involved nodes.

        // TODO: This doesn't feel very accurate
        // Walking back up, each node gets reattached
        // marking them as modified, this bears no additional cost
        // TODO: balancing also happens here, map this out
        // after base insertion or updates, balancing occurs
        // the only effect this has is increasing the number of modified nodes for future hash re computation
        // worst case, it adds one additional node per balancing operation
        let max_number_of_modified_nodes = max_number_of_walks * 2;

        // commit stage
        // for every modified node, recursively call commit on all modified children
        // at base, write the node to storage
        // we create a batch entry [key, encoded_tree]
        // prefixed key is created during get storage context for merk open
        let prefix_size: u32 = 32;
        let prefixed_key_size = prefix_size + max_key_size;
        let value_size = (2 * Self::worst_case_encoded_link_size(max_key_size))
            + Self::worst_case_encoded_kv_node_size(max_element_size);

        for _ in 0..max_number_of_modified_nodes {
            cost.seek_count += 1;
            cost.hash_node_calls += 1;
            cost.storage_written_bytes += (prefixed_key_size + value_size)
        }

        // Write the root key
        cost.seek_count += 1;
        cost.storage_written_bytes += (b"root".len() + max_key_size);
    }

    /// Add worst case for getting a merk tree
    pub fn add_worst_case_get_merk<'db, 'p, P, S: Storage<'db>>(
        cost: &mut OperationCost,
        path: P,
        max_element_size: u32,
    ) where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: ExactSizeIterator + DoubleEndedIterator + Clone,
    {
        cost.seek_count += 2; // 1 for seek in meta for root key, 1 for loading that root key
        cost.storage_loaded_bytes += max_element_size;
        *cost += S::get_storage_context_cost(path);
    }

    /// Add worst case for getting a merk tree
    pub fn add_worst_case_merk_has_element(
        cost: &mut OperationCost,
        key: &[u8],
        max_element_size: u32,
    ) {
        cost.seek_count += 1;
        cost.storage_loaded_bytes += key.len() as u32 + max_element_size;
    }

    /// Add worst case for getting a merk tree root hash
    pub fn add_worst_case_merk_root_hash(cost: &mut OperationCost) {
        cost.hash_node_calls += Self::node_hash_update_count();
    }

    const fn node_hash_update_count() -> u16 {
        // It's a hash of node hash, left and right
        let bytes = merk::HASH_LENGTH * 3;
        let blocks = (bytes - 64 + 1) / 64;

        blocks as u16
    }

    /// Add worst case for insertion into merk
    pub(crate) fn add_worst_case_merk_insert(
        cost: &mut OperationCost,
        key: &[u8],
        value: &Element,
        input: MerkWorstCaseInput,
    ) {
        // TODO is is safe to unwrap?
        let bytes_len = key.len() + value.serialize().expect("element is serializeable").len();

        cost.storage_written_bytes += bytes_len as u32;
        // .. and hash computation for the inserted element iteslf
        cost.hash_node_calls += ((bytes_len - 64 + 1) / 64) as u16;

        Self::add_worst_case_merk_propagate(cost, input);
    }

    pub(crate) fn add_worst_case_merk_propagate(
        cost: &mut OperationCost,
        input: MerkWorstCaseInput,
    ) {
        let mut nodes_updated = 0;
        // Propagation requires to recompute and write hashes up to the root
        let levels = match input {
            MerkWorstCaseInput::MaxElementsNumber(n) => ((n + 1) as f32).log2().ceil() as u32,
            MerkWorstCaseInput::NumberOfLevels(n) => n,
        };
        nodes_updated += levels;
        // In AVL tree two rotation may happen at most on insertion, some of them may
        // update one more node except one we already have on our path to the
        // root, thus two more updates.
        nodes_updated += 2;

        // TODO: use separate field for hash propagation rather than written bytes
        cost.storage_written_bytes += nodes_updated * 32;
        // Same number of hash recomputations for propagation
        cost.hash_node_calls += (nodes_updated as u16) * Self::node_hash_update_count();
    }
}

pub(crate) enum MerkWorstCaseInput {
    MaxElementsNumber(u32),
    NumberOfLevels(u32),
}

#[cfg(test)]
mod test {
    use std::iter::empty;

    use costs::{CostContext, OperationCost};
    use merk::{test_utils::make_batch_seq, Merk, Op};
    use storage::{rocksdb_storage::RocksDbStorage, Storage};
    use tempfile::TempDir;

    use crate::GroveDb;

    #[test]
    fn test_get_merk_node_worst_case() {
        // Open a merk and insert 10 elements.
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

        // drop merk, so nothing is stored in memory
        drop(merk);

        // Reopen merk: this time, only root node is loaded to memory
        let mut merk = Merk::open(storage.get_storage_context(empty()).unwrap())
            .unwrap()
            .expect("cannot open merk");

        // To simulate worst case, we need to pick a node that:
        // 1. Is not in memory
        // 2. Left link exists
        // 3. Right link exists
        // Based on merk's avl rotation algorithm node is key 8 satisfies this
        let node_result = merk.get(&8_u64.to_be_bytes());

        // By tweaking the max element size, we can adapt the worst case function to
        // this scenario make_batch_seq creates values that are 60 bytes in size
        // (this will be the max_element_size)
        let mut cost = OperationCost::default();
        let key = &8_u64.to_be_bytes();
        GroveDb::add_worst_case_get_merk_node(&mut cost, key.len() as u32, 60);
        assert_eq!(cost, node_result.cost);
    }

    #[test]
    fn test_insert_merk_node_worst_case() {
        // let mut cost = OperationCost::default();
        // GroveDb::add_worst_case_insert_merk_node(&mut cost, 30, 10, 256);
        // dbg!(cost);
        // Open a merk and insert 10 elements.
        let tmp_dir = TempDir::new().expect("cannot open tempdir");
        let storage =
        RocksDbStorage::default_rocksdb_with_path(tmp_dir.path())
            .expect("cannot open rocksdb storage");
        let mut merk =
        Merk::open(storage.get_storage_context(empty()).unwrap())
            .unwrap()
            .expect("cannot open merk");
        // let batch = make_batch_seq(1..209);
        let a = vec![b"1".to_vec(), b"12".to_vec(), b"13".to_vec(), b"3".to_vec(), b"2".to_vec(), b"11".to_vec()];
        for m in a {
            println!();
            println!("inserting {}", std::str::from_utf8(&m).unwrap());
            merk.apply::<_, Vec<_>>(&[(m, Op::Put(b"a".to_vec()))], &[])
                .unwrap()
                .unwrap();
        }
        //
        // // drop merk, so nothing is stored in memory
        // drop(merk);
        // //
        // // // Reopen merk: this time, only root node is loaded to memory
        // let mut merk =
        // Merk::open(storage.get_storage_context(empty()).unwrap())
        //     .unwrap()
        //     .expect("cannot open merk");
        //
        // let batch = make_batch_seq(10..11);
        // let m = merk.apply::<_, Vec<_>>(batch.as_slice(), &[]);
        // dbg!(m);
    }
}
