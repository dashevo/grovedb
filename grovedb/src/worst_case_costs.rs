use costs::OperationCost;
use storage::Storage;

use super::GroveDb;
use crate::Element;

impl GroveDb {
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
