use std::{
    ops::{Deref, DerefMut},
    option::Option::None,
};

use ::visualize::{Drawer, Visualize};
use rand::Rng;
use tempfile::TempDir;

use super::*;

pub const TEST_LEAF: &[u8] = b"test_leaf";
pub const ANOTHER_TEST_LEAF: &[u8] = b"test_leaf2";
const DEEP_LEAF: &[u8] = b"deep_leaf";

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
    fn visualize<W: std::io::Write>(&self, drawer: Drawer<W>) -> std::io::Result<Drawer<W>> {
        self.db.visualize(drawer)
    }
}

/// A helper method to create GroveDB with one leaf for a root tree
pub fn make_grovedb() -> TempGroveDb {
    let tmp_dir = TempDir::new().unwrap();
    let mut db = GroveDb::open(tmp_dir.path()).unwrap();
    add_test_leaves(&mut db);
    TempGroveDb {
        _tmp_dir: tmp_dir,
        db,
    }
}

fn add_test_leaves(db: &mut GroveDb) {
    db.insert([], TEST_LEAF, Element::empty_tree(), None)
        .expect("successful root tree leaf insert");
    db.insert([], ANOTHER_TEST_LEAF, Element::empty_tree(), None)
        .expect("successful root tree leaf 2 insert");
}

fn make_deep_tree() -> TempGroveDb {
    // Tree Structure
    // root
    //     test_leaf
    //         innertree
    //             k1,v1
    //             k2,v2
    //             k3,v3
    //         innertree4
    //             k4,v4
    //             k5,v5
    //     another_test_leaf
    //         innertree2
    //             k3,v3
    //         innertree3
    //             k4,v4
    //     deep_leaf
    //          deep_node_1
    //              deeper_node_1
    //                  k1,v1
    //                  k2,v2
    //                  k3,v3
    //              deeper_node_2
    //                  k4,v4
    //                  k5,v5
    //                  k6,v6
    //          deep_node_2
    //              deeper_node_3
    //                  k7,v7
    //                  k8,v8
    //                  k9,v9
    //              deeper_node_4
    //                  k10,v10
    //                  k11,v11

    // Insert elements into grovedb instance
    let temp_db = make_grovedb();

    // add an extra root leaf
    temp_db
        .insert([], DEEP_LEAF, Element::empty_tree(), None)
        .expect("successful root tree leaf insert");

    // Insert level 1 nodes
    temp_db
        .insert([TEST_LEAF], b"innertree", Element::empty_tree(), None)
        .expect("successful subtree insert");
    temp_db
        .insert([TEST_LEAF], b"innertree4", Element::empty_tree(), None)
        .expect("successful subtree insert");
    temp_db
        .insert(
            [ANOTHER_TEST_LEAF],
            b"innertree2",
            Element::empty_tree(),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [ANOTHER_TEST_LEAF],
            b"innertree3",
            Element::empty_tree(),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert([DEEP_LEAF], b"deep_node_1", Element::empty_tree(), None)
        .expect("successful subtree insert");
    temp_db
        .insert([DEEP_LEAF], b"deep_node_2", Element::empty_tree(), None)
        .expect("successful subtree insert");
    // Insert level 2 nodes
    temp_db
        .insert(
            [TEST_LEAF, b"innertree"],
            b"key1",
            Element::new_item(b"value1".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [TEST_LEAF, b"innertree"],
            b"key2",
            Element::new_item(b"value2".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [TEST_LEAF, b"innertree"],
            b"key3",
            Element::new_item(b"value3".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [TEST_LEAF, b"innertree4"],
            b"key4",
            Element::new_item(b"value4".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [TEST_LEAF, b"innertree4"],
            b"key5",
            Element::new_item(b"value5".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [ANOTHER_TEST_LEAF, b"innertree2"],
            b"key3",
            Element::new_item(b"value3".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [ANOTHER_TEST_LEAF, b"innertree3"],
            b"key4",
            Element::new_item(b"value4".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [DEEP_LEAF, b"deep_node_1"],
            b"deeper_node_1",
            Element::empty_tree(),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [DEEP_LEAF, b"deep_node_1"],
            b"deeper_node_2",
            Element::empty_tree(),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [DEEP_LEAF, b"deep_node_2"],
            b"deeper_node_3",
            Element::empty_tree(),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [DEEP_LEAF, b"deep_node_2"],
            b"deeper_node_4",
            Element::empty_tree(),
            None,
        )
        .expect("successful subtree insert");
    // Insert level 3 nodes
    temp_db
        .insert(
            [DEEP_LEAF, b"deep_node_1", b"deeper_node_1"],
            b"key1",
            Element::new_item(b"value1".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [DEEP_LEAF, b"deep_node_1", b"deeper_node_1"],
            b"key2",
            Element::new_item(b"value2".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [DEEP_LEAF, b"deep_node_1", b"deeper_node_1"],
            b"key3",
            Element::new_item(b"value3".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [DEEP_LEAF, b"deep_node_1", b"deeper_node_2"],
            b"key4",
            Element::new_item(b"value4".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [DEEP_LEAF, b"deep_node_1", b"deeper_node_2"],
            b"key5",
            Element::new_item(b"value5".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [DEEP_LEAF, b"deep_node_1", b"deeper_node_2"],
            b"key6",
            Element::new_item(b"value6".to_vec()),
            None,
        )
        .expect("successful subtree insert");

    temp_db
        .insert(
            [DEEP_LEAF, b"deep_node_2", b"deeper_node_3"],
            b"key7",
            Element::new_item(b"value7".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [DEEP_LEAF, b"deep_node_2", b"deeper_node_3"],
            b"key8",
            Element::new_item(b"value8".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [DEEP_LEAF, b"deep_node_2", b"deeper_node_3"],
            b"key9",
            Element::new_item(b"value9".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [DEEP_LEAF, b"deep_node_2", b"deeper_node_4"],
            b"key10",
            Element::new_item(b"value10".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [DEEP_LEAF, b"deep_node_2", b"deeper_node_4"],
            b"key11",
            Element::new_item(b"value11".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
}

#[test]
fn test_init() {
    let tmp_dir = TempDir::new().unwrap();
    GroveDb::open(tmp_dir).expect("empty tree is ok");
}

#[test]
fn test_insert_value_to_merk() {
    let db = make_grovedb();
    let element = Element::new_item(b"ayy".to_vec());
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
    let element = Element::new_item(b"ayy".to_vec());

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
    let element = Element::new_item(b"ayy".to_vec());

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
fn test_references() {
    let db = make_grovedb();
    db.insert([TEST_LEAF], b"merk_1", Element::empty_tree(), None)
        .expect("successful subtree insert");
    db.insert(
        [TEST_LEAF, b"merk_1"],
        b"key1",
        Element::new_item(b"value1".to_vec()),
        None,
    )
    .expect("successful subtree insert");
    db.insert(
        [TEST_LEAF, b"merk_1"],
        b"key2",
        Element::new_item(b"value2".to_vec()),
        None,
    )
    .expect("successful subtree insert");

    db.insert([TEST_LEAF], b"merk_2", Element::empty_tree(), None)
        .expect("successful subtree insert");
    db.insert(
        [TEST_LEAF, b"merk_2"],
        b"key1",
        Element::new_reference(vec![
            TEST_LEAF.to_vec(),
            b"merk_1".to_vec(),
            b"key1".to_vec(),
        ]),
        None,
    )
    .expect("successful subtree insert");
    db.insert(
        [TEST_LEAF, b"merk_2"],
        b"key2",
        Element::new_reference(vec![
            TEST_LEAF.to_vec(),
            b"merk_1".to_vec(),
            b"key2".to_vec(),
        ]),
        None,
    )
    .expect("successful subtree insert");

    // Verify the base element is updated on reference insertion
    assert_eq!(
        db.get([TEST_LEAF, b"merk_1"], b"key1", None)
            .expect("successful get"),
        Element::Item(
            b"value1".to_vec(),
            vec![vec![
                TEST_LEAF.to_vec(),
                b"merk_2".to_vec(),
                b"key1".to_vec()
            ]]
        )
    );
    assert_eq!(
        db.get([TEST_LEAF, b"merk_1"], b"key2", None)
            .expect("successful get"),
        Element::Item(
            b"value2".to_vec(),
            vec![vec![
                TEST_LEAF.to_vec(),
                b"merk_2".to_vec(),
                b"key2".to_vec()
            ]]
        )
    );

    let subtree_storage = db.db.db.get_storage_context([TEST_LEAF, b"merk_1"]);
    let value_subtree = Merk::open(subtree_storage).expect("cannot open merk");

    let subtree_storage = db.db.db.get_storage_context([TEST_LEAF, b"merk_2"]);
    let reference_subtree = Merk::open(subtree_storage).expect("cannot open merk");

    // Verify that they have the same root hash
    assert_eq!(value_subtree.root_hash(), reference_subtree.root_hash());
}

#[test]
fn test_follow_references() {
    let db = make_grovedb();
    let element = Element::new_item(b"ayy".to_vec());

    // Insert an item to refer to
    db.insert([TEST_LEAF], b"key2", Element::empty_tree(), None)
        .expect("successful subtree 1 insert");
    db.insert([TEST_LEAF, b"key2"], b"key3", element.clone(), None)
        .expect("successful value insert");

    // Insert an item to refer to
    db.insert([TEST_LEAF], b"key2", Element::empty_tree(), None)
        .expect("successful subtree 1 insert");
    db.insert([TEST_LEAF, b"key2"], b"key3", element.clone(), None)
        .expect("successful value insert");

    // Insert a reference
    db.insert(
        [TEST_LEAF],
        b"reference_key",
        Element::new_reference(vec![TEST_LEAF.to_vec(), b"key2".to_vec(), b"key3".to_vec()]),
        None,
    )
    .expect("successful reference insert");

    assert_eq!(
        db.get([TEST_LEAF], b"reference_key", None)
            .expect("successful get"),
        Element::Item(
            b"ayy".to_vec(),
            vec![vec![TEST_LEAF.to_vec(), b"reference_key".to_vec()]]
        )
    );
}

#[test]
fn test_reference_must_point_to_item() {
    let db = make_grovedb();

    let result = db.insert(
        [TEST_LEAF],
        b"reference_key_1",
        Element::new_reference(vec![TEST_LEAF.to_vec(), b"reference_key_2".to_vec()]),
        None,
    );

    assert!(matches!(result, Err(Error::PathKeyNotFound(_))));
}

fn test_cyclic_references() {
    // impossible to have cyclic references
    // see: test_reference_must_point_to_item
}

#[test]
fn test_too_many_indirections() {
    use crate::operations::get::MAX_REFERENCE_HOPS;
    let db = make_grovedb();

    let keygen = |idx| format!("key{}", idx).bytes().collect::<Vec<u8>>();

    db.insert(
        [TEST_LEAF],
        b"key0",
        Element::new_item(b"oops".to_vec()),
        None,
    )
    .expect("successful item insert");

    for i in 1..=(MAX_REFERENCE_HOPS) {
        db.insert(
            [TEST_LEAF],
            &keygen(i),
            Element::new_reference(vec![TEST_LEAF.to_vec(), keygen(i - 1)]),
            None,
        )
        .expect("successful reference insert");
    }

    assert!(matches!(
        db.insert(
            [TEST_LEAF],
            &keygen(MAX_REFERENCE_HOPS + 1),
            Element::new_reference(vec![TEST_LEAF.to_vec(), keygen(MAX_REFERENCE_HOPS)]),
            None,
        ),
        Err(Error::ReferenceLimit)
    ))
}

#[test]
fn test_tree_structure_is_persistent() {
    let tmp_dir = TempDir::new().unwrap();
    let element = Element::new_item(b"ayy".to_vec());
    // Create a scoped GroveDB
    let prev_root_hash = {
        let mut db = GroveDb::open(tmp_dir.path()).unwrap();
        add_test_leaves(&mut db);

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
fn test_root_tree_leaves_are_noted() {
    let db = make_grovedb();
    let mut hm = BTreeMap::new();
    hm.insert(TEST_LEAF.to_vec(), 0);
    hm.insert(ANOTHER_TEST_LEAF.to_vec(), 1);
    assert_eq!(db.get_root_leaf_keys(None).unwrap(), hm);
    assert_eq!(db.get_root_tree(None).unwrap().leaves_len(), 2);
}

#[test]
fn test_path_query_proofs_without_subquery_with_reference() {
    // Tree Structure
    // root
    //     test_leaf
    //         innertree
    //             k1,v1
    //             k2,v2
    //             k3,v3
    //     another_test_leaf
    //         innertree2
    //             k3,v3
    //             k4, reference to k1 in innertree
    //             k5, reference to k4 in innertree3
    //         innertree3
    //             k4,v4

    // Insert elements into grovedb instance
    let temp_db = make_grovedb();
    // Insert level 1 nodes
    temp_db
        .insert([TEST_LEAF], b"innertree", Element::empty_tree(), None)
        .expect("successful subtree insert");
    temp_db
        .insert(
            [ANOTHER_TEST_LEAF],
            b"innertree2",
            Element::empty_tree(),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [ANOTHER_TEST_LEAF],
            b"innertree3",
            Element::empty_tree(),
            None,
        )
        .expect("successful subtree insert");
    // Insert level 2 nodes
    temp_db
        .insert(
            [TEST_LEAF, b"innertree"],
            b"key1",
            Element::new_item(b"value1".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [TEST_LEAF, b"innertree"],
            b"key2",
            Element::new_item(b"value2".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [TEST_LEAF, b"innertree"],
            b"key3",
            Element::new_item(b"value3".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [ANOTHER_TEST_LEAF, b"innertree2"],
            b"key3",
            Element::new_item(b"value3".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [ANOTHER_TEST_LEAF, b"innertree2"],
            b"key4",
            Element::new_reference(vec![
                TEST_LEAF.to_vec(),
                b"innertree".to_vec(),
                b"key1".to_vec(),
            ]),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [ANOTHER_TEST_LEAF, b"innertree3"],
            b"key4",
            Element::new_item(b"value4".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [ANOTHER_TEST_LEAF, b"innertree2"],
            b"key5",
            Element::new_reference(vec![
                ANOTHER_TEST_LEAF.to_vec(),
                b"innertree3".to_vec(),
                b"key4".to_vec(),
            ]),
            None,
        )
        .expect("successful subtree insert");

    // Single key query
    let mut query = Query::new();
    query.insert_range_from(b"key4".to_vec()..);

    let path_query = PathQuery::new_unsized(
        vec![ANOTHER_TEST_LEAF.to_vec(), b"innertree2".to_vec()],
        query,
    );

    let proof = temp_db.prove(path_query.clone()).unwrap();
    let (hash, result_set) =
        GroveDb::execute_proof(proof.as_slice(), path_query).expect("should execute proof");

    assert_eq!(hash, temp_db.root_hash(None).unwrap().unwrap());

    let r1 = Element::Item(
        b"value1".to_vec(),
        vec![vec![
            ANOTHER_TEST_LEAF.to_vec(),
            b"innertree2".to_vec(),
            b"key4".to_vec(),
        ]],
    )
    .serialize()
    .unwrap();
    let r2 = Element::Item(
        b"value4".to_vec(),
        vec![vec![
            ANOTHER_TEST_LEAF.to_vec(),
            b"innertree2".to_vec(),
            b"key5".to_vec(),
        ]],
    )
    .serialize()
    .unwrap();

    assert_eq!(
        result_set,
        vec![(b"key4".to_vec(), r1), (b"key5".to_vec(), r2),]
    );
}

#[test]
fn test_path_query_proofs_without_subquery() {
    // Tree Structure
    // root
    //     test_leaf
    //         innertree
    //             k1,v1
    //             k2,v2
    //             k3,v3
    //     another_test_leaf
    //         innertree2
    //             k3,v3
    //         innertree3
    //             k4,v4

    // Insert elements into grovedb instance
    let temp_db = make_grovedb();
    // Insert level 1 nodes
    temp_db
        .insert([TEST_LEAF], b"innertree", Element::empty_tree(), None)
        .expect("successful subtree insert");
    temp_db
        .insert(
            [ANOTHER_TEST_LEAF],
            b"innertree2",
            Element::empty_tree(),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [ANOTHER_TEST_LEAF],
            b"innertree3",
            Element::empty_tree(),
            None,
        )
        .expect("successful subtree insert");
    // Insert level 2 nodes
    temp_db
        .insert(
            [TEST_LEAF, b"innertree"],
            b"key1",
            Element::new_item(b"value1".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [TEST_LEAF, b"innertree"],
            b"key2",
            Element::new_item(b"value2".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [TEST_LEAF, b"innertree"],
            b"key3",
            Element::new_item(b"value3".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [ANOTHER_TEST_LEAF, b"innertree2"],
            b"key3",
            Element::new_item(b"value3".to_vec()),
            None,
        )
        .expect("successful subtree insert");
    temp_db
        .insert(
            [ANOTHER_TEST_LEAF, b"innertree3"],
            b"key4",
            Element::new_item(b"value4".to_vec()),
            None,
        )
        .expect("successful subtree insert");

    // Single key query
    let mut query = Query::new();
    query.insert_key(b"key1".to_vec());

    let path_query = PathQuery::new_unsized(vec![TEST_LEAF.to_vec(), b"innertree".to_vec()], query);

    let proof = temp_db.prove(path_query.clone()).unwrap();
    let (hash, result_set) =
        GroveDb::execute_proof(proof.as_slice(), path_query).expect("should execute proof");

    assert_eq!(hash, temp_db.root_hash(None).unwrap().unwrap());
    let r1 = Element::new_item(b"value1".to_vec()).serialize().unwrap();
    assert_eq!(result_set, vec![(b"key1".to_vec(), r1)]);

    // Range query + limit
    let mut query = Query::new();
    query.insert_range_after(b"key1".to_vec()..);
    let path_query = PathQuery::new(
        vec![TEST_LEAF.to_vec(), b"innertree".to_vec()],
        SizedQuery::new(query, Some(1), None),
    );

    let proof = temp_db.prove(path_query.clone()).unwrap();
    let (hash, result_set) =
        GroveDb::execute_proof(proof.as_slice(), path_query).expect("should execute proof");

    assert_eq!(hash, temp_db.root_hash(None).unwrap().unwrap());
    let r1 = Element::new_item(b"value2".to_vec()).serialize().unwrap();
    assert_eq!(result_set, vec![(b"key2".to_vec(), r1)]);

    // Range query + offset + limit
    let mut query = Query::new();
    query.insert_range_after(b"key1".to_vec()..);
    let path_query = PathQuery::new(
        vec![TEST_LEAF.to_vec(), b"innertree".to_vec()],
        SizedQuery::new(query, Some(1), Some(1)),
    );

    let proof = temp_db.prove(path_query.clone()).unwrap();
    let (hash, result_set) =
        GroveDb::execute_proof(proof.as_slice(), path_query).expect("should execute proof");

    assert_eq!(hash, temp_db.root_hash(None).unwrap().unwrap());
    let r1 = Element::new_item(b"value3".to_vec()).serialize().unwrap();
    assert_eq!(result_set, vec![(b"key3".to_vec(), r1)]);

    // Range query + direction + limit
    let mut query = Query::new_with_direction(false);
    query.insert_all();
    let path_query = PathQuery::new(
        vec![TEST_LEAF.to_vec(), b"innertree".to_vec()],
        SizedQuery::new(query, Some(2), None),
    );

    let mut proof = temp_db.prove(path_query.clone()).unwrap();
    let (hash, result_set) =
        GroveDb::execute_proof(proof.as_slice(), path_query).expect("should execute proof");

    assert_eq!(hash, temp_db.root_hash(None).unwrap().unwrap());
    let r1 = Element::new_item(b"value3".to_vec()).serialize().unwrap();
    let r2 = Element::new_item(b"value2".to_vec()).serialize().unwrap();
    assert_eq!(
        result_set,
        vec![(b"key3".to_vec(), r1), (b"key2".to_vec(), r2)]
    );
}

#[test]
fn test_path_query_proofs_with_default_subquery() {
    let temp_db = make_deep_tree();

    let mut query = Query::new();
    query.insert_all();

    let mut subq = Query::new();
    subq.insert_all();
    query.set_subquery(subq);

    let path_query = PathQuery::new_unsized(vec![TEST_LEAF.to_vec()], query);

    let proof = temp_db.prove(path_query.clone()).unwrap();
    let (hash, result_set) =
        GroveDb::execute_proof(proof.as_slice(), path_query).expect("should execute proof");

    assert_eq!(hash, temp_db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 5);

    let keys = [
        b"key1".to_vec(),
        b"key2".to_vec(),
        b"key3".to_vec(),
        b"key4".to_vec(),
        b"key5".to_vec(),
    ];
    let values = [
        b"value1".to_vec(),
        b"value2".to_vec(),
        b"value3".to_vec(),
        b"value4".to_vec(),
        b"value5".to_vec(),
    ];
    let elements = values.map(|x| Element::new_item(x).serialize().unwrap());
    let expected_result_set: Vec<(Vec<u8>, Vec<u8>)> = keys.into_iter().zip(elements).collect();
    assert_eq!(result_set, expected_result_set);

    // range subquery
    let mut query = Query::new();
    query.insert_all();

    let mut subq = Query::new();
    subq.insert_range_after_to_inclusive(b"key1".to_vec()..=b"key4".to_vec());
    query.set_subquery(subq);

    let path_query = PathQuery::new_unsized(vec![TEST_LEAF.to_vec()], query);

    let proof = temp_db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(proof.as_slice(), path_query).expect(
        "should
    execute proof",
    );

    assert_eq!(hash, temp_db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 3);

    let keys = [b"key2".to_vec(), b"key3".to_vec(), b"key4".to_vec()];
    let values = [b"value2".to_vec(), b"value3".to_vec(), b"value4".to_vec()];
    let elements = values.map(|x| Element::new_item(x).serialize().unwrap());
    let expected_result_set: Vec<(Vec<u8>, Vec<u8>)> = keys.into_iter().zip(elements).collect();
    assert_eq!(result_set, expected_result_set);

    // deep tree test
    let mut query = Query::new();
    query.insert_all();

    let mut subq = Query::new();
    subq.insert_all();

    let mut sub_subquery = Query::new();
    sub_subquery.insert_all();

    subq.set_subquery(sub_subquery);
    query.set_subquery(subq);

    let path_query = PathQuery::new_unsized(vec![DEEP_LEAF.to_vec()], query);

    let proof = temp_db.prove(path_query.clone()).unwrap();
    let (hash, result_set) =
        GroveDb::execute_proof(proof.as_slice(), path_query).expect("should execute proof");

    assert_eq!(hash, temp_db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 11);

    let keys = [
        b"key1".to_vec(),
        b"key2".to_vec(),
        b"key3".to_vec(),
        b"key4".to_vec(),
        b"key5".to_vec(),
        b"key6".to_vec(),
        b"key7".to_vec(),
        b"key8".to_vec(),
        b"key9".to_vec(),
        b"key10".to_vec(),
        b"key11".to_vec(),
    ];
    let values = [
        b"value1".to_vec(),
        b"value2".to_vec(),
        b"value3".to_vec(),
        b"value4".to_vec(),
        b"value5".to_vec(),
        b"value6".to_vec(),
        b"value7".to_vec(),
        b"value8".to_vec(),
        b"value9".to_vec(),
        b"value10".to_vec(),
        b"value11".to_vec(),
    ];
    let elements = values.map(|x| Element::new_item(x).serialize().unwrap());
    let expected_result_set: Vec<(Vec<u8>, Vec<u8>)> = keys.into_iter().zip(elements).collect();
    assert_eq!(result_set, expected_result_set);
}

#[test]
fn test_path_query_proofs_with_subquery_key() {
    let temp_db = make_deep_tree();

    let mut query = Query::new();
    query.insert_all();

    let mut subq = Query::new();
    subq.insert_all();

    query.set_subquery_key(b"deeper_node_1".to_vec());
    query.set_subquery(subq);

    let path_query = PathQuery::new_unsized(vec![DEEP_LEAF.to_vec()], query);

    let proof = temp_db.prove(path_query.clone()).unwrap();
    let (hash, result_set) =
        GroveDb::execute_proof(proof.as_slice(), path_query).expect("should execute proof");

    assert_eq!(hash, temp_db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 3);

    let keys = [b"key1".to_vec(), b"key2".to_vec(), b"key3".to_vec()];
    let values = [b"value1".to_vec(), b"value2".to_vec(), b"value3".to_vec()];
    let elements = values.map(|x| Element::new_item(x).serialize().unwrap());
    let expected_result_set: Vec<(Vec<u8>, Vec<u8>)> = keys.into_iter().zip(elements).collect();
    assert_eq!(result_set, expected_result_set);
}

#[test]
fn test_path_query_proofs_with_conditional_subquery() {
    let temp_db = make_deep_tree();

    let mut query = Query::new();
    query.insert_all();

    let mut subquery = Query::new();
    subquery.insert_all();

    let mut final_subquery = Query::new();
    final_subquery.insert_all();

    subquery.add_conditional_subquery(
        QueryItem::Key(b"deeper_node_4".to_vec()),
        None,
        Some(final_subquery),
    );

    query.set_subquery(subquery);

    let path_query = PathQuery::new_unsized(vec![DEEP_LEAF.to_vec()], query);
    let proof = temp_db.prove(path_query.clone()).unwrap();
    let (hash, result_set) =
        GroveDb::execute_proof(proof.as_slice(), path_query).expect("should execute proof");

    assert_eq!(hash, temp_db.root_hash(None).unwrap().unwrap());

    let keys = [
        b"deeper_node_1".to_vec(),
        b"deeper_node_2".to_vec(),
        b"key10".to_vec(),
        b"key11".to_vec(),
    ];
    assert_eq!(result_set.len(), keys.len());

    // TODO: Is this defined behaviour
    for (index, key) in keys.iter().enumerate() {
        assert_eq!(&result_set[index].0, key);
    }

    // Default + Conditional subquery
    let mut query = Query::new();
    query.insert_all();

    let mut subquery = Query::new();
    subquery.insert_all();

    let mut final_conditional_subquery = Query::new();
    final_conditional_subquery.insert_all();

    let mut final_default_subquery = Query::new();
    final_default_subquery.insert_range_inclusive(b"key3".to_vec()..=b"key6".to_vec());

    subquery.add_conditional_subquery(
        QueryItem::Key(b"deeper_node_4".to_vec()),
        None,
        Some(final_conditional_subquery),
    );
    subquery.set_subquery(final_default_subquery);

    query.set_subquery(subquery);

    let path_query = PathQuery::new_unsized(vec![DEEP_LEAF.to_vec()], query);
    let proof = temp_db.prove(path_query.clone()).unwrap();
    let (hash, result_set) =
        GroveDb::execute_proof(proof.as_slice(), path_query).expect("should execute proof");

    assert_eq!(hash, temp_db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 6);

    let keys = [
        b"key3".to_vec(),
        b"key4".to_vec(),
        b"key5".to_vec(),
        b"key6".to_vec(),
        b"key10".to_vec(),
        b"key11".to_vec(),
    ];
    let values = [
        b"value3".to_vec(),
        b"value4".to_vec(),
        b"value5".to_vec(),
        b"value6".to_vec(),
        b"value10".to_vec(),
        b"value11".to_vec(),
    ];
    let elements = values.map(|x| Element::new_item(x).serialize().unwrap());
    let expected_result_set: Vec<(Vec<u8>, Vec<u8>)> = keys.into_iter().zip(elements).collect();
    assert_eq!(result_set, expected_result_set);
}

#[test]
fn test_path_query_proofs_with_sized_query() {
    let temp_db = make_deep_tree();

    let mut query = Query::new();
    query.insert_all();

    let mut subquery = Query::new();
    subquery.insert_all();

    let mut final_conditional_subquery = Query::new();
    final_conditional_subquery.insert_all();

    let mut final_default_subquery = Query::new();
    final_default_subquery.insert_range_inclusive(b"key3".to_vec()..=b"key6".to_vec());

    subquery.add_conditional_subquery(
        QueryItem::Key(b"deeper_node_4".to_vec()),
        None,
        Some(final_conditional_subquery),
    );
    subquery.set_subquery(final_default_subquery);
    // subquery.set_subquery_key(b"key3".to_vec());

    query.set_subquery(subquery);

    let path_query = PathQuery::new(
        vec![DEEP_LEAF.to_vec()],
        SizedQuery::new(query, Some(3), Some(1)),
    );
    let proof = temp_db.prove(path_query.clone()).unwrap();
    let (hash, result_set) =
        GroveDb::execute_proof(proof.as_slice(), path_query).expect("should execute proof");

    assert_eq!(hash, temp_db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 3);

    let keys = [b"key4".to_vec(), b"key5".to_vec(), b"key6".to_vec()];
    let values = [b"value4".to_vec(), b"value5".to_vec(), b"value6".to_vec()];
    let elements = values.map(|x| Element::new_item(x).serialize().unwrap());
    let expected_result_set: Vec<(Vec<u8>, Vec<u8>)> = keys.into_iter().zip(elements).collect();
    assert_eq!(result_set, expected_result_set);
}

#[test]
fn test_path_query_proofs_with_direction() {
    let temp_db = make_deep_tree();

    let mut query = Query::new_with_direction(false);
    query.insert_all();

    let mut subquery = Query::new_with_direction(false);
    subquery.insert_all();

    let mut final_conditional_subquery = Query::new_with_direction(false);
    final_conditional_subquery.insert_all();

    let mut final_default_subquery = Query::new_with_direction(false);
    final_default_subquery.insert_range_inclusive(b"key3".to_vec()..=b"key6".to_vec());

    subquery.add_conditional_subquery(
        QueryItem::Key(b"deeper_node_4".to_vec()),
        None,
        Some(final_conditional_subquery),
    );
    subquery.set_subquery(final_default_subquery);

    query.set_subquery(subquery);

    let path_query = PathQuery::new(
        vec![DEEP_LEAF.to_vec()],
        SizedQuery::new(query, Some(3), Some(1)),
    );
    let proof = temp_db.prove(path_query.clone()).unwrap();
    let (hash, result_set) =
        GroveDb::execute_proof(proof.as_slice(), path_query).expect("should execute proof");

    assert_eq!(hash, temp_db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 3);

    let keys = [b"key10".to_vec(), b"key6".to_vec(), b"key5".to_vec()];
    let values = [b"value10".to_vec(), b"value6".to_vec(), b"value5".to_vec()];
    let elements = values.map(|x| Element::new_item(x).serialize().unwrap());
    let expected_result_set: Vec<(Vec<u8>, Vec<u8>)> = keys.into_iter().zip(elements).collect();
    assert_eq!(result_set, expected_result_set);

    // combined directions
    let mut query = Query::new();
    query.insert_all();

    let mut subq = Query::new_with_direction(false);
    subq.insert_all();

    let mut sub_subquery = Query::new();
    sub_subquery.insert_all();

    subq.set_subquery(sub_subquery);
    query.set_subquery(subq);

    let path_query = PathQuery::new_unsized(vec![DEEP_LEAF.to_vec()], query);

    let proof = temp_db.prove(path_query.clone()).unwrap();
    let (hash, result_set) =
        GroveDb::execute_proof(proof.as_slice(), path_query).expect("should execute proof");

    assert_eq!(hash, temp_db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 11);

    let keys = [
        b"key4".to_vec(),
        b"key5".to_vec(),
        b"key6".to_vec(),
        b"key1".to_vec(),
        b"key2".to_vec(),
        b"key3".to_vec(),
        b"key10".to_vec(),
        b"key11".to_vec(),
        b"key7".to_vec(),
        b"key8".to_vec(),
        b"key9".to_vec(),
    ];
    let values = [
        b"value4".to_vec(),
        b"value5".to_vec(),
        b"value6".to_vec(),
        b"value1".to_vec(),
        b"value2".to_vec(),
        b"value3".to_vec(),
        b"value10".to_vec(),
        b"value11".to_vec(),
        b"value7".to_vec(),
        b"value8".to_vec(),
        b"value9".to_vec(),
    ];
    let elements = values.map(|x| Element::new_item(x).serialize().unwrap());
    let expected_result_set: Vec<(Vec<u8>, Vec<u8>)> = keys.into_iter().zip(elements).collect();
    assert_eq!(result_set, expected_result_set);
}

// #[test]
// fn test_checkpoint() {
//     let mut db = make_grovedb();
//     let element1 = Element::new_item(b"ayy".to_vec());
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
//     let element2 = Element::new_item(b"ayy2".to_vec());
//     let element3 = Element::new_item(b"ayy3".to_vec());
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
        Element::new_item(b"hello".to_vec()),
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

    let element1 = Element::new_item(b"ayy".to_vec());

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
    assert_eq!(result_with_transaction, Element::new_item(b"ayy".to_vec()));

    // Test that commit works
    // transaction.commit();
    db.commit_transaction(transaction).unwrap();

    // Check that the change was committed
    let result = db
        .get([TEST_LEAF], item_key, None)
        .expect("Expected transaction to work");
    assert_eq!(result, Element::new_item(b"ayy".to_vec()));
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

    let element1 = Element::new_item(b"ayy".to_vec());

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
    let element = Element::new_item(b"ayy".to_vec());

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
    let element = Element::new_item(b"ayy".to_vec());
    let element2 = Element::new_item(b"lmao".to_vec());

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
    let element = Element::new_item(b"ayy".to_vec());
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
fn test_referenced_element_deletion() {
    dbg!("hello");
    let db = make_grovedb();
    let element = Element::new_item(b"ayy".to_vec());
    db.insert([TEST_LEAF], b"key", element, None)
        .expect("successful insert");
    let other_elem = Element::new_item(b"wow".to_vec());
    db.insert([TEST_LEAF], b"key2", other_elem, None)
        .expect("successful insert");
    let reference = Element::new_reference(vec![TEST_LEAF.to_vec(), b"key".to_vec()]);
    db.insert([ANOTHER_TEST_LEAF], b"reference_key", reference, None)
        .expect("successful insert");
    let reference2 = Element::new_reference(vec![TEST_LEAF.to_vec(), b"key".to_vec()]);
    db.insert([ANOTHER_TEST_LEAF], b"reference_key_2", reference2, None)
        .expect("successful insert");
    let reference3 = Element::new_reference(vec![
        ANOTHER_TEST_LEAF.to_vec(),
        b"reference_key_2".to_vec(),
    ]);
    db.insert([TEST_LEAF], b"reference_of_reference", reference3, None)
        .expect("successful insert");

    let root_hash = db.root_hash(None).unwrap();
    assert!(db.delete([TEST_LEAF], b"key", None).is_ok());
    assert!(matches!(
        db.get([TEST_LEAF], b"key", None),
        Err(Error::PathKeyNotFound(_))
    ));
    assert!(matches!(
        db.get([ANOTHER_TEST_LEAF], b"reference_key", None),
        Err(Error::PathKeyNotFound(_))
    ));
    assert!(matches!(
        db.get([ANOTHER_TEST_LEAF], b"reference_key_2", None),
        Err(Error::PathKeyNotFound(_))
    ));
    assert!(matches!(
        db.get([TEST_LEAF], b"reference_of_reference", None),
        Err(Error::PathKeyNotFound(_))
    ));
    assert!(matches!(db.get([TEST_LEAF], b"key2", None), Ok(_)));
    assert_ne!(root_hash, db.root_hash(None).unwrap());
}

#[test]
fn test_reference_deletion() {
    let db = make_grovedb();
    let element = Element::new_item(b"ayy".to_vec());
    db.insert([TEST_LEAF], b"key", element, None)
        .expect("successful insert");
    let reference = Element::new_reference(vec![TEST_LEAF.to_vec(), b"key".to_vec()]);
    db.insert([ANOTHER_TEST_LEAF], b"reference_key", reference, None)
        .expect("successful insert");
    let reference2 = Element::new_reference(vec![TEST_LEAF.to_vec(), b"key".to_vec()]);
    db.insert([ANOTHER_TEST_LEAF], b"reference_key_2", reference2, None)
        .expect("successful insert");

    let element = db
        .get([TEST_LEAF], b"key", None)
        .expect("should get element");
    if let Element::Item(_, references) = element {
        assert_eq!(references.len(), 2);
    } else {
        panic!("should be able to fetch item");
    }

    // delete one reference
    assert!(db
        .delete([ANOTHER_TEST_LEAF], b"reference_key", None)
        .is_ok());

    // element should not point to reference anymore
    let element = db
        .get([TEST_LEAF], b"key", None)
        .expect("should get element");
    if let Element::Item(_, references) = element {
        assert_eq!(references.len(), 1);
    } else {
        panic!("should be able to fetch item");
    }

    // delete other reference
    assert!(db
        .delete([ANOTHER_TEST_LEAF], b"reference_key_2", None)
        .is_ok());

    // element should not point to reference anymore
    let element = db
        .get([TEST_LEAF], b"key", None)
        .expect("should get element");
    if let Element::Item(_, references) = element {
        assert_eq!(references.len(), 0);
    } else {
        panic!("should be able to fetch item");
    }
}

#[test]
fn test_find_subtrees() {
    let element = Element::new_item(b"ayy".to_vec());
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
    let element = Element::new_item(b"ayy".to_vec());

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
        assert_eq!(result_element, Element::new_item(b"ayy".to_vec()));
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
    assert_eq!(result_element, Element::new_item(b"ayy".to_vec()));

    // Should be able to retrieve instances created before transaction
    let subtree_storage = db.db.db.get_storage_context([TEST_LEAF, b"key1", b"key2"]);
    let subtree = Merk::open(subtree_storage).expect("cannot open merk");
    let result_element = Element::get(&subtree, b"key3").unwrap();
    assert_eq!(result_element, Element::new_item(b"ayy".to_vec()));
}

#[test]
fn test_subtree_deletion() {
    let element = Element::new_item(b"ayy".to_vec());
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
    let element = Element::new_item(b"value".to_vec());
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
    let element = Element::new_item(b"value".to_vec());
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
        Element::new_item(b"ayya".to_vec()),
        None,
    )
    .expect("successful value insert");
    db.insert(
        [TEST_LEAF, b"key1"],
        b"key4",
        Element::new_item(b"ayyb".to_vec()),
        None,
    )
    .expect("successful value insert");
    db.insert(
        [TEST_LEAF, b"key1"],
        b"key5",
        Element::new_item(b"ayyc".to_vec()),
        None,
    )
    .expect("successful value insert");
    db.insert(
        [TEST_LEAF, b"key2"],
        b"key6",
        Element::new_item(b"ayyd".to_vec()),
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
            subtree::Element::new_item(b"ayya".to_vec()),
            subtree::Element::new_item(b"ayyb".to_vec()),
            subtree::Element::new_item(b"ayyd".to_vec()),
        ]
    );
}

#[test]
fn test_aux_uses_separate_cf() {
    let element = Element::new_item(b"ayy".to_vec());
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
    let element = Element::new_item(b"ayy".to_vec());
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
                Element::new_item(j_vec),
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
                    Element::new_item(k_vec),
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
                Element::new_item(j_vec.clone()),
                None,
            )
            .expect("successful value insert");

            db.insert(
                [TEST_LEAF, b"1", i_vec.clone().as_slice(), b"\0"],
                &random_key,
                Element::new_reference(vec![
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
            Element::new_item(i_vec),
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
            Element::new_item(i_vec.clone()),
            None,
        )
        .expect("successful value insert");

        // We should insert a reference to the item
        db.insert(
            [TEST_LEAF, b"1", i_vec.clone().as_slice()],
            b"\0",
            Element::new_reference(vec![TEST_LEAF.to_vec(), b"\0".to_vec(), i_vec.clone()]),
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
            Element::new_item(i_vec.clone()),
            None,
        )
        .expect("successful value insert");
    }
}

fn compare_result_sets(elements: &Vec<Vec<u8>>, result_set: &Vec<(Vec<u8>, Vec<u8>)>) {
    for i in 0..elements.len() {
        assert_eq!(
            deserialize_and_extract_item_bytes(&result_set[i].1).unwrap(),
            elements[i]
        )
    }
}

fn deserialize_and_extract_item_bytes(raw_bytes: &[u8]) -> Result<Vec<u8>, Error> {
    dbg!(raw_bytes);
    let elem = Element::deserialize(raw_bytes)?;
    return match elem {
        Element::Item(item, _) => Ok(item),
        _ => Err(Error::CorruptedPath("expected only item type")),
    };
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 200);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 4);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 4);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 115);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 15);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 400);
    compare_result_sets(&elements, &result_set);
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
    // using contains as the elements get stored at random key locations
    // hence impossible to predict the final location
    // but must exist
    assert!(elements.contains(&first_value));

    let mut last_value = 1995_u32.to_be_bytes().to_vec();
    last_value.append(&mut 149_u32.to_be_bytes().to_vec());
    assert!(elements.contains(&last_value));

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 400);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 8);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 250);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 5);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 500);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 10);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 550);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 750);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 11);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 200);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 50);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 100);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 200);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 60);
    compare_result_sets(&elements, &result_set);
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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 250);
    compare_result_sets(&elements, &result_set);

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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 250);
    compare_result_sets(&elements, &result_set);

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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 55);
    compare_result_sets(&elements, &result_set);

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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 60);
    compare_result_sets(&elements, &result_set);

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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 60);
    compare_result_sets(&elements, &result_set);

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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 0);

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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 250);

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

    let proof = db.prove(path_query.clone()).unwrap();
    let (hash, result_set) = GroveDb::execute_proof(&proof, path_query).unwrap();
    assert_eq!(hash, db.root_hash(None).unwrap().unwrap());
    assert_eq!(result_set.len(), 5);
    compare_result_sets(&elements, &result_set);
}

#[test]
fn test_root_hash() {
    let db = make_grovedb();
    // Check hashes are different if tree is edited
    let old_root_hash = db.root_hash(None);
    db.insert(
        [TEST_LEAF],
        b"key1",
        Element::new_item(b"ayy".to_vec()),
        None,
    )
    .expect("unable to insert an item");
    assert_ne!(old_root_hash.unwrap(), db.root_hash(None).unwrap());

    // Check isolation
    let transaction = db.start_transaction();

    db.insert(
        [TEST_LEAF],
        b"key2",
        Element::new_item(b"ayy".to_vec()),
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
    let element = Element::new_item(b"ayy".to_vec());

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
