use std::collections::HashSet;

use costs::{
    cost_return_on_error, cost_return_on_error_no_add, CostResult, CostsExt, OperationCost,
};
use storage::{rocksdb_storage::RocksDbStorage, StorageContext};

use crate::{
    query_result_type::{QueryResultElement, QueryResultElements, QueryResultType},
    util::{merk_optional_tx, storage_context_optional_tx},
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
    ) -> CostResult<Element, Error>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: DoubleEndedIterator + ExactSizeIterator + Clone,
    {
        let mut cost = OperationCost::default();

        match cost_return_on_error!(&mut cost, self.get_raw(path, key, transaction)) {
            Element::Reference(reference_path, _) => self
                .follow_reference(reference_path, transaction)
                .add_cost(cost),
            other => Ok(other).wrap_with_cost(cost),
        }
    }

    pub fn follow_reference(
        &self,
        mut path: Vec<Vec<u8>>,
        transaction: TransactionArg,
    ) -> CostResult<Element, Error> {
        let mut cost = OperationCost::default();

        let mut hops_left = MAX_REFERENCE_HOPS;
        let mut current_element;
        let mut visited = HashSet::new();

        while hops_left > 0 {
            if visited.contains(&path) {
                return Err(Error::CyclicReference).wrap_with_cost(cost);
            }
            if let Some((key, path_slice)) = path.split_last() {
                current_element = cost_return_on_error!(
                    &mut cost,
                    self.get_raw(path_slice.iter().map(|x| x.as_slice()), key, transaction)
                )
            } else {
                return Err(Error::CorruptedPath("empty path")).wrap_with_cost(cost);
            }
            visited.insert(path);
            match current_element {
                Element::Reference(reference_path, _) => path = reference_path,
                other => return Ok(other).wrap_with_cost(cost),
            }
            hops_left -= 1;
        }
        Err(Error::ReferenceLimit).wrap_with_cost(cost)
    }

    /// Get tree item without following references
    pub fn get_raw<'p, P>(
        &self,
        path: P,
        key: &'p [u8],
        transaction: TransactionArg,
    ) -> CostResult<Element, Error>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: ExactSizeIterator + DoubleEndedIterator + Clone,
    {
        let mut cost = OperationCost::default();

        let path_iter = path.into_iter();
        if path_iter.len() == 0 {
            cost_return_on_error!(
                &mut cost,
                self.check_subtree_exists_path_not_found([key], transaction)
            );
            merk_optional_tx!(&mut cost, self.db, [key], transaction, subtree, {
                subtree
                    .root_hash()
                    .map(Element::new_tree)
                    .map(Ok)
                    .add_cost(cost)
            })
        } else {
            cost_return_on_error!(
                &mut cost,
                self.check_subtree_exists_path_not_found(path_iter.clone(), transaction)
            );
            merk_optional_tx!(&mut cost, self.db, path_iter, transaction, subtree, {
                Element::get(&subtree, key).add_cost(cost)
            })
        }
    }

    /// Does tree element exist without following references
    pub fn has_raw<'p, P>(
        &self,
        path: P,
        key: &'p [u8],
        transaction: TransactionArg,
    ) -> CostResult<bool, Error>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: ExactSizeIterator + DoubleEndedIterator + Clone,
    {
        let path_iter = path.into_iter();

        // Merk's items should be written into data storage and checked accordingly
        storage_context_optional_tx!(self.db, path_iter, transaction, storage, {
            storage.flat_map(|s| s.get(key).map_err(|e| e.into()).map_ok(|x| x.is_some()))
        })
    }

    pub fn query_many(
        &self,
        path_queries: &[&PathQuery],
        transaction: TransactionArg,
    ) -> CostResult<Vec<Vec<u8>>, Error> {
        let mut cost = OperationCost::default();

        let elements = cost_return_on_error!(
            &mut cost,
            self.query_many_raw(
                path_queries,
                QueryResultType::QueryElementResultType,
                transaction
            )
        );
        let results_wrapped = elements
            .into_iter()
            .map(|result_item| match result_item {
                QueryResultElement::ElementResultItem(Element::Reference(reference_path, _)) => {
                    let maybe_item = self
                        .follow_reference(reference_path, transaction)
                        .unwrap_add_cost(&mut cost)?;
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
            .collect::<Result<Vec<Vec<u8>>, Error>>();

        results_wrapped.wrap_with_cost(cost)
    }

    pub fn query_many_raw(
        &self,
        path_queries: &[&PathQuery],
        result_type: QueryResultType,
        transaction: TransactionArg,
    ) -> CostResult<QueryResultElements, Error>
where {
        let mut cost = OperationCost::default();

        let query = cost_return_on_error!(&mut cost, PathQuery::merge(path_queries.to_vec()));
        let (result, _) =
            cost_return_on_error!(&mut cost, self.query_raw(&query, result_type, transaction));
        Ok(result).wrap_with_cost(cost)
    }

    pub fn get_proved_path_query(
        &self,
        path_query: &PathQuery,
        transaction: TransactionArg,
    ) -> CostResult<Vec<u8>, Error> {
        if transaction.is_some() {
            Err(Error::NotSupported(
                "transactions are not currently supported",
            ))
            .wrap_with_cost(Default::default())
        } else {
            self.prove_query(path_query)
        }
    }

    pub fn query(
        &self,
        path_query: &PathQuery,
        transaction: TransactionArg,
    ) -> CostResult<(Vec<Vec<u8>>, u16), Error> {
        let mut cost = OperationCost::default();

        let (elements, skipped) = cost_return_on_error!(
            &mut cost,
            self.query_raw(
                path_query,
                QueryResultType::QueryElementResultType,
                transaction
            )
        );

        let results_wrapped = elements
            .into_iter()
            .map(|result_item| match result_item {
                QueryResultElement::ElementResultItem(element) => {
                    match element {
                        Element::Reference(reference_path, _) => {
                            // While `map` on iterator is lazy, we should accumulate costs even if
                            // `collect` will end in `Err`, so we'll use
                            // external costs accumulator instead of
                            // returning costs from `map` call.
                            let maybe_item = self
                                .follow_reference(reference_path, transaction)
                                .unwrap_add_cost(&mut cost)?;

                            if let Element::Item(item, _) = maybe_item {
                                Ok(item)
                            } else {
                                Err(Error::InvalidQuery("the reference must result in an item"))
                            }
                        }
                        Element::Item(item, _) => Ok(item),
                        Element::Tree(..) => Err(Error::InvalidQuery(
                            "path_queries can only refer to items and references",
                        )),
                    }
                }
                _ => Err(Error::CorruptedCodeExecution(
                    "query returned incorrect result type",
                )),
            })
            .collect::<Result<Vec<Vec<u8>>, Error>>();

        let results = cost_return_on_error_no_add!(&cost, results_wrapped);
        Ok((results, skipped)).wrap_with_cost(cost)
    }

    pub fn query_raw(
        &self,
        path_query: &PathQuery,
        result_type: QueryResultType,
        transaction: TransactionArg,
    ) -> CostResult<(QueryResultElements, u16), Error> {
        Element::get_raw_path_query(&self.db, path_query, result_type, transaction)
    }

    fn check_subtree_exists<'p, P>(
        &self,
        path: P,
        transaction: TransactionArg,
        error: Error,
    ) -> CostResult<(), Error>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: DoubleEndedIterator + ExactSizeIterator + Clone,
    {
        let mut cost = OperationCost::default();

        let path_iter = path.into_iter();
        if path_iter.len() == 0 {
            return Ok(()).wrap_with_cost(cost);
        }

        let mut parent_iter = path_iter;
        let parent_key = parent_iter.next_back().expect("path is not empty");
        merk_optional_tx!(&mut cost, self.db, parent_iter, transaction, parent, {
            match Element::get(&parent, parent_key).unwrap_add_cost(&mut cost) {
                Ok(Element::Tree(..)) => {}
                Ok(_) | Err(Error::PathKeyNotFound(_)) => return Err(error).wrap_with_cost(cost),
                Err(e) => return Err(e).wrap_with_cost(cost),
            }
        });

        Ok(()).wrap_with_cost(cost)
    }

    pub fn check_subtree_exists_path_not_found<'p, P>(
        &self,
        path: P,
        transaction: TransactionArg,
    ) -> CostResult<(), Error>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: DoubleEndedIterator + ExactSizeIterator + Clone,
    {
        self.check_subtree_exists(
            path,
            transaction,
            Error::PathNotFound("subtree doesn't exist"),
        )
    }

    pub fn check_subtree_exists_invalid_path<'p, P>(
        &self,
        path: P,
        transaction: TransactionArg,
    ) -> CostResult<(), Error>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: DoubleEndedIterator + ExactSizeIterator + Clone,
    {
        self.check_subtree_exists(
            path,
            transaction,
            Error::InvalidPath("subtree doesn't exist"),
        )
    }

    /// Does tree element exist without following references
    pub fn worst_case_for_has_raw<'p, P>(&self, path: P, key: &'p [u8]) -> CostResult<bool, Error>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: ExactSizeIterator + DoubleEndedIterator + Clone,
    {
        let mut cost = OperationCost::default();

        // First we get the merk tree
        Self::add_worst_case_get_merk::<_, RocksDbStorage>(
            &mut cost,
            path,
            crate::MAX_ELEMENT_SIZE,
        ); // TODO: use GroveDb's storage type parameter when it will be finally abstract
           // over it
        Self::add_worst_case_merk_has_element(&mut cost, key, crate::MAX_ELEMENT_SIZE);

        // In the worst case, there will not be an error, but the item will not be found
        Ok(false).wrap_with_cost(cost)
    }
}
