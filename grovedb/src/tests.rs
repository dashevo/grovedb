use std::{
    ops::{Deref, DerefMut},
    option::Option::None,
};

use rand::Rng;
use tempfile::TempDir;

// use test::RunIgnored::No;
use super::*;

pub const TEST_LEAF: &[u8] = b"test_leaf";
const ANOTHER_TEST_LEAF: &[u8] = b"test_leaf2";

/// GroveDB wrapper to keep temp directory alive
pub struct TempGroveDb {
    _tmp_dir: TempDir,
    db: GroveDb,
}

impl DerefMut for TempGroveDb {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.db
    }
}

impl Deref for TempGroveDb {
    type Target = GroveDb;

    fn deref(&self) -> &Self::Target {
        &self.db
    }
}

impl Visualize for TempGroveDb {
    fn visualize<'a, W: std::io::Write>(
        &self,
        drawer: Drawer<'a, W>,
    ) -> std::io::Result<Drawer<'a, W>> {
        self.db.visualize(drawer)
    }
}

/// A helper method to create GroveDB with one leaf for a root tree
pub fn make_grovedb() -> TempGroveDb {
    let tmp_dir = TempDir::new().unwrap();
    let mut db = GroveDb::open(tmp_dir.path()).unwrap();
    add_test_leafs(&mut db);
    TempGroveDb {
        _tmp_dir: tmp_dir,
        db,
    }
}

fn add_test_leafs(db: &mut GroveDb) {
    db.insert([], TEST_LEAF, Element::empty_tree(), None)
        .expect("successful root tree leaf insert");
    db.insert([], ANOTHER_TEST_LEAF, Element::empty_tree(), None)
        .expect("successful root tree leaf 2 insert");
}

#[test]
fn test_init() {
    let tmp_dir = TempDir::new().unwrap();
    GroveDb::open(tmp_dir).expect("empty tree is ok");
}

#[test]
fn test_insert_value_to_merk() {
    let db = make_grovedb();
    let element = Element::Item(b"ayy".to_vec());
    db.insert([TEST_LEAF], b"key", element.clone(), None)
        .expect("successful insert");
    assert_eq!(
        db.get([TEST_LEAF], b"key", None).expect("successful get"),
        element
    );
}

#[test]
fn test_insert_value_to_subtree() {
    let db = make_grovedb();
    let element = Element::Item(b"ayy".to_vec());

    // Insert a subtree first
    db.insert([TEST_LEAF], b"key1", Element::empty_tree(), None)
        .expect("successful subtree insert");
    // Insert an element into subtree
    db.insert([TEST_LEAF, b"key1"], b"key2", element.clone(), None)
        .expect("successful value insert");
    assert_eq!(
        db.get([TEST_LEAF, b"key1"], b"key2", None)
            .expect("successful get"),
        element
    );
}

#[test]
fn test_changes_propagated() {
    let db = make_grovedb();
    let old_hash = db.root_hash(None).unwrap();
    let element = Element::Item(b"ayy".to_vec());

    // Insert some nested subtrees
    db.insert([TEST_LEAF], b"key1", Element::empty_tree(), None)
        .expect("successful subtree 1 insert");
    db.insert([TEST_LEAF, b"key1"], b"key2", Element::empty_tree(), None)
        .expect("successful subtree 2 insert");
    // Insert an element into subtree
    db.insert(
        [TEST_LEAF, b"key1", b"key2"],
        b"key3",
        element.clone(),
        None,
    )
    .expect("successful value insert");
    assert_eq!(
        db.get([TEST_LEAF, b"key1", b"key2"], b"key3", None)
            .expect("successful get"),
        element
    );
    assert_ne!(old_hash, db.root_hash(None).unwrap());
}

#[test]
fn test_follow_references() {
    let db = make_grovedb();
    let element = Element::Item(b"ayy".to_vec());

    // Insert a reference
    db.insert(
        [TEST_LEAF],
        b"reference_key",
        Element::Reference(vec![TEST_LEAF.to_vec(), b"key2".to_vec(), b"key3".to_vec()]),
        None,
    )
    .expect("successful reference insert");

    // Insert an item to refer to
    db.insert([TEST_LEAF], b"key2", Element::empty_tree(), None)
        .expect("successful subtree 1 insert");
    db.insert([TEST_LEAF, b"key2"], b"key3", element.clone(), None)
        .expect("successful value insert");
    assert_eq!(
        db.get([TEST_LEAF], b"reference_key", None)
            .expect("successful get"),
        element
    );
}

#[test]
fn test_cyclic_references() {
    let db = make_grovedb();

    db.insert(
        [TEST_LEAF],
        b"reference_key_1",
        Element::Reference(vec![TEST_LEAF.to_vec(), b"reference_key_2".to_vec()]),
        None,
    )
    .expect("successful reference 1 insert");

    db.insert(
        [TEST_LEAF],
        b"reference_key_2",
        Element::Reference(vec![TEST_LEAF.to_vec(), b"reference_key_1".to_vec()]),
        None,
    )
    .expect("successful reference 2 insert");

    assert!(matches!(
        db.get([TEST_LEAF], b"reference_key_1", None).unwrap_err(),
        Error::CyclicReference
    ));
}

#[test]
fn test_too_many_indirections() {
    use crate::operations::get::MAX_REFERENCE_HOPS;
    let db = make_grovedb();

    let keygen = |idx| format!("key{}", idx).bytes().collect::<Vec<u8>>();

    db.insert([TEST_LEAF], b"key0", Element::Item(b"oops".to_vec()), None)
        .expect("successful item insert");

    for i in 1..=(MAX_REFERENCE_HOPS + 1) {
        db.insert(
            [TEST_LEAF],
            &keygen(i),
            Element::Reference(vec![TEST_LEAF.to_vec(), keygen(i - 1)]),
            None,
        )
        .expect("successful reference insert");
    }

    assert!(matches!(
        db.get([TEST_LEAF], &keygen(MAX_REFERENCE_HOPS + 1), None)
            .unwrap_err(),
        Error::ReferenceLimit
    ));
}

#[test]
fn test_tree_structure_is_persistent() {
    let tmp_dir = TempDir::new().unwrap();
    let element = Element::Item(b"ayy".to_vec());
    // Create a scoped GroveDB
    let prev_root_hash = {
        let mut db = GroveDb::open(tmp_dir.path()).unwrap();
        add_test_leafs(&mut db);

        // Insert some nested subtrees
        db.insert([TEST_LEAF], b"key1", Element::empty_tree(), None)
            .expect("successful subtree 1 insert");
        db.insert([TEST_LEAF, b"key1"], b"key2", Element::empty_tree(), None)
            .expect("successful subtree 2 insert");
        // Insert an element into subtree
        db.insert(
            [TEST_LEAF, b"key1", b"key2"],
            b"key3",
            element.clone(),
            None,
        )
        .expect("successful value insert");
        assert_eq!(
            db.get([TEST_LEAF, b"key1", b"key2"], b"key3", None)
                .expect("successful get 1"),
            element
        );
        db.root_hash(None).unwrap()
    };
    // Open a persisted GroveDB
    let db = GroveDb::open(tmp_dir).unwrap();
    assert_eq!(
        db.get([TEST_LEAF, b"key1", b"key2"], b"key3", None)
            .expect("successful get 2"),
        element
    );
    assert!(db
        .get([TEST_LEAF, b"key1", b"key2"], b"key4", None)
        .is_err());
    assert_eq!(prev_root_hash, db.root_hash(None).unwrap());
}

#[test]
fn test_root_tree_leafs_are_noted() {
    let db = make_grovedb();
    let mut hm = BTreeMap::new();
    hm.insert(TEST_LEAF.to_vec(), 0);
    hm.insert(ANOTHER_TEST_LEAF.to_vec(), 1);
    assert_eq!(db.get_root_leaf_keys(None).unwrap(), hm);
    assert_eq!(db.get_root_tree(None).unwrap().leaves_len(), 2);
}

// #[test]
// fn test_proof_construction() {
//     // Tree Structure
//     // root
//     //     test_leaf
//     //         innertree
//     //             k1,v1
//     //             k2,v2
//     //     another_test_leaf
//     //         innertree2
//     //             k3,v3
//     //         innertree3
//     //             k4,v4
//
//     // Insert elements into grovedb instance
//     let mut temp_db = make_grovedb();
//     // Insert level 1 nodes
//     temp_db
//         .insert(
//             [TEST_LEAF],
//             b"innertree",
//             Element::empty_tree(),
//             None,
//         )
//         .expect("successful subtree insert");
//     temp_db
//         .insert(
//             &[ANOTHER_TEST_LEAF],
//             b"innertree2",
//             Element::empty_tree(),
//             None,
//         )
//         .expect("successful subtree insert");
//     temp_db
//         .insert(
//             &[ANOTHER_TEST_LEAF],
//             b"innertree3",
//             Element::empty_tree(),
//             None,
//         )
//         .expect("successful subtree insert");
//     // Insert level 2 nodes
//     temp_db
//         .insert(
//             &[TEST_LEAF, b"innertree"],
//             b"key1",
//             Element::Item(b"value1"),
//             None,
//         )
//         .expect("successful subtree insert");
//     temp_db
//         .insert(
//             &[TEST_LEAF, b"innertree"],
//             b"key2",
//             Element::Item(b"value2".to_vec()),
//             None,
//         )
//         .expect("successful subtree insert");
//     temp_db
//         .insert(
//             &[ANOTHER_TEST_LEAF, b"innertree2"],
//             b"key3",
//             Element::Item(b"value3".to_vec()),
//             None,
//         )
//         .expect("successful subtree insert");
//     temp_db
//         .insert(
//             &[ANOTHER_TEST_LEAF, b"innertree3"],
//             b"key4",
//             Element::Item(b"value4".to_vec()),
//             None,
//         )
//         .expect("successful subtree insert");
//
//     // Manually construct HADS bottom up
//     // Insert level 2 nodes
//     let mut inner_tree = TempMerk::new();
//     let value_one = Element::Item(b"value1".to_vec());
//     value_one
//         .insert(&mut inner_tree, b"key1", None)
//         .unwrap();
//     let value_two = Element::Item(b"value2".to_vec());
//     value_two
//         .insert(&mut inner_tree, b"key2", None)
//         .unwrap();
//
//     let mut inner_tree_2 = TempMerk::new();
//     let value_three = Element::Item(b"value3".to_vec());
//     value_three
//         .insert(&mut inner_tree_2, b"key3", None)
//         .unwrap();
//
//     let mut inner_tree_3 = TempMerk::new();
//     let value_four = Element::Item(b"value4".to_vec());
//     value_four
//         .insert(&mut inner_tree_3, b"key4", None)
//         .unwrap();
//     // Insert level 1 nodes
//     let mut test_leaf = TempMerk::new();
//     let inner_tree_root = Element::Tree(inner_tree.root_hash());
//     inner_tree_root
//         .insert(&mut test_leaf, b"innertree", None)
//         .unwrap();
//     let mut another_test_leaf = TempMerk::new();
//     let inner_tree_2_root = Element::Tree(inner_tree_2.root_hash());
//     inner_tree_2_root
//         .insert(&mut another_test_leaf, b"innertree2", None)
//         .unwrap();
//     let inner_tree_3_root = Element::Tree(inner_tree_3.root_hash());
//     inner_tree_3_root
//         .insert(&mut another_test_leaf, b"innertree3", None)
//         .unwrap();
//     // Insert root nodes
//     let leaves = [test_leaf.root_hash(), another_test_leaf.root_hash()];
//     let root_tree = MerkleTree::<Sha256>::from_leaves(&leaves);
//
//     // Proof construction
//     // Generating a proof for two paths
//     // root -> test_leaf -> innertree (prove both k1 and k2)
//     // root -> another_test_leaf -> innertree3 (prove k4)
//     // root -> another_test_leaf -> innertree2 (prove k3)
//
//     // Build reusable query objects
//     let mut path_one_query = Query::new();
//     path_one_query.insert_key(b"key1");
//     path_one_query.insert_key(b"key2");
//
//     let mut path_two_query = Query::new();
//     path_two_query.insert_key(b"key4".to_vec());
//
//     let mut path_three_query = Query::new();
//     path_three_query.insert_key(b"key3");
//
//     // Get grovedb proof
//     let proof = temp_db
//         .proof(vec![
//             PathQuery::new_unsized(&[TEST_LEAF, b"innertree"],
// path_one_query),             PathQuery::new_unsized(&[ANOTHER_TEST_LEAF,
// b"innertree3"], path_two_query),
// PathQuery::new_unsized(&[ANOTHER_TEST_LEAF, b"innertree2"],
// path_three_query),         ])
//         .unwrap();
//
//     // Deserialize the proof
//     let proof: Proof = bincode::deserialize(&proof).unwrap();
//
//     // Perform assertions
//     assert_eq!(proof.query_paths.len(), 3);
//     assert_eq!(proof.query_paths[0], &[TEST_LEAF, b"innertree"]);
//     assert_eq!(proof.query_paths[1], &[ANOTHER_TEST_LEAF, b"innertree3"]);
//     assert_eq!(proof.query_paths[2], &[ANOTHER_TEST_LEAF, b"innertree2"]);
//
//     // For path 1 to path 3, there are 9 nodes
//     // root is repeated three times and another_test_leaf is repeated twice
//     // Accounting for duplication, there are 6 unique nodes
//     // root, test_leaf, another_test_leaf, innertree, innertree2, innertree3
//     // proof.proofs contains all nodes except the root so we expect 5 sub
// proofs     assert_eq!(proof.proofs.len(), 5);
//
//     // Check that all the subproofs were constructed correctly for each path
// and     // subpath
//     let path_one_as_vec = GroveDb::compress_subtree_key(&[TEST_LEAF,
// b"innertree"], None);     let path_two_as_vec =
// GroveDb::compress_subtree_key(&[ANOTHER_TEST_LEAF, b"innertree3"], None);
//     let path_three_as_vec =
//         GroveDb::compress_subtree_key(&[ANOTHER_TEST_LEAF, b"innertree2"],
// None);     let test_leaf_path_as_vec =
// GroveDb::compress_subtree_key([TEST_LEAF], None);
//     let another_test_leaf_path_as_vec =
// GroveDb::compress_subtree_key(&[ANOTHER_TEST_LEAF], None);
//
//     let proof_for_path_one = proof.proofs.get(&path_one_as_vec).unwrap();
//     let proof_for_path_two = proof.proofs.get(&path_two_as_vec).unwrap();
//     let proof_for_path_three = proof.proofs.get(&path_three_as_vec).unwrap();
//     let proof_for_test_leaf =
// proof.proofs.get(&test_leaf_path_as_vec).unwrap();
//     let proof_for_another_test_leaf =
// proof.proofs.get(&another_test_leaf_path_as_vec).unwrap();
//
//     // Assert path 1 proof
//     let mut proof_query = Query::new();
//     proof_query.insert_key(b"key1");
//     proof_query.insert_key(b"key2");
//     assert_eq!(
//         *proof_for_path_one,
//         inner_tree.prove(proof_query, None, None).unwrap()
//     );
//
//     // Assert path 2 proof
//     let mut proof_query = Query::new();
//     proof_query.insert_key(b"key4".to_vec());
//     assert_eq!(
//         *proof_for_path_two,
//         inner_tree_3.prove(proof_query, None, None).unwrap()
//     );
//
//     // Assert path 3 proof
//     let mut proof_query = Query::new();
//     proof_query.insert_key(b"key3");
//     assert_eq!(
//         *proof_for_path_three,
//         inner_tree_2.prove(proof_query, None, None).unwrap()
//     );
//
//     // Assert test leaf proof
//     let mut proof_query = Query::new();
//     proof_query.insert_key(b"innertree".to_vec());
//     assert_eq!(
//         *proof_for_test_leaf,
//         test_leaf.prove(proof_query, None, None).unwrap()
//     );
//
//     // Assert another test leaf proof
//     // another test leaf appeared in two path,
//     // hence it should contain proofs for both keys
//     let mut proof_query = Query::new();
//     proof_query.insert_key(b"innertree2".to_vec());
//     proof_query.insert_key(b"innertree3".to_vec());
//     assert_eq!(
//         *proof_for_another_test_leaf,
//         another_test_leaf
//             .prove(proof_query, None, None)
//             .unwrap()
//     );
//
//     // Check that the root proof is valid
//     // Root proof should contain proof for both test_leaf and
// another_test_leaf     let test_leaf_root_key =
// GroveDb::compress_subtree_key(&[], Some(TEST_LEAF));
//     let another_test_leaf_root_key = GroveDb::compress_subtree_key(&[],
// Some(ANOTHER_TEST_LEAF));     assert_eq!(
//         proof.root_proof,
//         root_tree
//             .proof(&[
//                 temp_db.root_leaf_keys[&test_leaf_root_key],
//                 temp_db.root_leaf_keys[&another_test_leaf_root_key],
//             ])
//             .to_bytes()
//     );
//
//     // Assert that we got the correct root leaf keys
//     assert_eq!(proof.root_leaf_keys.len(), 2);
//     assert_eq!(proof.root_leaf_keys[&test_leaf_root_key], 0);
//     assert_eq!(proof.root_leaf_keys[&another_test_leaf_root_key], 1);
// }

// #[test]
// fn test_successful_proof_verification() {
//     // Build a grovedb database
//     // Tree Structure
//     // root
//     //     test_leaf
//     //         innertree
//     //             k1,v1
//     //             k2,v2
//     //     another_test_leaf
//     //         innertree2
//     //             k3,v3
//     //         innertree3
//     //             k4,v4
//
//     // Insert elements into grovedb instance
//     let mut temp_db = make_grovedb();
//     // Insert level 1 nodes
//     temp_db
//         .insert(
//             [TEST_LEAF],
//             b"innertree",
//             Element::empty_tree(),
//             None,
//         )
//         .expect("successful subtree insert");
//     temp_db
//         .insert(
//             &[ANOTHER_TEST_LEAF],
//             b"innertree2",
//             Element::empty_tree(),
//             None,
//         )
//         .expect("successful subtree insert");
//     temp_db
//         .insert(
//             &[ANOTHER_TEST_LEAF],
//             b"innertree3",
//             Element::empty_tree(),
//             None,
//         )
//         .expect("successful subtree insert");
//     // Insert level 2 nodes
//     temp_db
//         .insert(
//             &[TEST_LEAF, b"innertree"],
//             b"key1",
//             Element::Item(b"value1".to_vec()),
//             None,
//         )
//         .expect("successful subtree insert");
//     temp_db
//         .insert(
//             &[TEST_LEAF, b"innertree"],
//             b"key2",
//             Element::Item(b"value2".to_vec()),
//             None,
//         )
//         .expect("successful subtree insert");
//     temp_db
//         .insert(
//             &[ANOTHER_TEST_LEAF, b"innertree2"],
//             b"key3",
//             Element::Item(b"value3".to_vec()),
//             None,
//         )
//         .expect("successful subtree insert");
//     temp_db
//         .insert(
//             &[ANOTHER_TEST_LEAF, b"innertree3"],
//             b"key4",
//             Element::Item(b"value4".to_vec()),
//             None,
//         )
//         .expect("successful subtree insert");
//
//     // Single query proof verification
//     let mut path_one_query = Query::new();
//     path_one_query.insert_key(b"key1");
//     path_one_query.insert_key(b"key2");
//
//     let proof = temp_db
//         .proof(vec![PathQuery::new_unsized(
//             &[TEST_LEAF, b"innertree"],
//             path_one_query,
//         )])
//         .unwrap();
//
//     // Assert correct root hash
//     let (root_hash, result_maps) = GroveDb::execute_proof(proof).unwrap();
//     assert_eq!(temp_db.root_tree.root().unwrap(), root_hash);
//
//     // Assert correct result object
//     // Proof query was for two keys key1 and key2
//     let path_as_vec = GroveDb::compress_subtree_key(&[TEST_LEAF,
// b"innertree"], None);     let result_map =
// result_maps.get(&path_as_vec).unwrap();     let elem_1: Element =
// bincode::deserialize(result_map.get(b"key1").unwrap().unwrap()).unwrap();
//     let elem_2: Element =
// bincode::deserialize(result_map.get(b"key2").unwrap().unwrap()).unwrap();
//     assert_eq!(elem_1, Element::Item(b"value1".to_vec()));
//     assert_eq!(elem_2, Element::Item(b"value2".to_vec()));
//
//     // Multi query proof verification
//     let mut path_two_query = Query::new();
//     path_two_query.insert_key(b"key4".to_vec());
//
//     let mut path_three_query = Query::new();
//     path_three_query.insert_key(b"key3");
//
//     // Get grovedb proof
//     let proof = temp_db
//         .proof(vec![
//             PathQuery::new_unsized(&[ANOTHER_TEST_LEAF, b"innertree3"],
// path_two_query),             PathQuery::new_unsized(&[ANOTHER_TEST_LEAF,
// b"innertree2"], path_three_query),         ])
//         .unwrap();
//
//     // Assert correct root hash
//     let (root_hash, result_maps) = GroveDb::execute_proof(proof).unwrap();
//     assert_eq!(temp_db.root_tree.root().unwrap(), root_hash);
//
//     // Assert correct result object
//     let path_one_as_vec = GroveDb::compress_subtree_key(&[ANOTHER_TEST_LEAF,
// b"innertree3"], None);     let result_map =
// result_maps.get(&path_one_as_vec).unwrap();     let elem: Element =
// bincode::deserialize(result_map.get(b"key4").unwrap().unwrap()).unwrap();
//     assert_eq!(elem, Element::Item(b"value4".to_vec()));
//
//     let path_two_as_vec = GroveDb::compress_subtree_key(&[ANOTHER_TEST_LEAF,
// b"innertree2"], None);     let result_map =
// result_maps.get(&path_two_as_vec).unwrap();     let elem: Element =
// bincode::deserialize(result_map.get(b"key3").unwrap().unwrap()).unwrap();
//     assert_eq!(elem, Element::Item(b"value3".to_vec()));
// }

// #[test]
// fn test_checkpoint() {
//     let mut db = make_grovedb();
//     let element1 = Element::Item(b"ayy".to_vec());
//
//     db.insert([], b"key1", Element::empty_tree())
//         .expect("cannot insert a subtree 1 into GroveDB");
//     db.insert([b"key1"], b"key2", Element::empty_tree())
//         .expect("cannot insert a subtree 2 into GroveDB");
//     db.insert([b"key1", b"key2"], b"key3", element1.clone())
//         .expect("cannot insert an item into GroveDB");
//
//     assert_eq!(
//         db.get([b"key1", b"key2"], b"key3")
//             .expect("cannot get from grovedb"),
//         element1
//     );
//
//     let checkpoint_tempdir = TempDir::new("checkpoint").expect("cannot open
// tempdir");     let mut checkpoint = db
//         .checkpoint(checkpoint_tempdir.path().join("checkpoint"))
//         .expect("cannot create a checkpoint");
//
//     assert_eq!(
//         db.get([b"key1", b"key2"], b"key3")
//             .expect("cannot get from grovedb"),
//         element1
//     );
//     assert_eq!(
//         checkpoint
//             .get(&[b"key1", b"key2"], b"key3")
//             .expect("cannot get from checkpoint"),
//         element1
//     );
//
//     let element2 = Element::Item(b"ayy2".to_vec());
//     let element3 = Element::Item(b"ayy3".to_vec());
//
//     checkpoint
//         .insert([b"key1"], b"key4", element2.clone())
//         .expect("cannot insert into checkpoint");
//
//     db.insert([b"key1"], b"key4", element3.clone())
//         .expect("cannot insert into GroveDB");
//
//     assert_eq!(
//         checkpoint
//             .get(&[b"key1"], b"key4")
//             .expect("cannot get from checkpoint"),
//         element2,
//     );
//
//     assert_eq!(
//         db.get([b"key1"], b"key4")
//             .expect("cannot get from GroveDB"),
//         element3
//     );
//
//     checkpoint
//         .insert([b"key1"], b"key5", element3.clone())
//         .expect("cannot insert into checkpoint");
//
//     db.insert([b"key1"], b"key6", element3.clone())
//         .expect("cannot insert into GroveDB");
//
//     assert!(matches!(
//         checkpoint.get(&[b"key1"], b"key6"),
//         Err(Error::InvalidPath(_))
//     ));
//
//     assert!(matches!(
//         db.get([b"key1"], b"key5"),
//         Err(Error::InvalidPath(_))
//     ));
// }

#[test]
fn test_insert_if_not_exists() {
    let db = make_grovedb();

    // Insert twice at the same path
    assert!(db
        .insert_if_not_exists([TEST_LEAF], b"key1", Element::empty_tree(), None)
        .expect("Provided valid path"));
    assert!(!db
        .insert_if_not_exists([TEST_LEAF], b"key1", Element::empty_tree(), None)
        .expect("Provided valid path"));

    // Should propagate errors from insertion
    let result = db.insert_if_not_exists(
        [TEST_LEAF, b"unknown"],
        b"key1",
        Element::empty_tree(),
        None,
    );
    assert!(matches!(result, Err(Error::InvalidPath(_))));
}

#[test]
fn test_insert_big_key() {
    let mut db = make_grovedb();

    // 256 bytes
    assert!(db
        .insert_if_not_exists([TEST_LEAF], b"db544499a468d972867e94fc615a697107e2fe75eed977ec089075884b2eade7c083cb389e7bf2e46df672ff03841961573145960e47d093b9c8fc9b77a2cf0edb544499a468d972867e94fc615a697107e2fe75eed977ec089075884b2eade7c083cb389e7bf2e46df672ff03841961573145960e47d093b9c8fc9b77a2cf0e", Element::empty_tree(), None)
        .expect("a key of size 256 should work"));
    // 512 bytes
    assert!(db
        .insert_if_not_exists([TEST_LEAF], b"db544499a468d972867e94fc615a697107e2fe75eed977ec089075884b2eade7c083cb389e7bf2e46df672ff03841961573145960e47d093b9c8fc9b77a2cf0edb544499a468d972867e94fc615a697107e2fe75eed977ec089075884b2eade7c083cb389e7bf2e46df672ff03841961573145960e47d093b9c8fc9b77a2cf0e3c7e361f621a3c07895308f3d2e21e7a1543ba66bcd411f68044c820d375981a8d3d87c9b7ae07f651ad7fd2084b141f42190fa063fb81cb62674f85333d2a04a84c67f40db785d042e6909f9b95056a43788832a85609eaf2667b62fca21e4d552e8cdd163ba0d808336cc894ec32c8714d09f819d36e87aa0b04fdc8828ba9", Element::empty_tree(), None)
        .expect("a key of size 512 should work"));
    // 1024 bytes
    assert!(db
        .insert_if_not_exists([TEST_LEAF], b"db544499a468d972867e94fc615a697107e2fe75eed977ec089075884b2eade7c083cb389e7bf2e46df672ff03841961573145960e47d093b9c8fc9b77a2cf0edb544499a468d972867e94fc615a697107e2fe75eed977ec089075884b2eade7c083cb389e7bf2e46df672ff03841961573145960e47d093b9c8fc9b77a2cf0e3c7e361f621a3c07895308f3d2e21e7a1543ba66bcd411f68044c820d375981a8d3d87c9b7ae07f651ad7fd2084b141f42190fa063fb81cb62674f85333d2a04a84c67f40db785d042e6909f9b95056a43788832a85609eaf2667b62fca21e4d552e8cdd163ba0d808336cc894ec32c8714d09f819d36e87aa0b04fdc8828ba9db544499a468d972867e94fc615a697107e2fe75eed977ec089075884b2eade7c083cb389e7bf2e46df672ff03841961573145960e47d093b9c8fc9b77a2cf0edb544499a468d972867e94fc615a697107e2fe75eed977ec089075884b2eade7c083cb389e7bf2e46df672ff03841961573145960e47d093b9c8fc9b77a2cf0e3c7e361f621a3c07895308f3d2e21e7a1543ba66bcd411f68044c820d375981a8d3d87c9b7ae07f651ad7fd2084b141f42190fa063fb81cb62674f85333d2a04a84c67f40db785d042e6909f9b95056a43788832a85609eaf2667b62fca21e4d552e8cdd163ba0d808336cc894ec32c8714d09f819d36e87aa0b04fdc8828ba9", Element::empty_tree(), None)
        .expect("a key of size 1024 should work"));

}

#[test]
#[should_panic]
fn test_insert_too_big_key() {
    let mut db = make_grovedb();
    // 1025 bytes
    db
        .insert_if_not_exists([TEST_LEAF], b"db544499a468d972867e94fc615a697107e2fe75eed977ec089075884b2eade7c083cb389e7bf2e46df672ff03841961573145960e47d093b9c8fc9b77a2cf0edb544499a468d972867e94fc615a697107e2fe75eed977ec089075884b2eade7c083cb389e7bf2e46df672ff03841961573145960e47d093b9c8fc9b77a2cf0e3c7e361f621a3c07895308f3d2e21e7a1543ba66bcd411f68044c820d375981a8d3d87c9b7ae07f651ad7fd2084b141f42190fa063fb81cb62674f85333d2a04a84c67f40db785d042e6909f9b95056a43788832a85609eaf2667b62fca21e4d552e8cdd163ba0d808336cc894ec32c8714d09f819d36e87aa0b04fdc8828ba9db544499a468d972867e94fc615a697107e2fe75eed977ec089075884b2eade7c083cb389e7bf2e46df672ff03841961573145960e47d093b9c8fc9b77a2cf0edb544499a468d972867e94fc615a697107e2fe75eed977ec089075884b2eade7c083cb389e7bf2e46df672ff03841961573145960e47d093b9c8fc9b77a2cf0e3c7e361f621a3c07895308f3d2e21e7a1543ba66bcd411f68044c820d375981a8d3d87c9b7ae07f651ad7fd2084b141f42190fa063fb81cb62674f85333d2a04a84c67f40db785d042e6909f9b95056a43788832a85609eaf2667b62fca21e4d552e8cdd163ba0d808336cc894ec32c8714d09f819d36e87aa0b04fdc8828ba95", Element::empty_tree(), None)
        .expect_err("a key of size 1025 should be too big");
}

#[test]
fn test_is_empty_tree() {
    let db = make_grovedb();

    // Create an empty tree with no elements
    db.insert([TEST_LEAF], b"innertree", Element::empty_tree(), None)
        .unwrap();

    assert!(db
        .is_empty_tree([TEST_LEAF, b"innertree"], None)
        .expect("path is valid tree"));

    // add an element to the tree to make it non empty
    db.insert(
        [TEST_LEAF, b"innertree"],
        b"key1",
        Element::Item(b"hello".to_vec()),
        None,
    )
    .unwrap();
    assert!(!db
        .is_empty_tree([TEST_LEAF, b"innertree"], None)
        .expect("path is valid tree"));
}

#[test]
fn transaction_insert_item_with_transaction_should_use_transaction() {
    let item_key = b"key3";

    let db = make_grovedb();
    let transaction = db.start_transaction();

    // Check that there's no such key in the DB
    let result = db.get([TEST_LEAF], item_key, None);
    assert!(matches!(result, Err(Error::PathKeyNotFound(_))));

    let element1 = Element::Item(b"ayy".to_vec());

    db.insert([TEST_LEAF], item_key, element1, Some(&transaction))
        .expect("cannot insert an item into GroveDB");

    // The key was inserted inside the transaction, so it shouldn't be
    // possible to get it back without committing or using transaction
    let result = db.get([TEST_LEAF], item_key, None);
    assert!(matches!(result, Err(Error::PathKeyNotFound(_))));
    // Check that the element can be retrieved when transaction is passed
    let result_with_transaction = db
        .get([TEST_LEAF], item_key, Some(&transaction))
        .expect("Expected to work");
    assert_eq!(result_with_transaction, Element::Item(b"ayy".to_vec()));

    // Test that commit works
    // transaction.commit();
    db.commit_transaction(transaction).unwrap();

    // Check that the change was committed
    let result = db
        .get([TEST_LEAF], item_key, None)
        .expect("Expected transaction to work");
    assert_eq!(result, Element::Item(b"ayy".to_vec()));
}

#[test]
fn transaction_insert_tree_with_transaction_should_use_transaction() {
    let subtree_key = b"subtree_key";

    let db = make_grovedb();
    let transaction = db.start_transaction();

    // Check that there's no such key in the DB
    let result = db.get([TEST_LEAF], subtree_key, None);
    assert!(matches!(result, Err(Error::PathKeyNotFound(_))));

    db.insert(
        [TEST_LEAF],
        subtree_key,
        Element::empty_tree(),
        Some(&transaction),
    )
    .expect("cannot insert an item into GroveDB");

    let result = db.get([TEST_LEAF], subtree_key, None);
    assert!(matches!(result, Err(Error::PathKeyNotFound(_))));

    let result_with_transaction = db
        .get([TEST_LEAF], subtree_key, Some(&transaction))
        .expect("Expected to work");
    assert_eq!(result_with_transaction, Element::empty_tree());

    db.commit_transaction(transaction).unwrap();

    let result = db
        .get([TEST_LEAF], subtree_key, None)
        .expect("Expected transaction to work");
    assert_eq!(result, Element::empty_tree());
}

#[test]
fn transaction_should_be_aborted_when_rollback_is_called() {
    let item_key = b"key3";

    let db = make_grovedb();
    let transaction = db.start_transaction();

    let element1 = Element::Item(b"ayy".to_vec());

    let result = db.insert([TEST_LEAF], item_key, element1, Some(&transaction));

    assert!(matches!(result, Ok(())));

    db.rollback_transaction(&transaction).unwrap();

    let result = db.get([TEST_LEAF], item_key, Some(&transaction));
    assert!(matches!(result, Err(Error::PathKeyNotFound(_))));
}

#[test]
fn transaction_should_be_aborted() {
    let db = make_grovedb();
    let transaction = db.start_transaction();

    let item_key = b"key3";
    let element = Element::Item(b"ayy".to_vec());

    db.insert([TEST_LEAF], item_key, element, Some(&transaction))
        .unwrap();

    drop(transaction);

    // Transactional data shouldn't be committed to the main database
    let result = db.get([TEST_LEAF], item_key, None);
    assert!(matches!(result, Err(Error::PathKeyNotFound(_))));
}

#[test]
fn test_subtree_pairs_iterator() {
    let db = make_grovedb();
    let element = Element::Item(b"ayy".to_vec());
    let element2 = Element::Item(b"lmao".to_vec());

    // Insert some nested subtrees
    db.insert([TEST_LEAF], b"subtree1", Element::empty_tree(), None)
        .expect("successful subtree 1 insert");
    db.insert(
        [TEST_LEAF, b"subtree1"],
        b"subtree11",
        Element::empty_tree(),
        None,
    )
    .expect("successful subtree 2 insert");
    // Insert an element into subtree
    db.insert(
        [TEST_LEAF, b"subtree1", b"subtree11"],
        b"key1",
        element.clone(),
        None,
    )
    .expect("successful value insert");
    assert_eq!(
        db.get([TEST_LEAF, b"subtree1", b"subtree11"], b"key1", None)
            .expect("successful get 1"),
        element
    );
    db.insert(
        [TEST_LEAF, b"subtree1", b"subtree11"],
        b"key0",
        element.clone(),
        None,
    )
    .expect("successful value insert");
    db.insert(
        [TEST_LEAF, b"subtree1"],
        b"subtree12",
        Element::empty_tree(),
        None,
    )
    .expect("successful subtree 3 insert");
    db.insert([TEST_LEAF, b"subtree1"], b"key1", element.clone(), None)
        .expect("successful value insert");
    db.insert([TEST_LEAF, b"subtree1"], b"key2", element2.clone(), None)
        .expect("successful value insert");

    // Iterate over subtree1 to see if keys of other subtrees messed up
    // let mut iter = db
    //     .elements_iterator(&[TEST_LEAF, b"subtree1"], None)
    //     .expect("cannot create iterator");
    let storage_context = db.db.db.get_storage_context([TEST_LEAF, b"subtree1"]);
    let mut iter = Element::iterator(storage_context.raw_iter());
    assert_eq!(iter.next().unwrap(), Some((b"key1".to_vec(), element)));
    assert_eq!(iter.next().unwrap(), Some((b"key2".to_vec(), element2)));
    let subtree_element = iter.next().unwrap().unwrap();
    assert_eq!(subtree_element.0, b"subtree11".to_vec());
    assert!(matches!(subtree_element.1, Element::Tree(_)));
    let subtree_element = iter.next().unwrap().unwrap();
    assert_eq!(subtree_element.0, b"subtree12".to_vec());
    assert!(matches!(subtree_element.1, Element::Tree(_)));
    assert!(matches!(iter.next(), Ok(None)));
}

#[test]
fn test_element_deletion() {
    let db = make_grovedb();
    let element = Element::Item(b"ayy".to_vec());
    db.insert([TEST_LEAF], b"key", element, None)
        .expect("successful insert");
    let root_hash = db.root_hash(None).unwrap();
    assert!(db.delete([TEST_LEAF], b"key", None).is_ok());
    assert!(matches!(
        db.get([TEST_LEAF], b"key", None),
        Err(Error::PathKeyNotFound(_))
    ));
    assert_ne!(root_hash, db.root_hash(None).unwrap());
}

#[test]
fn test_find_subtrees() {
    let element = Element::Item(b"ayy".to_vec());
    let db = make_grovedb();
    // Insert some nested subtrees
    db.insert([TEST_LEAF], b"key1", Element::empty_tree(), None)
        .expect("successful subtree 1 insert");
    db.insert([TEST_LEAF, b"key1"], b"key2", Element::empty_tree(), None)
        .expect("successful subtree 2 insert");
    // Insert an element into subtree
    db.insert([TEST_LEAF, b"key1", b"key2"], b"key3", element, None)
        .expect("successful value insert");
    db.insert([TEST_LEAF], b"key4", Element::empty_tree(), None)
        .expect("successful subtree 3 insert");
    let subtrees = db
        .find_subtrees(vec![TEST_LEAF], None)
        .expect("cannot get subtrees");
    assert_eq!(
        vec![
            vec![TEST_LEAF],
            vec![TEST_LEAF, b"key1"],
            vec![TEST_LEAF, b"key4"],
            vec![TEST_LEAF, b"key1", b"key2"],
        ],
        subtrees
    );
}

#[test]
fn test_get_subtree() {
    let db = make_grovedb();
    let element = Element::Item(b"ayy".to_vec());

    // Returns error is subtree is not valid
    {
        let subtree = db.get([TEST_LEAF], b"invalid_tree", None);
        assert!(subtree.is_err());

        // Doesn't return an error for subtree that exists but empty
        let subtree = db.get([], TEST_LEAF, None);
        assert!(subtree.is_ok());
    }
    // Insert some nested subtrees
    db.insert([TEST_LEAF], b"key1", Element::empty_tree(), None)
        .expect("successful subtree 1 insert");

    db.insert([TEST_LEAF, b"key1"], b"key2", Element::empty_tree(), None)
        .expect("successful subtree 2 insert");

    // Insert an element into subtree
    db.insert(
        [TEST_LEAF, b"key1", b"key2"],
        b"key3",
        element.clone(),
        None,
    )
    .expect("successful value insert");
    db.insert([TEST_LEAF], b"key4", Element::empty_tree(), None)
        .expect("successful subtree 3 insert");

    // Retrieve subtree instance
    // Check if it returns the same instance that was inserted
    {
        let subtree_storage = db.db.db.get_storage_context([TEST_LEAF, b"key1", b"key2"]);
        let subtree = Merk::open(subtree_storage).expect("cannot open merk");
        let result_element = Element::get(&subtree, b"key3").unwrap();
        assert_eq!(result_element, Element::Item(b"ayy".to_vec()));
    }
    // Insert a new tree with transaction
    let transaction = db.start_transaction();

    db.insert(
        [TEST_LEAF, b"key1"],
        b"innertree",
        Element::empty_tree(),
        Some(&transaction),
    )
    .expect("successful subtree insert");

    db.insert(
        [TEST_LEAF, b"key1", b"innertree"],
        b"key4",
        element,
        Some(&transaction),
    )
    .expect("successful value insert");

    // Retrieve subtree instance with transaction
    let subtree_storage = db
        .db
        .db
        .get_transactional_storage_context([TEST_LEAF, b"key1", b"innertree"], &transaction);
    let subtree = Merk::open(subtree_storage).expect("cannot open merk");
    let result_element = Element::get(&subtree, b"key4").unwrap();
    assert_eq!(result_element, Element::Item(b"ayy".to_vec()));

    // Should be able to retrieve instances created before transaction
    let subtree_storage = db.db.db.get_storage_context([TEST_LEAF, b"key1", b"key2"]);
    let subtree = Merk::open(subtree_storage).expect("cannot open merk");
    let result_element = Element::get(&subtree, b"key3").unwrap();
    assert_eq!(result_element, Element::Item(b"ayy".to_vec()));
}

#[test]
fn test_subtree_deletion() {
    let element = Element::Item(b"ayy".to_vec());
    let db = make_grovedb();
    // Insert some nested subtrees
    db.insert([TEST_LEAF], b"key1", Element::empty_tree(), None)
        .expect("successful subtree 1 insert");
    db.insert([TEST_LEAF, b"key1"], b"key2", Element::empty_tree(), None)
        .expect("successful subtree 2 insert");
    // Insert an element into subtree
    db.insert([TEST_LEAF, b"key1", b"key2"], b"key3", element, None)
        .expect("successful value insert");
    db.insert([TEST_LEAF], b"key4", Element::empty_tree(), None)
        .expect("successful subtree 3 insert");

    let root_hash = db.root_hash(None).unwrap();
    db.delete([TEST_LEAF], b"key1", None)
        .expect("unable to delete subtree");
    assert!(matches!(
        db.get([TEST_LEAF, b"key1", b"key2"], b"key3", None),
        Err(Error::PathNotFound(_))
    ));
    // assert_eq!(db.subtrees.len(), 3); // TEST_LEAF, ANOTHER_TEST_LEAF
    // TEST_LEAF.key4 stay
    assert!(db.get([], TEST_LEAF, None).is_ok());
    assert!(db.get([], ANOTHER_TEST_LEAF, None).is_ok());
    assert!(db.get([TEST_LEAF], b"key4", None).is_ok());
    assert_ne!(root_hash, db.root_hash(None).unwrap());
}

#[test]
fn test_subtree_deletion_if_empty() {
    let element = Element::Item(b"value".to_vec());
    let db = make_grovedb();

    let transaction = db.start_transaction();

    // Insert some nested subtrees
    db.insert(
        [TEST_LEAF],
        b"level1-A",
        Element::empty_tree(),
        Some(&transaction),
    )
    .expect("successful subtree insert A on level 1");
    db.insert(
        [TEST_LEAF, b"level1-A"],
        b"level2-A",
        Element::empty_tree(),
        Some(&transaction),
    )
    .expect("successful subtree insert A on level 2");
    db.insert(
        [TEST_LEAF, b"level1-A"],
        b"level2-B",
        Element::empty_tree(),
        Some(&transaction),
    )
    .expect("successful subtree insert B on level 2");
    // Insert an element into subtree
    db.insert(
        [TEST_LEAF, b"level1-A", b"level2-A"],
        b"level3-A",
        element,
        Some(&transaction),
    )
    .expect("successful value insert");
    db.insert(
        [TEST_LEAF],
        b"level1-B",
        Element::empty_tree(),
        Some(&transaction),
    )
    .expect("successful subtree insert B on level 1");

    db.commit_transaction(transaction)
        .expect("cannot commit changes");

    // Currently we have:
    // Level 1:            A
    //                    / \
    // Level 2:          A   B
    //                   |
    // Level 3:          A: value

    let transaction = db.start_transaction();

    let deleted = db
        .delete_if_empty_tree([TEST_LEAF], b"level1-A", Some(&transaction))
        .expect("unable to delete subtree");
    assert!(!deleted);

    let deleted = db
        .delete_up_tree_while_empty(
            [TEST_LEAF, b"level1-A", b"level2-A"],
            b"level3-A",
            Some(0),
            Some(&transaction),
        )
        .expect("unable to delete subtree");
    assert_eq!(deleted, 2);

    assert!(matches!(
        db.get(
            [TEST_LEAF, b"level1-A", b"level2-A"],
            b"level3-A",
            Some(&transaction)
        ),
        Err(Error::PathNotFound(_))
    ));

    assert!(matches!(
        db.get([TEST_LEAF, b"level1-A"], b"level2-A", Some(&transaction)),
        Err(Error::PathKeyNotFound(_))
    ));

    assert!(matches!(
        db.get([TEST_LEAF], b"level1-A", Some(&transaction)),
        Ok(Element::Tree(_)),
    ));
}

#[test]
fn test_subtree_deletion_if_empty_without_transaction() {
    let element = Element::Item(b"value".to_vec());
    let db = make_grovedb();

    // Insert some nested subtrees
    db.insert([TEST_LEAF], b"level1-A", Element::empty_tree(), None)
        .expect("successful subtree insert A on level 1");
    db.insert(
        [TEST_LEAF, b"level1-A"],
        b"level2-A",
        Element::empty_tree(),
        None,
    )
    .expect("successful subtree insert A on level 2");
    db.insert(
        [TEST_LEAF, b"level1-A"],
        b"level2-B",
        Element::empty_tree(),
        None,
    )
    .expect("successful subtree insert B on level 2");
    // Insert an element into subtree
    db.insert(
        [TEST_LEAF, b"level1-A", b"level2-A"],
        b"level3-A",
        element,
        None,
    )
    .expect("successful value insert");
    db.insert([TEST_LEAF], b"level1-B", Element::empty_tree(), None)
        .expect("successful subtree insert B on level 1");

    // Currently we have:
    // Level 1:            A
    //                    / \
    // Level 2:          A   B
    //                   |
    // Level 3:          A: value

    let deleted = db
        .delete_if_empty_tree([TEST_LEAF], b"level1-A", None)
        .expect("unable to delete subtree");
    assert!(!deleted);

    let deleted = db
        .delete_up_tree_while_empty(
            [TEST_LEAF, b"level1-A", b"level2-A"],
            b"level3-A",
            Some(0),
            None,
        )
        .expect("unable to delete subtree");
    assert_eq!(deleted, 2);

    assert!(matches!(
        db.get([TEST_LEAF, b"level1-A", b"level2-A"], b"level3-A", None,),
        Err(Error::PathNotFound(_))
    ));

    assert!(matches!(
        db.get([TEST_LEAF, b"level1-A"], b"level2-A", None),
        Err(Error::PathKeyNotFound(_))
    ));

    assert!(matches!(
        db.get([TEST_LEAF], b"level1-A", None),
        Ok(Element::Tree(_)),
    ));
}

#[test]
fn test_get_full_query() {
    let db = make_grovedb();

    // Insert a couple of subtrees first
    db.insert([TEST_LEAF], b"key1", Element::empty_tree(), None)
        .expect("successful subtree insert");
    db.insert([TEST_LEAF], b"key2", Element::empty_tree(), None)
        .expect("successful subtree insert");
    // Insert some elements into subtree
    db.insert(
        [TEST_LEAF, b"key1"],
        b"key3",
        Element::Item(b"ayya".to_vec()),
        None,
    )
    .expect("successful value insert");
    db.insert(
        [TEST_LEAF, b"key1"],
        b"key4",
        Element::Item(b"ayyb".to_vec()),
        None,
    )
    .expect("successful value insert");
    db.insert(
        [TEST_LEAF, b"key1"],
        b"key5",
        Element::Item(b"ayyc".to_vec()),
        None,
    )
    .expect("successful value insert");
    db.insert(
        [TEST_LEAF, b"key2"],
        b"key6",
        Element::Item(b"ayyd".to_vec()),
        None,
    )
    .expect("successful value insert");

    let path1 = vec![TEST_LEAF.to_vec(), b"key1".to_vec()];
    let path2 = vec![TEST_LEAF.to_vec(), b"key2".to_vec()];
    let mut query1 = Query::new();
    let mut query2 = Query::new();
    query1.insert_range_inclusive(b"key3".to_vec()..=b"key4".to_vec());
    query2.insert_key(b"key6".to_vec());

    let path_query1 = PathQuery::new_unsized(path1, query1);
    let path_query2 = PathQuery::new_unsized(path2, query2);

    assert_eq!(
        db.get_path_queries_raw(&[&path_query1, &path_query2], None)
            .expect("expected successful get_query"),
        vec![
            subtree::Element::Item(b"ayya".to_vec()),
            subtree::Element::Item(b"ayyb".to_vec()),
            subtree::Element::Item(b"ayyd".to_vec()),
        ]
    );
}

#[test]
fn test_aux_uses_separate_cf() {
    let element = Element::Item(b"ayy".to_vec());
    let db = make_grovedb();
    // Insert some nested subtrees
    db.insert([TEST_LEAF], b"key1", Element::empty_tree(), None)
        .expect("successful subtree 1 insert");
    db.insert([TEST_LEAF, b"key1"], b"key2", Element::empty_tree(), None)
        .expect("successful subtree 2 insert");
    // Insert an element into subtree
    db.insert(
        [TEST_LEAF, b"key1", b"key2"],
        b"key3",
        element.clone(),
        None,
    )
    .expect("successful value insert");

    db.put_aux(b"key1", b"a", None).expect("cannot put aux");
    db.put_aux(b"key2", b"b", None).expect("cannot put aux");
    db.put_aux(b"key3", b"c", None).expect("cannot put aux");
    db.delete_aux(b"key3", None)
        .expect("cannot delete from aux");

    assert_eq!(
        db.get([TEST_LEAF, b"key1", b"key2"], b"key3", None)
            .expect("cannot get element"),
        element
    );
    assert_eq!(
        db.get_aux(b"key1", None).expect("cannot get from aux"),
        Some(b"a".to_vec())
    );
    assert_eq!(
        db.get_aux(b"key2", None).expect("cannot get from aux"),
        Some(b"b".to_vec())
    );
    assert_eq!(
        db.get_aux(b"key3", None).expect("cannot get from aux"),
        None
    );
    assert_eq!(
        db.get_aux(b"key4", None).expect("cannot get from aux"),
        None
    );
}

#[test]
fn test_aux_with_transaction() {
    let element = Element::Item(b"ayy".to_vec());
    let aux_value = b"ayylmao".to_vec();
    let key = b"key".to_vec();
    let db = make_grovedb();
    let transaction = db.start_transaction();

    // Insert a regular data with aux data in the same transaction
    db.insert([TEST_LEAF], &key, element, Some(&transaction))
        .expect("unable to insert");
    db.put_aux(&key, &aux_value, Some(&transaction))
        .expect("unable to insert aux value");
    assert_eq!(
        db.get_aux(&key, Some(&transaction))
            .expect("unable to get aux value"),
        Some(aux_value.clone())
    );
    // Cannot reach the data outside of transaction
    assert_eq!(
        db.get_aux(&key, None).expect("unable to get aux value"),
        None
    );
    // And should be able to get data when committed
    db.commit_transaction(transaction)
        .expect("unable to commit transaction");
    assert_eq!(
        db.get_aux(&key, None)
            .expect("unable to get committed aux value"),
        Some(aux_value)
    );
}

fn populate_tree_for_non_unique_range_subquery(db: &TempGroveDb) {
    // Insert a couple of subtrees first
    for i in 1985u32..2000 {
        let i_vec = (i as u32).to_be_bytes().to_vec();
        db.insert([TEST_LEAF], &i_vec, Element::empty_tree(), None)
            .expect("successful subtree insert");
        // Insert element 0
        // Insert some elements into subtree
        db.insert(
            [TEST_LEAF, i_vec.as_slice()],
            b"\0",
            Element::empty_tree(),
            None,
        )
        .expect("successful subtree insert");

        for j in 100u32..150 {
            let mut j_vec = i_vec.clone();
            j_vec.append(&mut (j as u32).to_be_bytes().to_vec());
            db.insert(
                [TEST_LEAF, i_vec.as_slice(), b"\0"],
                &j_vec.clone(),
                Element::Item(j_vec),
                None,
            )
            .expect("successful value insert");
        }
    }
}

fn populate_tree_for_non_unique_double_range_subquery(db: &TempGroveDb) {
    // Insert a couple of subtrees first
    for i in 0u32..10 {
        let i_vec = (i as u32).to_be_bytes().to_vec();
        db.insert([TEST_LEAF], &i_vec, Element::empty_tree(), None)
            .expect("successful subtree insert");
        // Insert element 0
        // Insert some elements into subtree
        db.insert(
            [TEST_LEAF, i_vec.as_slice()],
            b"a",
            Element::empty_tree(),
            None,
        )
        .expect("successful subtree insert");

        for j in 25u32..50 {
            let j_vec = (j as u32).to_be_bytes().to_vec();
            db.insert(
                [TEST_LEAF, i_vec.as_slice(), b"a"],
                &j_vec,
                Element::empty_tree(),
                None,
            )
            .expect("successful value insert");

            // Insert element 0
            // Insert some elements into subtree
            db.insert(
                [TEST_LEAF, i_vec.as_slice(), b"a", j_vec.as_slice()],
                b"\0",
                Element::empty_tree(),
                None,
            )
            .expect("successful subtree insert");

            for k in 100u32..110 {
                let k_vec = (k as u32).to_be_bytes().to_vec();
                db.insert(
                    [TEST_LEAF, i_vec.as_slice(), b"a", &j_vec, b"\0"],
                    &k_vec.clone(),
                    Element::Item(k_vec),
                    None,
                )
                .expect("successful value insert");
            }
        }
    }
}

fn populate_tree_by_reference_for_non_unique_range_subquery(db: &TempGroveDb) {
    // This subtree will be holding values
    db.insert([TEST_LEAF], b"\0", Element::empty_tree(), None)
        .expect("successful subtree insert");

    // This subtree will be holding references
    db.insert([TEST_LEAF], b"1", Element::empty_tree(), None)
        .expect("successful subtree insert");
    // Insert a couple of subtrees first
    for i in 1985u32..2000 {
        let i_vec = (i as u32).to_be_bytes().to_vec();
        db.insert([TEST_LEAF, b"1"], &i_vec, Element::empty_tree(), None)
            .expect("successful subtree insert");
        // Insert element 0
        // Insert some elements into subtree
        db.insert(
            [TEST_LEAF, b"1", i_vec.as_slice()],
            b"\0",
            Element::empty_tree(),
            None,
        )
        .expect("successful subtree insert");

        for j in 100u32..150 {
            let random_key = rand::thread_rng().gen::<[u8; 32]>();
            let mut j_vec = i_vec.clone();
            j_vec.append(&mut (j as u32).to_be_bytes().to_vec());

            // We should insert every item to the tree holding items
            db.insert(
                [TEST_LEAF, b"\0"],
                &random_key,
                Element::Item(j_vec.clone()),
                None,
            )
            .expect("successful value insert");

            db.insert(
                [TEST_LEAF, b"1", i_vec.clone().as_slice(), b"\0"],
                &random_key,
                Element::Reference(vec![
                    TEST_LEAF.to_vec(),
                    b"\0".to_vec(),
                    random_key.to_vec(),
                ]),
                None,
            )
            .expect("successful value insert");
        }
    }
}

fn populate_tree_for_unique_range_subquery(db: &TempGroveDb) {
    // Insert a couple of subtrees first
    for i in 1985u32..2000 {
        let i_vec = (i as u32).to_be_bytes().to_vec();
        db.insert([TEST_LEAF], &i_vec, Element::empty_tree(), None)
            .expect("successful subtree insert");

        db.insert(
            [TEST_LEAF, &i_vec.clone()],
            b"\0",
            Element::Item(i_vec),
            None,
        )
        .expect("successful value insert");
    }
}

fn populate_tree_by_reference_for_unique_range_subquery(db: &TempGroveDb) {
    // This subtree will be holding values
    db.insert([TEST_LEAF], b"\0", Element::empty_tree(), None)
        .expect("successful subtree insert");

    // This subtree will be holding references
    db.insert([TEST_LEAF], b"1", Element::empty_tree(), None)
        .expect("successful subtree insert");

    for i in 1985u32..2000 {
        let i_vec = (i as u32).to_be_bytes().to_vec();
        db.insert([TEST_LEAF, b"1"], &i_vec, Element::empty_tree(), None)
            .expect("successful subtree insert");

        // We should insert every item to the tree holding items
        db.insert(
            [TEST_LEAF, b"\0"],
            &i_vec,
            Element::Item(i_vec.clone()),
            None,
        )
        .expect("successful value insert");

        // We should insert a reference to the item
        db.insert(
            [TEST_LEAF, b"1", i_vec.clone().as_slice()],
            b"\0",
            Element::Reference(vec![TEST_LEAF.to_vec(), b"\0".to_vec(), i_vec.clone()]),
            None,
        )
        .expect("successful value insert");
    }
}

fn populate_tree_for_unique_range_subquery_with_non_unique_null_values(db: &mut TempGroveDb) {
    populate_tree_for_unique_range_subquery(db);
    db.insert([TEST_LEAF], &[], Element::empty_tree(), None)
        .expect("successful subtree insert");
    db.insert([TEST_LEAF, &[]], b"\0", Element::empty_tree(), None)
        .expect("successful subtree insert");
    // Insert a couple of subtrees first
    for i in 100u32..200 {
        let i_vec = (i as u32).to_be_bytes().to_vec();
        db.insert(
            [TEST_LEAF, &[], b"\0"],
            &i_vec,
            Element::Item(i_vec.clone()),
            None,
        )
        .expect("successful value insert");
    }
}

#[test]
fn test_get_range_query_with_non_unique_subquery() {
    let db = make_grovedb();
    populate_tree_for_non_unique_range_subquery(&db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new();
    query.insert_range(1988_u32.to_be_bytes().to_vec()..1992_u32.to_be_bytes().to_vec());

    let subquery_key: Vec<u8> = b"\0".to_vec();
    let mut subquery = Query::new();
    subquery.insert_all();

    query.set_subquery_key(subquery_key);
    query.set_subquery(subquery);

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 200);

    let mut first_value = 1988_u32.to_be_bytes().to_vec();
    first_value.append(&mut 100_u32.to_be_bytes().to_vec());
    assert_eq!(elements[0], first_value);

    let mut last_value = 1991_u32.to_be_bytes().to_vec();
    last_value.append(&mut 149_u32.to_be_bytes().to_vec());
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_query_with_unique_subquery() {
    let mut db = make_grovedb();
    populate_tree_for_unique_range_subquery(&mut db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new();
    query.insert_range(1988_u32.to_be_bytes().to_vec()..1992_u32.to_be_bytes().to_vec());

    let subquery_key: Vec<u8> = b"\0".to_vec();

    query.set_subquery_key(subquery_key);

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 4);

    let first_value = 1988_u32.to_be_bytes().to_vec();
    assert_eq!(elements[0], first_value);

    let last_value = 1991_u32.to_be_bytes().to_vec();
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_query_with_unique_subquery_on_references() {
    let db = make_grovedb();
    populate_tree_by_reference_for_unique_range_subquery(&db);

    let path = vec![TEST_LEAF.to_vec(), b"1".to_vec()];
    let mut query = Query::new();
    query.insert_range(1988_u32.to_be_bytes().to_vec()..1992_u32.to_be_bytes().to_vec());

    let subquery_key: Vec<u8> = b"\0".to_vec();

    query.set_subquery_key(subquery_key);

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 4);

    let first_value = 1988_u32.to_be_bytes().to_vec();
    assert_eq!(elements[0], first_value);

    let last_value = 1991_u32.to_be_bytes().to_vec();
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_query_with_unique_subquery_with_non_unique_null_values() {
    let mut db = make_grovedb();
    populate_tree_for_unique_range_subquery_with_non_unique_null_values(&mut db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new();
    query.insert_all();

    let subquery_key: Vec<u8> = b"\0".to_vec();

    query.set_subquery_key(subquery_key);

    let mut subquery = Query::new();
    subquery.insert_all();

    query.add_conditional_subquery(
        QueryItem::Key(b"".to_vec()),
        Some(b"\0".to_vec()),
        Some(subquery),
    );

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 115);

    let first_value = 100_u32.to_be_bytes().to_vec();
    assert_eq!(elements[0], first_value);

    let last_value = 1999_u32.to_be_bytes().to_vec();
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_query_with_unique_subquery_ignore_non_unique_null_values() {
    let mut db = make_grovedb();
    populate_tree_for_unique_range_subquery_with_non_unique_null_values(&mut db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new();
    query.insert_all();

    let subquery_key: Vec<u8> = b"\0".to_vec();

    query.set_subquery_key(subquery_key);

    let subquery = Query::new();

    query.add_conditional_subquery(
        QueryItem::Key(b"".to_vec()),
        Some(b"\0".to_vec()),
        Some(subquery),
    );

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 15);

    let first_value = 1985_u32.to_be_bytes().to_vec();
    assert_eq!(elements[0], first_value);

    let last_value = 1999_u32.to_be_bytes().to_vec();
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_inclusive_query_with_non_unique_subquery() {
    let db = make_grovedb();
    populate_tree_for_non_unique_range_subquery(&db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new();
    query.insert_range_inclusive(1988_u32.to_be_bytes().to_vec()..=1995_u32.to_be_bytes().to_vec());

    let subquery_key: Vec<u8> = b"\0".to_vec();
    let mut subquery = Query::new();
    subquery.insert_all();

    query.set_subquery_key(subquery_key);
    query.set_subquery(subquery);

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 400);

    let mut first_value = 1988_u32.to_be_bytes().to_vec();
    first_value.append(&mut 100_u32.to_be_bytes().to_vec());
    assert_eq!(elements[0], first_value);

    let mut last_value = 1995_u32.to_be_bytes().to_vec();
    last_value.append(&mut 149_u32.to_be_bytes().to_vec());
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_inclusive_query_with_non_unique_subquery_on_references() {
    let db = make_grovedb();
    populate_tree_by_reference_for_non_unique_range_subquery(&db);

    let path = vec![TEST_LEAF.to_vec(), b"1".to_vec()];
    let mut query = Query::new();
    query.insert_range_inclusive(1988_u32.to_be_bytes().to_vec()..=1995_u32.to_be_bytes().to_vec());

    let subquery_key: Vec<u8> = b"\0".to_vec();
    let mut subquery = Query::new();
    subquery.insert_all();

    query.set_subquery_key(subquery_key);
    query.set_subquery(subquery);

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 400);

    let mut first_value = 1988_u32.to_be_bytes().to_vec();
    first_value.append(&mut 100_u32.to_be_bytes().to_vec());
    assert!(elements.contains(&first_value));

    let mut last_value = 1995_u32.to_be_bytes().to_vec();
    last_value.append(&mut 149_u32.to_be_bytes().to_vec());
    assert!(elements.contains(&last_value));
}

#[test]
fn test_get_range_inclusive_query_with_unique_subquery() {
    let db = make_grovedb();
    populate_tree_for_unique_range_subquery(&db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new();
    query.insert_range_inclusive(1988_u32.to_be_bytes().to_vec()..=1995_u32.to_be_bytes().to_vec());

    let subquery_key: Vec<u8> = b"\0".to_vec();

    query.set_subquery_key(subquery_key);

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 8);

    let first_value = 1988_u32.to_be_bytes().to_vec();
    assert_eq!(elements[0], first_value);

    let last_value = 1995_u32.to_be_bytes().to_vec();
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_from_query_with_non_unique_subquery() {
    let db = make_grovedb();
    populate_tree_for_non_unique_range_subquery(&db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new();
    query.insert_range_from(1995_u32.to_be_bytes().to_vec()..);

    let subquery_key: Vec<u8> = b"\0".to_vec();
    let mut subquery = Query::new();
    subquery.insert_all();

    query.set_subquery_key(subquery_key);
    query.set_subquery(subquery);

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 250);

    let mut first_value = 1995_u32.to_be_bytes().to_vec();
    first_value.append(&mut 100_u32.to_be_bytes().to_vec());
    assert_eq!(elements[0], first_value);

    let mut last_value = 1999_u32.to_be_bytes().to_vec();
    last_value.append(&mut 149_u32.to_be_bytes().to_vec());
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_from_query_with_unique_subquery() {
    let db = make_grovedb();
    populate_tree_for_unique_range_subquery(&db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new();
    query.insert_range_from(1995_u32.to_be_bytes().to_vec()..);

    let subquery_key: Vec<u8> = b"\0".to_vec();

    query.set_subquery_key(subquery_key);

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 5);

    let first_value = 1995_u32.to_be_bytes().to_vec();
    assert_eq!(elements[0], first_value);

    let last_value = 1999_u32.to_be_bytes().to_vec();
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_to_query_with_non_unique_subquery() {
    let db = make_grovedb();
    populate_tree_for_non_unique_range_subquery(&db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new();
    query.insert_range_to(..1995_u32.to_be_bytes().to_vec());

    let subquery_key: Vec<u8> = b"\0".to_vec();
    let mut subquery = Query::new();
    subquery.insert_all();

    query.set_subquery_key(subquery_key);
    query.set_subquery(subquery);

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 500);

    let mut first_value = 1985_u32.to_be_bytes().to_vec();
    first_value.append(&mut 100_u32.to_be_bytes().to_vec());
    assert_eq!(elements[0], first_value);

    let mut last_value = 1994_u32.to_be_bytes().to_vec();
    last_value.append(&mut 149_u32.to_be_bytes().to_vec());
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_to_query_with_unique_subquery() {
    let db = make_grovedb();
    populate_tree_for_unique_range_subquery(&db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new();
    query.insert_range_to(..1995_u32.to_be_bytes().to_vec());

    let subquery_key: Vec<u8> = b"\0".to_vec();

    query.set_subquery_key(subquery_key);

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 10);

    let first_value = 1985_u32.to_be_bytes().to_vec();
    assert_eq!(elements[0], first_value);

    let last_value = 1994_u32.to_be_bytes().to_vec();
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_to_inclusive_query_with_non_unique_subquery() {
    let db = make_grovedb();
    populate_tree_for_non_unique_range_subquery(&db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new();
    query.insert_range_to_inclusive(..=1995_u32.to_be_bytes().to_vec());

    let subquery_key: Vec<u8> = b"\0".to_vec();
    let mut subquery = Query::new();
    subquery.insert_all();

    query.set_subquery_key(subquery_key);
    query.set_subquery(subquery);

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 550);

    let mut first_value = 1985_u32.to_be_bytes().to_vec();
    first_value.append(&mut 100_u32.to_be_bytes().to_vec());
    assert_eq!(elements[0], first_value);

    let mut last_value = 1995_u32.to_be_bytes().to_vec();
    last_value.append(&mut 149_u32.to_be_bytes().to_vec());
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_to_inclusive_query_with_non_unique_subquery_and_key_out_of_bounds() {
    let db = make_grovedb();
    populate_tree_for_non_unique_range_subquery(&db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new_with_direction(false);
    query.insert_range_to_inclusive(..=5000_u32.to_be_bytes().to_vec());

    let subquery_key: Vec<u8> = b"\0".to_vec();
    let mut subquery = Query::new_with_direction(false);
    subquery.insert_all();

    query.set_subquery_key(subquery_key);
    query.set_subquery(subquery);

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 750);

    let mut first_value = 1999_u32.to_be_bytes().to_vec();
    first_value.append(&mut 149_u32.to_be_bytes().to_vec());
    assert_eq!(elements[0], first_value);

    let mut last_value = 1985_u32.to_be_bytes().to_vec();
    last_value.append(&mut 100_u32.to_be_bytes().to_vec());
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_to_inclusive_query_with_unique_subquery() {
    let db = make_grovedb();
    populate_tree_for_unique_range_subquery(&db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new();
    query.insert_range_to_inclusive(..=1995_u32.to_be_bytes().to_vec());

    let subquery_key: Vec<u8> = b"\0".to_vec();

    query.set_subquery_key(subquery_key);

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 11);

    let first_value = 1985_u32.to_be_bytes().to_vec();
    assert_eq!(elements[0], first_value);

    let last_value = 1995_u32.to_be_bytes().to_vec();
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_after_query_with_non_unique_subquery() {
    let db = make_grovedb();
    populate_tree_for_non_unique_range_subquery(&db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new();
    query.insert_range_after(1995_u32.to_be_bytes().to_vec()..);

    let subquery_key: Vec<u8> = b"\0".to_vec();
    let mut subquery = Query::new();
    subquery.insert_all();

    query.set_subquery_key(subquery_key);
    query.set_subquery(subquery);

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 200);

    let mut first_value = 1996_u32.to_be_bytes().to_vec();
    first_value.append(&mut 100_u32.to_be_bytes().to_vec());
    assert_eq!(elements[0], first_value);

    let mut last_value = 1999_u32.to_be_bytes().to_vec();
    last_value.append(&mut 149_u32.to_be_bytes().to_vec());
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_after_to_query_with_non_unique_subquery() {
    let db = make_grovedb();
    populate_tree_for_non_unique_range_subquery(&db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new();
    query.insert_range_after_to(1995_u32.to_be_bytes().to_vec()..1997_u32.to_be_bytes().to_vec());

    let subquery_key: Vec<u8> = b"\0".to_vec();
    let mut subquery = Query::new();
    subquery.insert_all();

    query.set_subquery_key(subquery_key);
    query.set_subquery(subquery);

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 50);

    let mut first_value = 1996_u32.to_be_bytes().to_vec();
    first_value.append(&mut 100_u32.to_be_bytes().to_vec());
    assert_eq!(elements[0], first_value);

    let mut last_value = 1996_u32.to_be_bytes().to_vec();
    last_value.append(&mut 149_u32.to_be_bytes().to_vec());
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_after_to_inclusive_query_with_non_unique_subquery() {
    let db = make_grovedb();
    populate_tree_for_non_unique_range_subquery(&db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new();
    query.insert_range_after_to_inclusive(
        1995_u32.to_be_bytes().to_vec()..=1997_u32.to_be_bytes().to_vec(),
    );

    let subquery_key: Vec<u8> = b"\0".to_vec();
    let mut subquery = Query::new();
    subquery.insert_all();

    query.set_subquery_key(subquery_key);
    query.set_subquery(subquery);

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 100);

    let mut first_value = 1996_u32.to_be_bytes().to_vec();
    first_value.append(&mut 100_u32.to_be_bytes().to_vec());
    assert_eq!(elements[0], first_value);

    let mut last_value = 1997_u32.to_be_bytes().to_vec();
    last_value.append(&mut 149_u32.to_be_bytes().to_vec());
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_after_to_inclusive_query_with_non_unique_subquery_and_key_out_of_bounds() {
    let db = make_grovedb();
    populate_tree_for_non_unique_range_subquery(&db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new_with_direction(false);
    query.insert_range_after_to_inclusive(
        1995_u32.to_be_bytes().to_vec()..=5000_u32.to_be_bytes().to_vec(),
    );

    let subquery_key: Vec<u8> = b"\0".to_vec();
    let mut subquery = Query::new_with_direction(false);
    subquery.insert_all();

    query.set_subquery_key(subquery_key);
    query.set_subquery(subquery);

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 200);

    let mut first_value = 1999_u32.to_be_bytes().to_vec();
    first_value.append(&mut 149_u32.to_be_bytes().to_vec());
    assert_eq!(elements[0], first_value);

    let mut last_value = 1996_u32.to_be_bytes().to_vec();
    last_value.append(&mut 100_u32.to_be_bytes().to_vec());
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_inclusive_query_with_double_non_unique_subquery() {
    let db = make_grovedb();
    populate_tree_for_non_unique_double_range_subquery(&db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new();
    query.insert_range_inclusive((3u32).to_be_bytes().to_vec()..=(4u32).to_be_bytes().to_vec());

    query.set_subquery_key(b"a".to_vec());

    let mut subquery = Query::new();
    subquery
        .insert_range_inclusive((29u32).to_be_bytes().to_vec()..=(31u32).to_be_bytes().to_vec());

    subquery.set_subquery_key(b"\0".to_vec());

    let mut subsubquery = Query::new();
    subsubquery.insert_all();

    subquery.set_subquery(subsubquery);

    query.set_subquery(subquery);

    let path_query = PathQuery::new_unsized(path, query.clone());

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 60);

    let first_value = 100_u32.to_be_bytes().to_vec();
    assert_eq!(elements[0], first_value);

    let last_value = 109_u32.to_be_bytes().to_vec();
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_get_range_query_with_limit_and_offset() {
    let db = make_grovedb();
    populate_tree_for_non_unique_range_subquery(&db);

    let path = vec![TEST_LEAF.to_vec()];
    let mut query = Query::new_with_direction(true);
    query.insert_range(1990_u32.to_be_bytes().to_vec()..1995_u32.to_be_bytes().to_vec());

    let subquery_key: Vec<u8> = b"\0".to_vec();
    let mut subquery = Query::new();
    subquery.insert_all();

    query.set_subquery_key(subquery_key.clone());
    query.set_subquery(subquery.clone());

    // Baseline query: no offset or limit + left to right
    let path_query = PathQuery::new(path.clone(), SizedQuery::new(query.clone(), None, None));

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 250);

    let mut first_value = 1990_u32.to_be_bytes().to_vec();
    first_value.append(&mut 100_u32.to_be_bytes().to_vec());
    assert_eq!(elements[0], first_value);

    let mut last_value = 1994_u32.to_be_bytes().to_vec();
    last_value.append(&mut 149_u32.to_be_bytes().to_vec());
    assert_eq!(elements[elements.len() - 1], last_value);

    subquery.left_to_right = false;

    query.set_subquery_key(subquery_key.clone());
    query.set_subquery(subquery.clone());

    query.left_to_right = false;

    // Baseline query: no offset or limit + right to left
    let path_query = PathQuery::new(path.clone(), SizedQuery::new(query.clone(), None, None));

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 250);

    let mut first_value = 1994_u32.to_be_bytes().to_vec();
    first_value.append(&mut 149_u32.to_be_bytes().to_vec());
    assert_eq!(elements[0], first_value);

    let mut last_value = 1990_u32.to_be_bytes().to_vec();
    last_value.append(&mut 100_u32.to_be_bytes().to_vec());
    assert_eq!(elements[elements.len() - 1], last_value);

    subquery.left_to_right = true;

    query.set_subquery_key(subquery_key.clone());
    query.set_subquery(subquery.clone());

    query.left_to_right = true;

    // Limit the result to just 55 elements
    let path_query = PathQuery::new(path.clone(), SizedQuery::new(query.clone(), Some(55), None));

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 55);

    let mut first_value = 1990_u32.to_be_bytes().to_vec();
    first_value.append(&mut 100_u32.to_be_bytes().to_vec());
    assert_eq!(elements[0], first_value);

    // Second tree 5 element [100, 101, 102, 103, 104]
    let mut last_value = 1991_u32.to_be_bytes().to_vec();
    last_value.append(&mut 104_u32.to_be_bytes().to_vec());
    assert_eq!(elements[elements.len() - 1], last_value);

    query.set_subquery_key(subquery_key.clone());
    query.set_subquery(subquery.clone());

    // Limit the result set to 60 elements but skip the first 14 elements
    let path_query = PathQuery::new(
        path.clone(),
        SizedQuery::new(query.clone(), Some(60), Some(14)),
    );

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 60);

    // Skips the first 14 elements, starts from the 15th
    // i.e skips [100 - 113] starts from 114
    let mut first_value = 1990_u32.to_be_bytes().to_vec();
    first_value.append(&mut 114_u32.to_be_bytes().to_vec());
    assert_eq!(elements[0], first_value);

    // Continues for 60 iterations
    // Takes 36 elements from the first tree (50 - 14)
    // takes the remaining 24 from the second three (60 - 36)
    let mut last_value = 1991_u32.to_be_bytes().to_vec();
    last_value.append(&mut 123_u32.to_be_bytes().to_vec());
    assert_eq!(elements[elements.len() - 1], last_value);

    query.set_subquery_key(subquery_key.clone());
    query.set_subquery(subquery.clone());

    query.left_to_right = false;

    // Limit the result set to 60 element but skip first 10 elements (this time
    // right to left)
    let path_query = PathQuery::new(
        path.clone(),
        SizedQuery::new(query.clone(), Some(60), Some(10)),
    );

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 60);

    // Skips the first 10 elements from the back
    // last tree and starts from the 11th before the end
    // Underlying subquery is ascending
    let mut first_value = 1994_u32.to_be_bytes().to_vec();
    first_value.append(&mut 110_u32.to_be_bytes().to_vec());
    assert_eq!(elements[0], first_value);

    let mut last_value = 1993_u32.to_be_bytes().to_vec();
    last_value.append(&mut 119_u32.to_be_bytes().to_vec());
    assert_eq!(elements[elements.len() - 1], last_value);

    query.set_subquery_key(subquery_key.clone());
    query.set_subquery(subquery.clone());

    query.left_to_right = true;

    // Offset bigger than elements in range
    let path_query = PathQuery::new(
        path.clone(),
        SizedQuery::new(query.clone(), None, Some(5000)),
    );

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 0);

    query.set_subquery_key(subquery_key.clone());
    query.set_subquery(subquery);

    // Limit bigger than elements in range
    let path_query = PathQuery::new(
        path.clone(),
        SizedQuery::new(query.clone(), Some(5000), None),
    );

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 250);

    // Test on unique subtree build
    let db = make_grovedb();
    populate_tree_for_unique_range_subquery(&db);

    let mut query = Query::new_with_direction(true);
    query.insert_range(1990_u32.to_be_bytes().to_vec()..2000_u32.to_be_bytes().to_vec());

    query.set_subquery_key(subquery_key);

    let path_query = PathQuery::new(path, SizedQuery::new(query.clone(), Some(5), Some(2)));

    let (elements, _) = db
        .get_path_query(&path_query, None)
        .expect("expected successful get_path_query");

    assert_eq!(elements.len(), 5);

    let first_value = 1992_u32.to_be_bytes().to_vec();
    assert_eq!(elements[0], first_value);

    let last_value = 1996_u32.to_be_bytes().to_vec();
    assert_eq!(elements[elements.len() - 1], last_value);
}

#[test]
fn test_root_hash() {
    let db = make_grovedb();
    // Check hashes are different if tree is edited
    let old_root_hash = db.root_hash(None);
    db.insert([TEST_LEAF], b"key1", Element::Item(b"ayy".to_vec()), None)
        .expect("unable to insert an item");
    assert_ne!(old_root_hash.unwrap(), db.root_hash(None).unwrap());

    // Check isolation
    let transaction = db.start_transaction();

    db.insert(
        [TEST_LEAF],
        b"key2",
        Element::Item(b"ayy".to_vec()),
        Some(&transaction),
    )
    .expect("unable to insert an item");
    let root_hash_outside = db.root_hash(None).unwrap();
    assert_ne!(db.root_hash(Some(&transaction)).unwrap(), root_hash_outside);

    assert_eq!(db.root_hash(None).unwrap(), root_hash_outside);
    db.commit_transaction(transaction).unwrap();
    assert_ne!(db.root_hash(None).unwrap(), root_hash_outside);
}

#[test]
fn test_subtree_deletion_with_transaction() {
    let element = Element::Item(b"ayy".to_vec());

    let db = make_grovedb();
    let transaction = db.start_transaction();

    // Insert some nested subtrees
    db.insert(
        [TEST_LEAF],
        b"key1",
        Element::empty_tree(),
        Some(&transaction),
    )
    .expect("successful subtree 1 insert");
    db.insert(
        [TEST_LEAF, b"key1"],
        b"key2",
        Element::empty_tree(),
        Some(&transaction),
    )
    .expect("successful subtree 2 insert");

    // Insert an element into subtree
    db.insert(
        [TEST_LEAF, b"key1", b"key2"],
        b"key3",
        element,
        Some(&transaction),
    )
    .expect("successful value insert");
    db.insert(
        [TEST_LEAF],
        b"key4",
        Element::empty_tree(),
        Some(&transaction),
    )
    .expect("successful subtree 3 insert");

    db.delete([TEST_LEAF], b"key1", Some(&transaction))
        .expect("unable to delete subtree");
    assert!(matches!(
        db.get([TEST_LEAF, b"key1", b"key2"], b"key3", Some(&transaction)),
        Err(Error::PathNotFound(_))
    ));
    transaction.commit().expect("cannot commit transaction");
    assert!(matches!(
        db.get([TEST_LEAF], b"key1", None),
        Err(Error::PathKeyNotFound(_))
    ));
    assert!(matches!(db.get([TEST_LEAF], b"key4", None), Ok(_)));
}

#[test]
fn test_get_non_existing_root_leaf() {
    let db = make_grovedb();
    assert!(matches!(db.get([], b"ayy", None), Err(_)));
}
