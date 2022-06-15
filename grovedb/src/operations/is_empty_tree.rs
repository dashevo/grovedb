use costs::{cost_return_on_error, CostContext, CostsExt, OperationCost};

use crate::{util::merk_optional_tx, Error, GroveDb, TransactionArg};

impl GroveDb {
    pub fn is_empty_tree<'p, P>(
        &self,
        path: P,
        transaction: TransactionArg,
    ) -> CostContext<Result<bool, Error>>
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: Clone + DoubleEndedIterator + ExactSizeIterator,
    {
        let mut cost = OperationCost::default();

        let path_iter = path.into_iter();
        cost_return_on_error!(
            &mut cost,
            self.check_subtree_exists_path_not_found(path_iter.clone(), transaction)
        );
        merk_optional_tx!(&mut cost, self.db, path_iter, transaction, subtree, {
            Ok(subtree.is_empty_tree()).wrap_with_cost(cost)
        })
    }
}
