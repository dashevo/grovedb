//! Module for subtrees handling.
//! Subtrees handling is isolated so basically this module is about adapting
//! Merk API to GroveDB needs.

use bincode::Options;
use integer_encoding::VarInt;
use merk::{
    proofs::{query::QueryItem, Query},
    tree::Tree,
    Op,
};
use serde::{Deserialize, Serialize};
use storage::{rocksdb_storage::RocksDbStorage, RawIterator, StorageContext};

use crate::{
    util::{merk_optional_tx, storage_context_optional_tx},
    Error, Merk, PathQuery, SizedQuery, TransactionArg,
};

/// Variants of GroveDB stored entities
/// ONLY APPEND TO THIS LIST!!! Because
/// of how serialization works.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Element {
    /// An ordinary value
    Item(Vec<u8>, Vec<Vec<Vec<u8>>>),
    /// A reference to an object by its path
    Reference(Vec<Vec<u8>>, Vec<Vec<Vec<u8>>>),
    /// A subtree, contains a root hash of the underlying Merk.
    /// Hash is stored to make Merk become different when its subtrees have
    /// changed, otherwise changes won't be reflected in parent trees.
    Tree([u8; 32]),
}

pub struct PathQueryPushArgs<'db, 'ctx, 'a>
where
    'db: 'ctx,
{
    pub storage: &'db RocksDbStorage,
    pub transaction: TransactionArg<'db, 'ctx>,
    pub key: Option<&'a [u8]>,
    pub element: Element,
    pub path: Option<&'a [&'a [u8]]>,
    pub subquery_key: Option<Vec<u8>>,
    pub subquery: Option<Query>,
    pub left_to_right: bool,
    pub results: &'a mut Vec<Element>,
    pub limit: &'a mut Option<u16>,
    pub offset: &'a mut Option<u16>,
}

impl Element {
    // TODO: improve API to avoid creation of Tree elements with uncertain state
    pub fn empty_tree() -> Element {
        Element::Tree(Default::default())
    }

    pub fn new_item(item_value: Vec<u8>) -> Self {
        Element::Item(item_value, vec![])
    }

    pub fn new_reference(reference_path: Vec<Vec<u8>>) -> Self {
        Element::Reference(reference_path, vec![])
    }

    /// Get the size of an element in bytes
    pub fn byte_size(&self) -> usize {
        match self {
            Element::Item(item, references) => {
                item.len()
                    + references
                        .iter()
                        .map(|inner| inner.iter().map(|inner| inner.len()).sum::<usize>())
                        .sum::<usize>()
            }
            Element::Reference(path_reference, references) => {
                path_reference
                    .iter()
                    .map(|inner| inner.len())
                    .sum::<usize>()
                    + references
                        .iter()
                        .map(|inner| inner.iter().map(|inner| inner.len()).sum::<usize>())
                        .sum::<usize>()
            }
            Element::Tree(_) => 32,
        }
    }

    /// Get the size of the serialization of an element in bytes
    // TODO: Fix size calculation for item type and reference type
    pub fn serialized_byte_size(&self) -> usize {
        match self {
            Element::Item(item, references) => {
                let len_of_item = item.len();
                let len_of_references = references
                    .iter()
                    .map(|inner| inner.iter().map(|inner| inner.len()).sum::<usize>())
                    .sum::<usize>();

                len_of_item
                    + len_of_item.required_space()
                    + len_of_references
                    + len_of_references.required_space()
                    + 1 // 1 for enum
            }
            Element::Reference(path_reference, references) => {
                let len_of_references = references
                    .iter()
                    .map(|inner| {
                        inner
                            .iter()
                            .map(|inner| {
                                let inner_len = inner.len();
                                inner_len + inner_len.required_space()
                            })
                            .sum::<usize>()
                    })
                    .sum::<usize>();

                path_reference
                    .iter()
                    .map(|inner| {
                        let inner_len = inner.len();
                        inner_len + inner_len.required_space()
                    })
                    .sum::<usize>()
                    + path_reference.len().required_space()
                    + len_of_references
                    + len_of_references.required_space()
                    + 1 // for the enum
            }
            Element::Tree(_) => 33, // 32 + 1 for enum
        }
    }

    /// Get the size that the element will occupy on disk
    pub fn node_byte_size(&self, key: &[u8]) -> usize {
        // todo v23: this is just an approximation for now
        let serialized_value_size = self.serialized_byte_size();
        let node_value_size = serialized_value_size + serialized_value_size.required_space();
        let key_len = key.len();
        let node_key_size = key_len + key_len.required_space();
        // Each node stores the key and value, the value hash and the key_value hash
        let node_size = node_value_size + node_key_size + 32 + 32;
        // The node will be a child of another node which stores it's key and hash
        let parent_additions = node_key_size + 32;
        let child_sizes = 2 as usize;
        node_size + parent_additions + child_sizes
    }

    /// Delete an element from Merk under a key
    pub fn delete<'db, K: AsRef<[u8]>, S: StorageContext<'db>>(
        merk: &mut Merk<S>,
        key: K,
    ) -> Result<(), Error> {
        // TODO: delete references on this element
        let batch = [(key, Op::Delete)];
        merk.apply::<_, Vec<u8>>(&batch, &[])
            .map_err(|e| Error::CorruptedData(e.to_string()))
    }

    /// Get an element from Merk under a key; path should be resolved and proper
    /// Merk should be loaded by this moment
    pub fn get<'db, K: AsRef<[u8]>, S: StorageContext<'db>>(
        merk: &Merk<S>,
        key: K,
    ) -> Result<Element, Error> {
        let element = Self::deserialize(
            merk.get(key.as_ref())
                .map_err(|e| Error::CorruptedData(e.to_string()))?
                .ok_or_else(|| {
                    Error::PathKeyNotFound(format!("key not found in Merk: {}", hex::encode(key)))
                })?
                .as_slice(),
        )
        .map_err(|_| Error::CorruptedData(String::from("unable to deserialize element")))?;
        Ok(element)
    }

    pub fn get_query(
        storage: &RocksDbStorage,
        merk_path: &[&[u8]],
        query: &Query,
        transaction: TransactionArg,
    ) -> Result<Vec<Element>, Error> {
        let sized_query = SizedQuery::new(query.clone(), None, None);
        let (elements, _) =
            Element::get_sized_query(storage, merk_path, &sized_query, transaction)?;
        Ok(elements)
    }

    fn basic_push(args: PathQueryPushArgs) -> Result<(), Error> {
        let PathQueryPushArgs {
            element,
            results,
            limit,
            offset,
            ..
        } = args;
        if offset.unwrap_or(0) == 0 {
            results.push(element);
            if let Some(limit) = limit {
                *limit -= 1;
            }
        } else if let Some(offset) = offset {
            *offset -= 1;
        }
        Ok(())
    }

    fn path_query_push(args: PathQueryPushArgs) -> Result<(), Error> {
        let PathQueryPushArgs {
            storage,
            transaction,
            key,
            element,
            path,
            subquery_key,
            subquery,
            left_to_right,
            results,
            limit,
            offset,
        } = args;
        match element {
            Element::Tree(_) => {
                let mut path_vec = path
                    .ok_or(Error::MissingParameter(
                        "the path must be provided when using a subquery key",
                    ))?
                    .to_vec();
                path_vec.push(key.ok_or(Error::MissingParameter(
                    "the key must be provided when using a subquery key",
                ))?);

                if let Some(subquery) = subquery {
                    if let Some(subquery_key) = &subquery_key {
                        path_vec.push(subquery_key.as_slice());
                    }

                    let inner_query = SizedQuery::new(subquery, *limit, *offset);
                    let path_vec_owned = path_vec.iter().map(|x| x.to_vec()).collect();
                    let inner_path_query = PathQuery::new(path_vec_owned, inner_query);

                    let (mut sub_elements, skipped) = Element::get_path_query(
                        storage,
                        &path_vec,
                        &inner_path_query,
                        transaction,
                    )?;

                    if let Some(limit) = limit {
                        *limit -= sub_elements.len() as u16;
                    }
                    if let Some(offset) = offset {
                        *offset -= skipped;
                    }
                    results.append(&mut sub_elements);
                } else if let Some(subquery_key) = subquery_key {
                    if offset.unwrap_or(0) == 0 {
                        merk_optional_tx!(
                            storage,
                            path_vec.iter().copied(),
                            transaction,
                            subtree,
                            {
                                results.push(Element::get(&subtree, subquery_key.as_slice())?);
                            }
                        );
                        if let Some(limit) = limit {
                            *limit -= 1;
                        }
                    } else if let Some(offset) = offset {
                        *offset -= 1;
                    }
                } else {
                    return Err(Error::InvalidPath(
                        "you must provide a subquery or a subquery_key when interacting with a \
                         tree of trees",
                    ));
                }
            }
            _ => {
                Element::basic_push(PathQueryPushArgs {
                    storage,
                    transaction,
                    key,
                    element,
                    path,
                    subquery_key,
                    subquery,
                    left_to_right,
                    results,
                    limit,
                    offset,
                })?;
            }
        }
        Ok(())
    }

    pub fn subquery_paths_for_sized_query(
        sized_query: &SizedQuery,
        key: &[u8],
    ) -> (Option<Vec<u8>>, Option<Query>) {
        for (query_item, subquery_branch) in &sized_query.query.conditional_subquery_branches {
            if query_item.contains(key) {
                let subquery_key = subquery_branch.subquery_key.clone();
                let subquery = subquery_branch
                    .subquery
                    .as_ref()
                    .map(|query| *query.clone());
                return (subquery_key, subquery);
            }
        }
        let subquery_key = sized_query
            .query
            .default_subquery_branch
            .subquery_key
            .clone();
        let subquery = sized_query
            .query
            .default_subquery_branch
            .subquery
            .as_ref()
            .map(|query| *query.clone());
        (subquery_key, subquery)
    }

    fn query_item(
        storage: &RocksDbStorage,
        item: &QueryItem,
        results: &mut Vec<Element>,
        merk_path: &[&[u8]],
        sized_query: &SizedQuery,
        path: Option<&[&[u8]]>,
        transaction: TransactionArg,
        limit: &mut Option<u16>,
        offset: &mut Option<u16>,
        add_element_function: fn(PathQueryPushArgs) -> Result<(), Error>,
    ) -> Result<(), Error> {
        if !item.is_range() {
            // this is a query on a key
            if let QueryItem::Key(key) = item {
                let element_res =
                    merk_optional_tx!(storage, merk_path.iter().copied(), transaction, subtree, {
                        Element::get(&subtree, key)
                    });
                match element_res {
                    Ok(element) => {
                        let (subquery_key, subquery) =
                            Self::subquery_paths_for_sized_query(sized_query, key);
                        add_element_function(PathQueryPushArgs {
                            storage,
                            transaction,
                            key: Some(key.as_slice()),
                            element,
                            path,
                            subquery_key,
                            subquery,
                            left_to_right: sized_query.query.left_to_right,
                            results,
                            limit,
                            offset,
                        })
                    }
                    Err(Error::PathKeyNotFound(_)) => Ok(()),
                    Err(e) => Err(e),
                }
            } else {
                Err(Error::InternalError(
                    "QueryItem must be a Key if not a range",
                ))
            }
        } else {
            // this is a query on a range
            storage_context_optional_tx!(storage, merk_path.iter().copied(), transaction, ctx, {
                let mut iter = ctx.raw_iter();

                item.seek_for_iter(&mut iter, sized_query.query.left_to_right);

                while item.iter_is_valid_for_type(&iter, *limit, sized_query.query.left_to_right) {
                    let element =
                        raw_decode(iter.value().expect("if key exists then value should too"))?;
                    let key = iter.key().expect("key should exist");
                    let (subquery_key, subquery) =
                        Self::subquery_paths_for_sized_query(sized_query, key);
                    add_element_function(PathQueryPushArgs {
                        storage,
                        transaction,
                        key: Some(key),
                        element,
                        path,
                        subquery_key,
                        subquery,
                        left_to_right: sized_query.query.left_to_right,
                        results,
                        limit,
                        offset,
                    })?;
                    if sized_query.query.left_to_right {
                        iter.next();
                    } else {
                        iter.prev();
                    }
                }
                Ok(())
            })
        }
    }

    pub fn get_query_apply_function(
        storage: &RocksDbStorage,
        merk_path: &[&[u8]],
        sized_query: &SizedQuery,
        path: Option<&[&[u8]]>,
        transaction: TransactionArg,
        add_element_function: fn(PathQueryPushArgs) -> Result<(), Error>,
    ) -> Result<(Vec<Element>, u16), Error> {
        let mut results = Vec::new();

        let mut limit = sized_query.limit;
        let original_offset = sized_query.offset;
        let mut offset = original_offset;

        if sized_query.query.left_to_right {
            for item in sized_query.query.iter() {
                Self::query_item(
                    storage,
                    item,
                    &mut results,
                    merk_path,
                    sized_query,
                    path,
                    transaction,
                    &mut limit,
                    &mut offset,
                    add_element_function,
                )?;
                if limit == Some(0) {
                    break;
                }
            }
        } else {
            for item in sized_query.query.rev_iter() {
                Self::query_item(
                    storage,
                    item,
                    &mut results,
                    merk_path,
                    sized_query,
                    path,
                    transaction,
                    &mut limit,
                    &mut offset,
                    add_element_function,
                )?;
                if limit == Some(0) {
                    break;
                }
            }
        }

        let skipped = if let Some(original_offset_unwrapped) = original_offset {
            original_offset_unwrapped - offset.unwrap()
        } else {
            0
        };
        Ok((results, skipped))
    }

    // Returns a vector of elements, and the number of skipped elements
    pub fn get_path_query(
        storage: &RocksDbStorage,
        merk_path: &[&[u8]],
        path_query: &PathQuery,
        transaction: TransactionArg,
    ) -> Result<(Vec<Element>, u16), Error> {
        let path_slices = path_query
            .path
            .iter()
            .map(|x| x.as_slice())
            .collect::<Vec<_>>();
        Element::get_query_apply_function(
            storage,
            merk_path,
            &path_query.query,
            Some(path_slices.as_slice()),
            transaction,
            Element::path_query_push,
        )
    }

    /// Returns a vector of elements, and the number of skipped elements
    pub fn get_sized_query(
        storage: &RocksDbStorage,
        merk_path: &[&[u8]],
        sized_query: &SizedQuery,
        transaction: TransactionArg,
    ) -> Result<(Vec<Element>, u16), Error> {
        Element::get_query_apply_function(
            storage,
            merk_path,
            sized_query,
            None,
            transaction,
            Element::path_query_push,
        )
    }

    /// Insert an element in Merk under a key; path should be resolved and
    /// proper Merk should be loaded by this moment
    /// If transaction is not passed, the batch will be written immediately.
    /// If transaction is passed, the operation will be committed on the
    /// transaction commit.
    pub fn insert<'db, K: AsRef<[u8]>, S: StorageContext<'db>>(
        &self,
        merk: &mut Merk<S>,
        key: K,
    ) -> Result<(), Error> {
        let batch_operations = [(key, Op::Put(self.serialize()?))];
        merk.apply::<_, Vec<u8>>(&batch_operations, &[])
            .map_err(|e| Error::CorruptedData(e.to_string()))
    }

    /// Insert a reference element in Merk under a key; path should be resolved
    /// and proper Merk should be loaded by this moment
    /// If transaction is not passed, the batch will be written immediately.
    /// If transaction is passed, the operation will be committed on the
    /// transaction commit.
    pub fn insert_reference<'db, K: AsRef<[u8]>, S: StorageContext<'db>>(
        &self,
        merk: &mut Merk<S>,
        key: K,
        referenced_value: Vec<u8>,
    ) -> Result<(), Error> {
        let batch_operations = [(key, Op::PutReference(self.serialize()?, referenced_value))];
        merk.apply::<_, Vec<u8>>(&batch_operations, &[])
            .map_err(|e| Error::CorruptedData(e.to_string()))
    }

    pub fn serialize(&self) -> Result<Vec<u8>, Error> {
        bincode::DefaultOptions::default()
            .with_varint_encoding()
            .reject_trailing_bytes()
            .serialize(self)
            .map_err(|_| Error::CorruptedData(String::from("unable to serialize element")))
    }

    pub fn deserialize(bytes: &[u8]) -> Result<Self, Error> {
        bincode::DefaultOptions::default()
            .with_varint_encoding()
            .reject_trailing_bytes()
            .deserialize(bytes)
            .map_err(|_| Error::CorruptedData(String::from("unable to deserialize element")))
    }

    pub fn iterator<I: RawIterator>(mut raw_iter: I) -> ElementsIterator<I> {
        raw_iter.seek_to_first();
        ElementsIterator::new(raw_iter)
    }
}

pub struct ElementsIterator<I: RawIterator> {
    raw_iter: I,
}

pub fn raw_decode(bytes: &[u8]) -> Result<Element, Error> {
    let tree = Tree::decode_raw(bytes).map_err(|e| Error::CorruptedData(e.to_string()))?;
    let element: Element = Element::deserialize(tree.value())?;
    Ok(element)
}

impl<I: RawIterator> ElementsIterator<I> {
    pub fn new(raw_iter: I) -> Self {
        ElementsIterator { raw_iter }
    }

    pub fn next(&mut self) -> Result<Option<(Vec<u8>, Element)>, Error> {
        Ok(if self.raw_iter.valid() {
            if let Some((key, value)) = self.raw_iter.key().zip(self.raw_iter.value()) {
                let element = raw_decode(value)?;
                let key_vec = key.to_vec();
                self.raw_iter.next();
                Some((key_vec, element))
            } else {
                None
            }
        } else {
            None
        })
    }
}

#[cfg(test)]
mod tests {
    use merk::test_utils::TempMerk;
    use storage::Storage;

    use super::*;
    use crate::tests::{make_grovedb, TEST_LEAF};

    #[test]
    fn test_success_insert() {
        let mut merk = TempMerk::new();
        Element::empty_tree()
            .insert(&mut merk, b"mykey")
            .expect("expected successful insertion");
        Element::new_item(b"value".to_vec())
            .insert(&mut merk, b"another-key")
            .expect("expected successful insertion 2");

        assert_eq!(
            Element::get(&merk, b"another-key").expect("expected successful get"),
            Element::new_item(b"value".to_vec()),
        );
    }

    #[test]
    fn test_serialization() {
        let empty_tree = Element::empty_tree();
        let serialized = empty_tree.serialize().expect("expected to serialize");
        assert_eq!(serialized.len(), 33);
        assert_eq!(serialized.len(), empty_tree.serialized_byte_size());
        // The tree is fixed length 32 bytes, so it's enum 2 then 32 bytes of zeroes
        assert_eq!(
            hex::encode(serialized),
            "020000000000000000000000000000000000000000000000000000000000000000"
        );

        let item = Element::new_item(hex::decode("abcdef").expect("expected to decode"));
        let serialized = item.serialize().expect("expected to serialize");
        assert_eq!(serialized.len(), 6);
        assert_eq!(serialized.len(), item.serialized_byte_size());
        // The item is variable length 3 bytes, so it's enum 2 then 32 bytes of zeroes
        assert_eq!(hex::encode(serialized), "0003abcdef00");

        let reference = Element::new_reference(vec![
            vec![0],
            hex::decode("abcd").expect("expected to decode"),
            vec![5],
        ]);
        let serialized = reference.serialize().expect("expected to serialize");
        assert_eq!(serialized.len(), 10);
        assert_eq!(serialized.len(), reference.serialized_byte_size());
        // The item is variable length 2 bytes, so it's enum 1 then 1 byte for length,
        // then 1 byte for 0, then 1 byte 02 for abcd, then 1 byte '1' for 05
        assert_eq!(hex::encode(serialized), "0103010002abcd010500");
    }

    #[test]
    fn test_get_query() {
        let db = make_grovedb();

        let storage = &db.db;
        let storage_context = storage.get_storage_context([TEST_LEAF]);
        let mut merk = Merk::open(storage_context).expect("cannot open Merk");

        Element::new_item(b"ayyd".to_vec())
            .insert(&mut merk, b"d")
            .expect("expected successful insertion");
        Element::new_item(b"ayyc".to_vec())
            .insert(&mut merk, b"c")
            .expect("expected successful insertion");
        Element::new_item(b"ayya".to_vec())
            .insert(&mut merk, b"a")
            .expect("expected successful insertion");
        Element::new_item(b"ayyb".to_vec())
            .insert(&mut merk, b"b")
            .expect("expected successful insertion");

        // Test queries by key
        let mut query = Query::new();
        query.insert_key(b"c".to_vec());
        query.insert_key(b"a".to_vec());
        assert_eq!(
            Element::get_query(&storage, &[TEST_LEAF], &query, None)
                .expect("expected successful get_query"),
            vec![
                Element::new_item(b"ayya".to_vec()),
                Element::new_item(b"ayyc".to_vec())
            ]
        );

        // Test range query
        let mut query = Query::new();
        query.insert_range(b"b".to_vec()..b"d".to_vec());
        query.insert_range(b"a".to_vec()..b"c".to_vec());
        assert_eq!(
            Element::get_query(&storage, &[TEST_LEAF], &query, None)
                .expect("expected successful get_query"),
            vec![
                Element::new_item(b"ayya".to_vec()),
                Element::new_item(b"ayyb".to_vec()),
                Element::new_item(b"ayyc".to_vec())
            ]
        );

        // Test range inclusive query
        let mut query = Query::new();
        query.insert_range_inclusive(b"b".to_vec()..=b"d".to_vec());
        query.insert_range(b"b".to_vec()..b"c".to_vec());
        assert_eq!(
            Element::get_query(&storage, &[TEST_LEAF], &query, None)
                .expect("expected successful get_query"),
            vec![
                Element::new_item(b"ayyb".to_vec()),
                Element::new_item(b"ayyc".to_vec()),
                Element::new_item(b"ayyd".to_vec())
            ]
        );

        // Test overlaps
        let mut query = Query::new();
        query.insert_key(b"a".to_vec());
        query.insert_range(b"b".to_vec()..b"d".to_vec());
        query.insert_range(b"a".to_vec()..b"c".to_vec());
        assert_eq!(
            Element::get_query(&storage, &[TEST_LEAF], &query, None)
                .expect("expected successful get_query"),
            vec![
                Element::new_item(b"ayya".to_vec()),
                Element::new_item(b"ayyb".to_vec()),
                Element::new_item(b"ayyc".to_vec())
            ]
        );
    }

    #[test]
    fn test_get_range_query() {
        let db = make_grovedb();

        let storage = &db.db;
        let storage_context = storage.get_storage_context([TEST_LEAF]);
        let mut merk = Merk::open(storage_context).expect("cannot open Merk");

        Element::new_item(b"ayyd".to_vec())
            .insert(&mut merk, b"d")
            .expect("expected successful insertion");
        Element::new_item(b"ayyc".to_vec())
            .insert(&mut merk, b"c")
            .expect("expected successful insertion");
        Element::new_item(b"ayya".to_vec())
            .insert(&mut merk, b"a")
            .expect("expected successful insertion");
        Element::new_item(b"ayyb".to_vec())
            .insert(&mut merk, b"b")
            .expect("expected successful insertion");

        // Test range inclusive query
        let mut query = Query::new();
        query.insert_range(b"a".to_vec()..b"d".to_vec());

        let ascending_query = SizedQuery::new(query.clone(), None, None);
        let (elements, skipped) =
            Element::get_sized_query(&storage, &[TEST_LEAF], &ascending_query, None)
                .expect("expected successful get_query");
        assert_eq!(
            elements,
            vec![
                Element::new_item(b"ayya".to_vec()),
                Element::new_item(b"ayyb".to_vec()),
                Element::new_item(b"ayyc".to_vec()),
            ]
        );
        assert_eq!(skipped, 0);

        query.left_to_right = false;

        let backwards_query = SizedQuery::new(query.clone(), None, None);
        let (elements, skipped) =
            Element::get_sized_query(&storage, &[TEST_LEAF], &backwards_query, None)
                .expect("expected successful get_query");
        assert_eq!(
            elements,
            vec![
                Element::new_item(b"ayyc".to_vec()),
                Element::new_item(b"ayyb".to_vec()),
                Element::new_item(b"ayya".to_vec()),
            ]
        );
        assert_eq!(skipped, 0);
    }

    #[test]
    fn test_get_range_inclusive_query() {
        let db = make_grovedb();

        let storage = &db.db;
        let storage_context = storage.get_storage_context([TEST_LEAF]);
        let mut merk = Merk::open(storage_context).expect("cannot open Merk");

        Element::new_item(b"ayyd".to_vec())
            .insert(&mut merk, b"d")
            .expect("expected successful insertion");
        Element::new_item(b"ayyc".to_vec())
            .insert(&mut merk, b"c")
            .expect("expected successful insertion");
        Element::new_item(b"ayya".to_vec())
            .insert(&mut merk, b"a")
            .expect("expected successful insertion");
        Element::new_item(b"ayyb".to_vec())
            .insert(&mut merk, b"b")
            .expect("expected successful insertion");

        // Test range inclusive query
        let mut query = Query::new_with_direction(true);
        query.insert_range_inclusive(b"a".to_vec()..=b"d".to_vec());

        let ascending_query = SizedQuery::new(query.clone(), None, None);
        fn check_elements_no_skipped((elements, skipped): (Vec<Element>, u16), reverse: bool) {
            let mut expected = vec![
                Element::new_item(b"ayya".to_vec()),
                Element::new_item(b"ayyb".to_vec()),
                Element::new_item(b"ayyc".to_vec()),
                Element::new_item(b"ayyd".to_vec()),
            ];
            if reverse {
                expected.reverse();
            }
            assert_eq!(elements, expected);
            assert_eq!(skipped, 0);
        }

        check_elements_no_skipped(
            Element::get_sized_query(&storage, &[TEST_LEAF], &ascending_query, None)
                .expect("expected successful get_query"),
            false,
        );

        query.left_to_right = false;

        let backwards_query = SizedQuery::new(query.clone(), None, None);
        check_elements_no_skipped(
            Element::get_sized_query(&storage, &[TEST_LEAF], &backwards_query, None)
                .expect("expected successful get_query"),
            true,
        );

        // Test range inclusive query
        let mut query = Query::new_with_direction(false);
        query.insert_range_inclusive(b"b".to_vec()..=b"d".to_vec());
        query.insert_range(b"a".to_vec()..b"c".to_vec());

        let backwards_query = SizedQuery::new(query.clone(), None, None);
        check_elements_no_skipped(
            Element::get_sized_query(&storage, &[TEST_LEAF], &backwards_query, None)
                .expect("expected successful get_query"),
            true,
        );
    }

    #[test]
    fn test_get_limit_query() {
        let db = make_grovedb();

        let storage = &db.db;
        let storage_context = storage.get_storage_context([TEST_LEAF]);
        let mut merk = Merk::open(storage_context).expect("cannot open Merk");

        Element::new_item(b"ayyd".to_vec())
            .insert(&mut merk, b"d")
            .expect("expected successful insertion");
        Element::new_item(b"ayyc".to_vec())
            .insert(&mut merk, b"c")
            .expect("expected successful insertion");
        Element::new_item(b"ayya".to_vec())
            .insert(&mut merk, b"a")
            .expect("expected successful insertion");
        Element::new_item(b"ayyb".to_vec())
            .insert(&mut merk, b"b")
            .expect("expected successful insertion");

        // Test queries by key
        let mut query = Query::new_with_direction(true);
        query.insert_key(b"c".to_vec());
        query.insert_key(b"a".to_vec());

        // since these are just keys a backwards query will keep same order
        let backwards_query = SizedQuery::new(query.clone(), None, None);
        let (elements, skipped) =
            Element::get_sized_query(&storage, &[TEST_LEAF], &backwards_query, None)
                .expect("expected successful get_query");
        assert_eq!(
            elements,
            vec![
                Element::new_item(b"ayya".to_vec()),
                Element::new_item(b"ayyc".to_vec()),
            ]
        );
        assert_eq!(skipped, 0);

        // Test queries by key
        let mut query = Query::new_with_direction(false);
        query.insert_key(b"c".to_vec());
        query.insert_key(b"a".to_vec());

        // since these are just keys a backwards query will keep same order
        let backwards_query = SizedQuery::new(query.clone(), None, None);
        let (elements, skipped) =
            Element::get_sized_query(&storage, &[TEST_LEAF], &backwards_query, None)
                .expect("expected successful get_query");
        assert_eq!(
            elements,
            vec![
                Element::new_item(b"ayyc".to_vec()),
                Element::new_item(b"ayya".to_vec()),
            ]
        );
        assert_eq!(skipped, 0);

        // The limit will mean we will only get back 1 item
        let limit_query = SizedQuery::new(query.clone(), Some(1), None);
        let (elements, skipped) =
            Element::get_sized_query(&storage, &[TEST_LEAF], &limit_query, None)
                .expect("expected successful get_query");
        assert_eq!(elements, vec![Element::new_item(b"ayyc".to_vec())]);
        assert_eq!(skipped, 0);

        // Test range query
        let mut query = Query::new_with_direction(true);
        query.insert_range(b"b".to_vec()..b"d".to_vec());
        query.insert_range(b"a".to_vec()..b"c".to_vec());
        let limit_query = SizedQuery::new(query.clone(), Some(2), None);
        let (elements, skipped) =
            Element::get_sized_query(&storage, &[TEST_LEAF], &limit_query, None)
                .expect("expected successful get_query");
        assert_eq!(
            elements,
            vec![
                Element::new_item(b"ayya".to_vec()),
                Element::new_item(b"ayyb".to_vec())
            ]
        );
        assert_eq!(skipped, 0);

        let limit_offset_query = SizedQuery::new(query.clone(), Some(2), Some(1));
        let (elements, skipped) =
            Element::get_sized_query(&storage, &[TEST_LEAF], &limit_offset_query, None)
                .expect("expected successful get_query");
        assert_eq!(
            elements,
            vec![
                Element::new_item(b"ayyb".to_vec()),
                Element::new_item(b"ayyc".to_vec())
            ]
        );
        assert_eq!(skipped, 1);

        // Test range query
        let mut query = Query::new_with_direction(false);
        query.insert_range(b"b".to_vec()..b"d".to_vec());
        query.insert_range(b"a".to_vec()..b"c".to_vec());

        let limit_offset_backwards_query = SizedQuery::new(query.clone(), Some(2), Some(1));
        let (elements, skipped) =
            Element::get_sized_query(&storage, &[TEST_LEAF], &limit_offset_backwards_query, None)
                .expect("expected successful get_query");
        assert_eq!(
            elements,
            vec![
                Element::new_item(b"ayyb".to_vec()),
                Element::new_item(b"ayya".to_vec())
            ]
        );
        assert_eq!(skipped, 1);

        // Test range inclusive query
        let mut query = Query::new_with_direction(true);
        query.insert_range_inclusive(b"b".to_vec()..=b"d".to_vec());
        query.insert_range(b"b".to_vec()..b"c".to_vec());
        let limit_full_query = SizedQuery::new(query.clone(), Some(5), Some(0));
        let (elements, skipped) =
            Element::get_sized_query(&storage, &[TEST_LEAF], &limit_full_query, None)
                .expect("expected successful get_query");
        assert_eq!(
            elements,
            vec![
                Element::new_item(b"ayyb".to_vec()),
                Element::new_item(b"ayyc".to_vec()),
                Element::new_item(b"ayyd".to_vec()),
            ]
        );
        assert_eq!(skipped, 0);

        let mut query = Query::new_with_direction(false);
        query.insert_range_inclusive(b"b".to_vec()..=b"d".to_vec());
        query.insert_range(b"b".to_vec()..b"c".to_vec());

        let limit_offset_backwards_query = SizedQuery::new(query.clone(), Some(2), Some(1));
        let (elements, skipped) =
            Element::get_sized_query(&storage, &[TEST_LEAF], &limit_offset_backwards_query, None)
                .expect("expected successful get_query");
        assert_eq!(
            elements,
            vec![
                Element::new_item(b"ayyc".to_vec()),
                Element::new_item(b"ayyb".to_vec()),
            ]
        );
        assert_eq!(skipped, 1);

        // Test overlaps
        let mut query = Query::new_with_direction(false);
        query.insert_key(b"a".to_vec());
        query.insert_range(b"b".to_vec()..b"d".to_vec());
        query.insert_range(b"b".to_vec()..b"c".to_vec());
        let limit_backwards_query = SizedQuery::new(query.clone(), Some(2), Some(1));
        let (elements, skipped) =
            Element::get_sized_query(&storage, &[TEST_LEAF], &limit_backwards_query, None)
                .expect("expected successful get_query");
        assert_eq!(
            elements,
            vec![
                Element::new_item(b"ayyb".to_vec()),
                Element::new_item(b"ayya".to_vec()),
            ]
        );
        assert_eq!(skipped, 1);
    }
}
