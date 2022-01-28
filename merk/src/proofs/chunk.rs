use anyhow::{bail, Result};
use storage::RawIterator;
#[cfg(feature = "full")]
use {
    super::tree::{execute, Tree as ProofTree},
    crate::tree::Hash,
    crate::tree::Tree,
};

use super::{Node, Op};
use crate::tree::{Fetch, RefWalker};

/// The minimum number of layers the trunk will be guaranteed to have before
/// splitting into multiple chunks. If the tree's height is less than double
/// this value, the trunk should be verified as a leaf chunk.
pub const MIN_TRUNK_HEIGHT: usize = 5;

impl<'a, S> RefWalker<'a, S>
where
    S: Fetch + Sized + Clone,
{
    /// Generates a trunk proof by traversing the tree.
    ///
    /// Returns a tuple containing the produced proof, and a boolean indicating
    /// whether or not there will be more chunks to follow. If the chunk
    /// contains the entire tree, the boolean will be `false`, if the chunk
    /// is abridged and will be connected to leaf chunks, it will be `true`.
    pub fn create_trunk_proof(&mut self) -> Result<(Vec<Op>, bool)> {
        let approx_size = 2usize.pow((self.tree().height() / 2) as u32) * 3;
        let mut proof = Vec::with_capacity(approx_size);

        let trunk_height = self.traverse_for_height_proof(&mut proof, 1)?;

        if trunk_height < MIN_TRUNK_HEIGHT {
            proof.clear();
            self.traverse_for_trunk(&mut proof, usize::MAX, true)?;
            Ok((proof, false))
        } else {
            self.traverse_for_trunk(&mut proof, trunk_height, true)?;
            Ok((proof, true))
        }
    }

    /// Traverses down the left edge of the tree and pushes ops to the proof, to
    /// act as a proof of the height of the tree. This is the first step in
    /// generating a trunk proof.
    fn traverse_for_height_proof(&mut self, proof: &mut Vec<Op>, depth: usize) -> Result<usize> {
        let maybe_left = self.walk(true)?;
        let has_left_child = maybe_left.is_some();

        let trunk_height = if let Some(mut left) = maybe_left {
            left.traverse_for_height_proof(proof, depth + 1)?
        } else {
            depth / 2
        };

        if depth > trunk_height {
            proof.push(Op::Push(self.to_kvhash_node()));

            if has_left_child {
                proof.push(Op::Parent);
            }

            if let Some(right) = self.tree().link(false) {
                proof.push(Op::Push(Node::Hash(*right.hash())));
                proof.push(Op::Child);
            }
        }

        Ok(trunk_height)
    }

    /// Traverses down the tree and adds KV push ops for all nodes up to a
    /// certain depth. This expects the proof to contain a height proof as
    /// generated by `traverse_for_height_proof`.
    fn traverse_for_trunk(
        &mut self,
        proof: &mut Vec<Op>,
        remaining_depth: usize,
        is_leftmost: bool,
    ) -> Result<()> {
        if remaining_depth == 0 {
            // return early if we have reached bottom of trunk

            // for leftmost node, we already have height proof
            if is_leftmost {
                return Ok(());
            }

            // add this node's hash
            proof.push(Op::Push(self.to_hash_node()));

            return Ok(());
        }

        // traverse left
        let has_left_child = self.tree().link(true).is_some();
        if has_left_child {
            let mut left = self.walk(true)?.unwrap();
            left.traverse_for_trunk(proof, remaining_depth - 1, is_leftmost)?;
        }

        // add this node's data
        proof.push(Op::Push(self.to_kv_node()));

        if has_left_child {
            proof.push(Op::Parent);
        }

        // traverse right
        if let Some(mut right) = self.walk(false)? {
            right.traverse_for_trunk(proof, remaining_depth - 1, false)?;
            proof.push(Op::Child);
        }

        Ok(())
    }
}

/// Builds a chunk proof by iterating over values in a RocksDB, ending the chunk
/// when a node with key `end_key` is encountered.
///
/// Advances the iterator for all nodes in the chunk and the `end_key` (if any).
#[cfg(feature = "full")]
pub(crate) fn get_next_chunk(
    iter: &mut impl RawIterator,
    end_key: Option<&[u8]>,
) -> Result<Vec<Op>> {
    let mut chunk = Vec::with_capacity(512);
    let mut stack = Vec::with_capacity(32);
    let mut node = Tree::new(vec![], vec![]);

    while iter.valid() {
        let key = iter.key().unwrap();

        if let Some(end_key) = end_key {
            if key == end_key {
                break;
            }
        }

        let encoded_node = iter.value().unwrap();
        Tree::decode_into(&mut node, vec![], encoded_node);

        let kv = Node::KV(key.to_vec(), node.value().to_vec());
        chunk.push(Op::Push(kv));

        if node.link(true).is_some() {
            chunk.push(Op::Parent);
        }

        if let Some(child) = node.link(false) {
            stack.push(child.key().to_vec());
        } else {
            while let Some(top_key) = stack.last() {
                if key < top_key.as_slice() {
                    break;
                }
                stack.pop();
                chunk.push(Op::Child);
            }
        }

        iter.next();
    }

    if iter.valid() {
        iter.next();
    }

    Ok(chunk)
}

/// Verifies a leaf chunk proof by executing its operators. Checks that there
/// were no abridged nodes (Hash or KVHash) and the proof hashes to
/// `expected_hash`.
#[cfg(feature = "full")]
#[allow(dead_code)] // TODO: remove when proofs will be enabled
pub(crate) fn verify_leaf<I: Iterator<Item = Result<Op>>>(
    ops: I,
    expected_hash: Hash,
) -> Result<ProofTree> {
    let tree = execute(ops, false, |node| match node {
        Node::KV(..) => Ok(()),
        _ => bail!("Leaf chunks must contain full subtree"),
    })?;

    if tree.hash() != expected_hash {
        bail!(
            "Leaf chunk proof did not match expected hash\n\tExpected: {:?}\n\tActual: {:?}",
            expected_hash,
            tree.hash()
        );
    }

    Ok(tree)
}

/// Verifies a trunk chunk proof by executing its operators. Ensures the
/// resulting tree contains a valid height proof, the trunk is the correct
/// height, and all of its inner nodes are not abridged. Returns the tree and
/// the height given by the height proof.
#[cfg(feature = "full")]
#[allow(dead_code)] // TODO: remove when proofs will be enabled
pub(crate) fn verify_trunk<I: Iterator<Item = Result<Op>>>(ops: I) -> Result<(ProofTree, usize)> {
    fn verify_height_proof(tree: &ProofTree) -> Result<usize> {
        Ok(match tree.child(true) {
            Some(child) => {
                if let Node::Hash(_) = child.tree.node {
                    bail!("Expected height proof to only contain KV and KVHash nodes")
                }
                verify_height_proof(&child.tree)? + 1
            }
            None => 1,
        })
    }

    fn verify_completeness(tree: &ProofTree, remaining_depth: usize, leftmost: bool) -> Result<()> {
        let recurse = |left, leftmost| {
            if let Some(child) = tree.child(left) {
                verify_completeness(&child.tree, remaining_depth - 1, left && leftmost)?;
            }
            Ok(())
        };

        if remaining_depth > 0 {
            match tree.node {
                Node::KV(..) => {}
                _ => bail!("Expected trunk inner nodes to contain keys and values"),
            }
            recurse(true, leftmost)?;
            recurse(false, false)
        } else if !leftmost {
            match tree.node {
                Node::Hash(_) => Ok(()),
                _ => bail!("Expected trunk leaves to contain Hash nodes"),
            }
        } else {
            match &tree.node {
                Node::KVHash(_) => Ok(()),
                _ => bail!("Expected leftmost trunk leaf to contain KVHash node"),
            }
        }
    }

    let mut kv_only = true;
    let tree = execute(ops, false, |node| {
        kv_only &= matches!(node, Node::KV(_, _));
        Ok(())
    })?;

    let height = verify_height_proof(&tree)?;
    let trunk_height = height / 2;

    if trunk_height < MIN_TRUNK_HEIGHT {
        if !kv_only {
            bail!("Leaf chunks must contain full subtree");
        }
    } else {
        verify_completeness(&tree, trunk_height, true)?;
    }

    Ok((tree, height))
}

#[cfg(test)]
mod tests {
    use std::usize;

    use super::{super::tree::Tree, *};
    use crate::{
        test_utils::*,
        tree::{NoopCommit, PanicSource, Tree as BaseTree},
    };

    #[derive(Default)]
    struct NodeCounts {
        hash: usize,
        kvhash: usize,
        kv: usize,
    }

    fn count_node_types(tree: Tree) -> NodeCounts {
        let mut counts = NodeCounts::default();

        tree.visit_nodes(&mut |node| {
            match node {
                Node::Hash(_) => counts.hash += 1,
                Node::KVHash(_) => counts.kvhash += 1,
                Node::KV(..) => counts.kv += 1,
            };
        });

        counts
    }

    #[test]
    fn small_trunk_roundtrip() {
        let mut tree = make_tree_seq(31);
        let mut walker = RefWalker::new(&mut tree, PanicSource {});

        let (proof, has_more) = walker.create_trunk_proof().unwrap();
        assert!(!has_more);

        println!("{:?}", &proof);
        let (trunk, _) = verify_trunk(proof.into_iter().map(Ok)).unwrap();

        let counts = count_node_types(trunk);
        assert_eq!(counts.hash, 0);
        assert_eq!(counts.kv, 32);
        assert_eq!(counts.kvhash, 0);
    }

    #[test]
    fn big_trunk_roundtrip() {
        let mut tree = make_tree_seq(2u64.pow(MIN_TRUNK_HEIGHT as u32 * 2 + 1) - 1);
        let mut walker = RefWalker::new(&mut tree, PanicSource {});

        let (proof, has_more) = walker.create_trunk_proof().unwrap();
        assert!(has_more);
        let (trunk, _) = verify_trunk(proof.into_iter().map(Ok)).unwrap();

        let counts = count_node_types(trunk);
        // are these formulas correct for all values of `MIN_TRUNK_HEIGHT`? 🤔
        assert_eq!(
            counts.hash,
            2usize.pow(MIN_TRUNK_HEIGHT as u32) + MIN_TRUNK_HEIGHT - 1
        );
        assert_eq!(counts.kv, 2usize.pow(MIN_TRUNK_HEIGHT as u32) - 1);
        assert_eq!(counts.kvhash, MIN_TRUNK_HEIGHT + 1);
    }

    #[test]
    fn one_node_tree_trunk_roundtrip() {
        let mut tree = BaseTree::new(vec![0], vec![]);
        tree.commit(&mut NoopCommit {}).unwrap();

        let mut walker = RefWalker::new(&mut tree, PanicSource {});
        let (proof, has_more) = walker.create_trunk_proof().unwrap();
        assert!(!has_more);

        let (trunk, _) = verify_trunk(proof.into_iter().map(Ok)).unwrap();
        let counts = count_node_types(trunk);
        assert_eq!(counts.hash, 0);
        assert_eq!(counts.kv, 1);
        assert_eq!(counts.kvhash, 0);
    }

    #[test]
    fn two_node_right_heavy_tree_trunk_roundtrip() {
        // 0
        //  \
        //   1
        let mut tree =
            BaseTree::new(vec![0], vec![]).attach(false, Some(BaseTree::new(vec![1], vec![])));
        tree.commit(&mut NoopCommit {}).unwrap();
        let mut walker = RefWalker::new(&mut tree, PanicSource {});
        let (proof, has_more) = walker.create_trunk_proof().unwrap();
        assert!(!has_more);

        let (trunk, _) = verify_trunk(proof.into_iter().map(Ok)).unwrap();
        let counts = count_node_types(trunk);
        assert_eq!(counts.hash, 0);
        assert_eq!(counts.kv, 2);
        assert_eq!(counts.kvhash, 0);
    }

    #[test]
    fn two_node_left_heavy_tree_trunk_roundtrip() {
        //   1
        //  /
        // 0
        let mut tree =
            BaseTree::new(vec![1], vec![]).attach(true, Some(BaseTree::new(vec![0], vec![])));
        tree.commit(&mut NoopCommit {}).unwrap();
        let mut walker = RefWalker::new(&mut tree, PanicSource {});
        let (proof, has_more) = walker.create_trunk_proof().unwrap();
        assert!(!has_more);

        let (trunk, _) = verify_trunk(proof.into_iter().map(Ok)).unwrap();
        let counts = count_node_types(trunk);
        assert_eq!(counts.hash, 0);
        assert_eq!(counts.kv, 2);
        assert_eq!(counts.kvhash, 0);
    }

    #[test]
    fn three_node_tree_trunk_roundtrip() {
        //   1
        //  / \
        // 0   2
        let mut tree = BaseTree::new(vec![1], vec![])
            .attach(true, Some(BaseTree::new(vec![0], vec![])))
            .attach(false, Some(BaseTree::new(vec![2], vec![])));
        tree.commit(&mut NoopCommit {}).unwrap();

        let mut walker = RefWalker::new(&mut tree, PanicSource {});
        let (proof, has_more) = walker.create_trunk_proof().unwrap();
        assert!(!has_more);

        let (trunk, _) = verify_trunk(proof.into_iter().map(Ok)).unwrap();
        let counts = count_node_types(trunk);
        assert_eq!(counts.hash, 0);
        assert_eq!(counts.kv, 3);
        assert_eq!(counts.kvhash, 0);
    }

    #[test]
    fn leaf_chunk_roundtrip() {
        let mut merk = TempMerk::new();
        let batch = make_batch_seq(0..31);
        merk.apply::<_, Vec<_>>(batch.as_slice(), &[], None)
            .unwrap();

        let root_node = merk.tree.take();
        let root_key = root_node.as_ref().unwrap().key().to_vec();
        merk.tree.set(root_node);

        // whole tree as 1 leaf
        let mut iter = merk.inner.raw_iter();
        iter.seek_to_first();
        let chunk = get_next_chunk(&mut iter, None).unwrap();
        let ops = chunk.into_iter().map(Ok);
        let chunk = verify_leaf(ops, merk.root_hash()).unwrap();
        let counts = count_node_types(chunk);
        assert_eq!(counts.kv, 31);
        assert_eq!(counts.hash, 0);
        assert_eq!(counts.kvhash, 0);
        drop(iter);

        let mut iter = merk.inner.raw_iter();
        iter.seek_to_first();

        // left leaf
        let chunk = get_next_chunk(&mut iter, Some(root_key.as_slice())).unwrap();
        let ops = chunk.into_iter().map(Ok);
        let chunk = verify_leaf(
            ops,
            [
                34, 133, 104, 181, 253, 249, 189, 168, 15, 209, 70, 164, 224, 192, 18, 36, 1, 74,
                79, 9, 158, 188, 98, 47, 53, 32, 109, 14, 151, 13, 49, 74,
            ],
        )
        .unwrap();
        let counts = count_node_types(chunk);
        assert_eq!(counts.kv, 15);
        assert_eq!(counts.hash, 0);
        assert_eq!(counts.kvhash, 0);

        // right leaf
        let chunk = get_next_chunk(&mut iter, None).unwrap();
        let ops = chunk.into_iter().map(Ok);
        let chunk = verify_leaf(
            ops,
            [
                164, 29, 123, 213, 6, 25, 247, 238, 127, 53, 5, 70, 255, 87, 87, 204, 188, 169,
                181, 4, 185, 180, 74, 52, 244, 134, 75, 47, 105, 129, 209, 112,
            ],
        )
        .unwrap();
        let counts = count_node_types(chunk);
        assert_eq!(counts.kv, 15);
        assert_eq!(counts.hash, 0);
        assert_eq!(counts.kvhash, 0);
    }
}
