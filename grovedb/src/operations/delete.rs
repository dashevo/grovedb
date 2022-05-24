use storage::StorageContext;

use crate::{
    util::{merk_optional_tx, storage_context_optional_tx},
    Element, Error, GroveDb, TransactionArg,
};

impl GroveDb {
    pub fn delete_up_tree_while_empty<'p, P>(
        &self,
        path: P,
        key: &'p [u8],
        stop_path_height: Option<u16>,
        transaction: TransactionArg,
    ) -> Result<u16, Error>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: DoubleEndedIterator + ExactSizeIterator + Clone,
    {
        let mut path_iter = path.into_iter();
        self.check_subtree_exists_path_not_found(path_iter.clone(), Some(key), transaction)?;
        if let Some(stop_path_height) = stop_path_height {
            if stop_path_height == path_iter.clone().len() as u16 {
                return Ok(0);
            }
        }
        if !self.delete_internal(path_iter.clone(), key, true, transaction)? {
            return Ok(0);
        }
        let mut delete_count: u16 = 1;
        if let Some(last) = path_iter.next_back() {
            let deleted_parent =
                self.delete_up_tree_while_empty(path_iter, last, stop_path_height, transaction)?;
            delete_count += deleted_parent;
        }
        Ok(delete_count)
    }

    pub fn delete<'p, P>(
        &self,
        path: P,
        key: &'p [u8],
        transaction: TransactionArg,
    ) -> Result<(), Error>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: DoubleEndedIterator + ExactSizeIterator + Clone,
    {
        self.delete_internal(path, key, false, transaction)?;
        Ok(())
    }

    pub fn delete_if_empty_tree<'p, P>(
        &self,
        path: P,
        key: &'p [u8],
        transaction: TransactionArg,
    ) -> Result<bool, Error>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: DoubleEndedIterator + ExactSizeIterator + Clone,
    {
        self.delete_internal(path, key, true, transaction)
    }

    fn delete_internal<'p, P>(
        &self,
        path: P,
        key: &'p [u8],
        only_delete_tree_if_empty: bool,
        transaction: TransactionArg,
    ) -> Result<bool, Error>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: DoubleEndedIterator + ExactSizeIterator + Clone,
    {
        let path_iter = path.into_iter();
        if path_iter.len() == 0 {
            // Attempt to delete a root tree leaf
            Err(Error::InvalidPath(
                "root tree leaves currently cannot be deleted",
            ))
        } else {
            self.check_subtree_exists_path_not_found(path_iter.clone(), Some(key), transaction)?;
            let element = self.get_raw(path_iter.clone(), key.as_ref(), transaction)?;
            let delete_element = || -> Result<(), Error> {
                merk_optional_tx!(self.db, path_iter.clone(), transaction, mut parent_merk, {
                    Element::delete(&mut parent_merk, &key)?;
                    Ok(())
                })
            };

            if let Element::Tree(_) = element {
                let subtree_merk_path = path_iter.clone().chain(std::iter::once(key));
                let subtrees_paths = self.find_subtrees(subtree_merk_path.clone(), transaction)?;
                let is_empty =
                    merk_optional_tx!(self.db, subtree_merk_path, transaction, subtree, {
                        subtree.is_empty_tree()
                    });

                if only_delete_tree_if_empty && !is_empty {
                    return Ok(false);
                } else {
                    // TODO: dumb traversal should not be tolerated
                    for subtree_path in subtrees_paths {
                        merk_optional_tx!(
                            self.db,
                            subtree_path.iter().map(|x| x.as_slice()),
                            transaction,
                            mut subtree,
                            {
                                subtree.clear().map_err(|e| {
                                    Error::CorruptedData(format!(
                                        "unable to cleanup tree from storage: {}",
                                        e
                                    ))
                                })?;
                            }
                        );
                    }
                    delete_element()?;
                }
            } else {
                fn to_slice(x: &Vec<u8>) -> &[u8] {
                    x.as_slice()
                }

                if let Element::Reference(ref base_element_path, _) = element {
                    let (base_element_key, base_element_subtree_path) = base_element_path
                        .split_last()
                        .expect("reference path cannot be empty");

                    let base_element_subtree_path_as_slice =
                        base_element_subtree_path.iter().map(to_slice);
                    let mut base_element = self.get(
                        base_element_subtree_path_as_slice.clone(),
                        base_element_key,
                        transaction,
                    )?;

                    match base_element {
                        Element::Tree(_) => {}
                        Element::Item(_, ref mut references)
                        | Element::Reference(_, ref mut references) => {
                            let mut path_owned: Vec<Vec<u8>> =
                                path_iter.clone().map(|x| x.to_vec()).collect();
                            path_owned.push(key.to_vec());
                            let position = references.iter().position(|path| path == &path_owned);
                            if let Some(index) = position {
                                references.remove(index);
                                merk_optional_tx!(
                                    self.db,
                                    base_element_subtree_path_as_slice,
                                    transaction,
                                    mut subtree,
                                    {
                                        base_element.insert(&mut subtree, base_element_key)?;
                                    }
                                );
                            }
                        }
                    }
                }

                if let Element::Item(_, references) | Element::Reference(_, references) = element {
                    // recursively delete all references of to be deleted element
                    for reference in references {
                        let (referenced_key, reference_subtree_path) = reference
                            .split_last()
                            .expect("reference path cannot be empty");
                        self.delete_internal(
                            reference_subtree_path.iter().map(to_slice),
                            referenced_key,
                            false,
                            transaction,
                        )?;
                    }
                }

                delete_element()?;
            }
            self.propagate_changes(path_iter, transaction)?;
            Ok(true)
        }
    }

    // TODO: dumb traversal should not be tolerated
    /// Finds keys which are trees for a given subtree recursively.
    /// One element means a key of a `merk`, n > 1 elements mean relative path
    /// for a deeply nested subtree.
    pub(crate) fn find_subtrees<'p, P>(
        &self,
        path: P,
        transaction: TransactionArg,
    ) -> Result<Vec<Vec<Vec<u8>>>, Error>
    where
        P: IntoIterator<Item = &'p [u8]>,
    {
        // TODO: remove conversion to vec;
        // However, it's not easy for a reason:
        // new keys to enqueue are taken from raw iterator which returns Vec<u8>;
        // changing that to slice is hard as cursor should be moved for next iteration
        // which requires exclusive (&mut) reference, also there is no guarantee that
        // slice which points into storage internals will remain valid if raw iterator
        // got altered so why that reference should be exclusive;

        let mut queue: Vec<Vec<Vec<u8>>> =
            vec![path.into_iter().map(|x| x.as_ref().to_vec()).collect()];
        let mut result: Vec<Vec<Vec<u8>>> = queue.clone();

        while let Some(q) = queue.pop() {
            // Get the correct subtree with q_ref as path
            let path_iter = q.iter().map(|x| x.as_slice());
            storage_context_optional_tx!(self.db, path_iter.clone(), transaction, storage, {
                let mut raw_iter = Element::iterator(storage.raw_iter());
                while let Some((key, value)) = raw_iter.next()? {
                    if let Element::Tree(_) = value {
                        let mut sub_path = q.clone();
                        sub_path.push(key.to_vec());
                        queue.push(sub_path.clone());
                        result.push(sub_path);
                    }
                }
            })
        }
        Ok(result)
    }
}
