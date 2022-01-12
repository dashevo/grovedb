use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

use merk::{
    proofs::{query::QueryItem, Query},
    Merk,
};
use storage::{
    rocksdb_storage::{OptimisticTransactionDBTransaction, PrefixedRocksDbStorage},
    RawIterator,
};

use crate::{subtree::raw_decode, Element, Error, GroveDb, PathQuery, SizedQuery};

/// Limit of possible indirections
pub(crate) const MAX_REFERENCE_HOPS: usize = 10;

impl GroveDb {
    pub fn get(
        &self,
        path: &[&[u8]],
        key: &[u8],
        transaction: Option<&OptimisticTransactionDBTransaction>,
    ) -> Result<Element, Error> {
        match self.get_raw(path, key, transaction)? {
            Element::Reference(reference_path) => {
                self.follow_reference(reference_path, transaction)
            }
            other => Ok(other),
        }
    }

    fn follow_reference(
        &self,
        mut path: Vec<Vec<u8>>,
        transaction: Option<&OptimisticTransactionDBTransaction>,
    ) -> Result<Element, Error> {
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

    /// Get tree item without following references
    pub(super) fn get_raw(
        &self,
        path: &[&[u8]],
        key: &[u8],
        transaction: Option<&OptimisticTransactionDBTransaction>,
    ) -> Result<Element, Error> {
        let subtrees = match transaction {
            None => &self.subtrees,
            Some(_) => &self.temp_subtrees,
        };

        let merk = subtrees
            .get(&Self::compress_subtree_key(path, None))
            .ok_or(Error::InvalidPath("no subtree found under that path"))?;
        Element::get(&merk, key)
    }

    pub fn get_path_queries(
        &mut self,
        path_queries: &[&PathQuery],
        transaction: Option<&OptimisticTransactionDBTransaction>,
    ) -> Result<Vec<Element>, Error> {
        let subtrees = match transaction {
            None => &self.subtrees,
            Some(_) => &self.temp_subtrees,
        };
        let mut result = Vec::new();
        for query in path_queries {
            let (query_results, _) = self.get_path_query_on_trees(query, subtrees)?;
            result.extend_from_slice(&query_results);
        }
        Ok(result)
    }

    pub fn get_path_query(
        &mut self,
        path_query: &PathQuery,
        transaction: Option<&OptimisticTransactionDBTransaction>,
    ) -> Result<(Vec<Element>, u16), Error> {
        let subtrees = match transaction {
            None => &self.subtrees,
            Some(_) => &self.temp_subtrees,
        };
        self.get_path_query_on_trees(path_query, subtrees)
    }

    fn get_path_query_on_trees(
        &self,
        path_query: &PathQuery,
        subtrees: &HashMap<Vec<u8>, Merk<PrefixedRocksDbStorage>>,
    ) -> Result<(Vec<Element>, u16), Error> {
        let path = path_query.path;
        let merk = subtrees
            .get(&Self::compress_subtree_key(path, None))
            .ok_or(Error::InvalidPath("no subtree found under that path"))?;
        Element::get_path_query(merk, path_query)
    }
}