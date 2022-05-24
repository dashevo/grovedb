use std::collections::HashSet;

use crate::{
    util::{merk_optional_tx, meta_storage_context_optional_tx},
    Element, Error, GroveDb, PathQuery, TransactionArg,
};

/// Limit of possible indirections
pub const MAX_REFERENCE_HOPS: usize = 10;

impl GroveDb {
    pub fn get<'p, P>(
        &self,
        path: P,
        key: &'p [u8],
        transaction: TransactionArg,
    ) -> Result<Element, Error>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: DoubleEndedIterator + ExactSizeIterator + Clone,
    {
        match self.get_raw(path, key, transaction)? {
            Element::Reference(reference_path, _) => {
                self.follow_reference(reference_path, transaction)
            }
            other => Ok(other),
        }
    }

    pub fn follow_reference(
        &self,
        mut path: Vec<Vec<u8>>,
        transaction: TransactionArg,
    ) -> Result<Element, Error> {
        let mut hops_left = MAX_REFERENCE_HOPS;
        let mut current_element;
        let mut visited = HashSet::new();

        while hops_left > 0 {
            if visited.contains(&path) {
                return Err(Error::CyclicReference);
            }
            if let Some((key, path_slice)) = path.split_last() {
                current_element =
                    self.get_raw(path_slice.iter().map(|x| x.as_slice()), key, transaction)?;
            } else {
                return Err(Error::CorruptedPath("empty path"));
            }
            visited.insert(path);
            match current_element {
                Element::Reference(reference_path, _) => path = reference_path,
                other => return Ok(other),
            }
            hops_left -= 1;
        }
        Err(Error::ReferenceLimit)
    }

    /// Get tree item without following references
    pub(super) fn get_raw<'p, P>(
        &self,
        path: P,
        key: &'p [u8],
        transaction: TransactionArg,
    ) -> Result<Element, Error>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: ExactSizeIterator + DoubleEndedIterator + Clone,
    {
        let path_iter = path.into_iter();
        self.check_subtree_exists_path_not_found(path_iter.clone(), Some(key), transaction)?;
        if path_iter.len() == 0 {
            merk_optional_tx!(self.db, [key], transaction, subtree, {
                Ok(Element::Tree(subtree.root_hash()))
            })
        } else {
            merk_optional_tx!(self.db, path_iter, transaction, subtree, {
                Element::get(&subtree, key)
            })
        }
    }

    pub fn get_path_queries(
        &self,
        path_queries: &[&PathQuery],
        transaction: TransactionArg,
    ) -> Result<Vec<Vec<u8>>, Error> {
        let elements = self.get_path_queries_raw(path_queries, transaction)?;
        let results = elements
            .into_iter()
            .map(|element| match element {
                Element::Reference(reference_path, _) => {
                    let maybe_item = self.follow_reference(reference_path, transaction)?;
                    if let Element::Item(item, _) = maybe_item {
                        Ok(item)
                    } else {
                        Err(Error::InvalidQuery("the reference must result in an item"))
                    }
                }
                _ => Err(Error::InvalidQuery(
                    "path_queries can only refer to references",
                )),
            })
            .collect::<Result<Vec<Vec<u8>>, Error>>()?;
        Ok(results)
    }

    pub fn get_path_queries_raw(
        &self,
        path_queries: &[&PathQuery],
        transaction: TransactionArg,
    ) -> Result<Vec<Element>, Error> {
        let mut result = Vec::new();
        for query in path_queries {
            let (query_results, _) = self.get_path_query_raw(query, transaction)?;
            result.extend_from_slice(&query_results);
        }
        Ok(result)
    }

    pub fn get_path_query(
        &self,
        path_query: &PathQuery,
        transaction: TransactionArg,
    ) -> Result<(Vec<Vec<u8>>, u16), Error> {
        let (elements, skipped) = self.get_path_query_raw(path_query, transaction)?;
        let results = elements
            .into_iter()
            .map(|element| match element {
                Element::Reference(reference_path, _) => {
                    let maybe_item = self.follow_reference(reference_path, transaction)?;
                    if let Element::Item(item, _) = maybe_item {
                        Ok(item)
                    } else {
                        Err(Error::InvalidQuery("the reference must result in an item"))
                    }
                }
                Element::Item(item, _) => Ok(item),
                Element::Tree(_) => Err(Error::InvalidQuery(
                    "path_queries can only refer to items and references",
                )),
            })
            .collect::<Result<Vec<Vec<u8>>, Error>>()?;
        Ok((results, skipped))
    }

    pub fn get_path_query_raw(
        &self,
        path_query: &PathQuery,
        transaction: TransactionArg,
    ) -> Result<(Vec<Element>, u16), Error> {
        let path_slices = path_query
            .path
            .iter()
            .map(|x| x.as_slice())
            .collect::<Vec<_>>();
        Element::get_path_query(&self.db, &path_slices, path_query, transaction)
    }

    fn check_subtree_exists<'p, P>(
        &self,
        path: P,
        key: Option<&'p [u8]>,
        transaction: TransactionArg,
        error: Error,
    ) -> Result<(), Error>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: DoubleEndedIterator + ExactSizeIterator + Clone,
    {
        let mut path_iter = path.into_iter();
        if path_iter.len() == 0 {
            meta_storage_context_optional_tx!(self.db, transaction, meta_storage, {
                let root_leaf_keys = Self::get_root_leaf_keys_internal(&meta_storage)?;
                if !root_leaf_keys.contains_key(key.ok_or(Error::MissingParameter("key"))?) {
                    return Err(error);
                }
            });
        } else if path_iter.len() == 1 {
            meta_storage_context_optional_tx!(self.db, transaction, meta_storage, {
                let root_leaf_keys = Self::get_root_leaf_keys_internal(&meta_storage)?;
                if !root_leaf_keys.contains_key(path_iter.next().expect("must contain an item")) {
                    return Err(error);
                }
            });
        } else {
            let mut parent_iter = path_iter;
            let parent_key = parent_iter.next_back().expect("path is not empty");
            merk_optional_tx!(self.db, parent_iter, transaction, parent, {
                if matches!(
                    Element::get(&parent, parent_key),
                    Err(Error::PathKeyNotFound(_))
                ) {
                    return Err(error);
                }
            });
        }
        Ok(())
    }

    pub fn check_subtree_exists_path_not_found<'p, P>(
        &self,
        path: P,
        key: Option<&'p [u8]>,
        transaction: TransactionArg,
    ) -> Result<(), Error>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: DoubleEndedIterator + ExactSizeIterator + Clone,
    {
        self.check_subtree_exists(
            path,
            key,
            transaction,
            Error::PathNotFound("subtree doesn't exist"),
        )
    }

    pub fn check_subtree_exists_invalid_path<'p, P>(
        &self,
        path: P,
        key: Option<&'p [u8]>,
        transaction: TransactionArg,
    ) -> Result<(), Error>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: DoubleEndedIterator + ExactSizeIterator + Clone,
    {
        self.check_subtree_exists(
            path,
            key,
            transaction,
            Error::InvalidPath("subtree doesn't exist"),
        )
    }
}
