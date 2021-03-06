#![deny(missing_docs)]
//! Interface crate to unify how operations' costs are passed and retrieved.

use std::ops::{Add, AddAssign};

use storage::rocksdb_storage::RocksDbStorage;

/// Piece of data representing affected computer resources (approximately).
#[derive(Debug, Default, Eq, PartialEq)]
pub struct OperationCost {
    /// How many storage seeks were done.
    pub seek_count: u16,
    /// How many bytes were written on hard drive.
    pub storage_written_bytes: u32,
    /// How many bytes were loaded from hard drive.
    pub storage_loaded_bytes: u32,
    /// How many bytes were loaded into memory (usually keys and values).
    pub loaded_bytes: u32,
    /// How many times hash was called for bytes (paths, keys, values).
    pub hash_byte_calls: u32,
    /// How many times node hashing was done (for merkelized tree).
    pub hash_node_calls: u16,
}

impl OperationCost {
    /// Add worst case for getting a merk tree
    pub fn add_worst_case_get_merk<'p, P>(&mut self, path: P)
    where
        P: IntoIterator<Item = &'p [u8]>,
        <P as IntoIterator>::IntoIter: ExactSizeIterator + DoubleEndedIterator + Clone,
    {
        self.seek_count += 1;
        self.storage_written_bytes += 0;
        self.storage_loaded_bytes += 0;
        self.loaded_bytes += 0;
        self.hash_byte_calls += RocksDbStorage::build_prefix_hash_count(path) as u32;
        self.hash_node_calls += 0;
    }

    /// Add worst case for getting a merk tree
    pub fn add_worst_case_merk_has_element(&mut self, key: &[u8]) {
        self.seek_count += 1;
        self.storage_written_bytes += 0;
        self.storage_loaded_bytes += 0;
        self.loaded_bytes += key.len() as u32;
        self.hash_byte_calls += 0;
        self.hash_node_calls += 0;
    }

    /// Add worst case for getting a merk tree root hash
    pub fn add_worst_case_merk_root_hash(&mut self) {
        self.seek_count += 0;
        self.storage_written_bytes += 0;
        self.storage_loaded_bytes += 0;
        self.loaded_bytes += 0;
        self.hash_byte_calls += 0;
        self.hash_node_calls += 0;
    }

    /// Add worst case for opening a root meta storage
    pub fn add_worst_case_open_root_meta_storage(&mut self) {
        self.seek_count += 0;
        self.storage_written_bytes += 0;
        self.storage_loaded_bytes += 0;
        self.loaded_bytes += 0;
        self.hash_byte_calls += 0;
        self.hash_node_calls += 0;
    }

    /// Add worst case for saving the root tree
    pub fn add_worst_case_save_root_leaves(&mut self) {
        self.seek_count += 0;
        self.storage_written_bytes += 0;
        self.storage_loaded_bytes += 0;
        self.loaded_bytes += 0;
        self.hash_byte_calls += 0;
        self.hash_node_calls += 0;
    }

    /// Add worst case for loading the root tree
    pub fn add_worst_case_load_root_leaves(&mut self) {
        self.seek_count += 0;
        self.storage_written_bytes += 0;
        self.storage_loaded_bytes += 0;
        self.loaded_bytes += 0;
        self.hash_byte_calls += 0;
        self.hash_node_calls += 0;
    }
}

impl Add for OperationCost {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        OperationCost {
            seek_count: self.seek_count + rhs.seek_count,
            storage_written_bytes: self.storage_written_bytes + rhs.storage_written_bytes,
            storage_loaded_bytes: self.storage_loaded_bytes + rhs.storage_loaded_bytes,
            loaded_bytes: self.loaded_bytes + rhs.loaded_bytes,
            hash_byte_calls: self.hash_byte_calls + rhs.hash_byte_calls,
            hash_node_calls: self.hash_node_calls + rhs.hash_node_calls,
        }
    }
}

impl AddAssign for OperationCost {
    fn add_assign(&mut self, rhs: Self) {
        self.seek_count += rhs.seek_count;
        self.storage_written_bytes += rhs.storage_written_bytes;
        self.storage_loaded_bytes += rhs.storage_loaded_bytes;
        self.loaded_bytes += rhs.loaded_bytes;
        self.hash_byte_calls += rhs.hash_byte_calls;
        self.hash_node_calls += rhs.hash_node_calls;
    }
}

/// Wrapped operation result with associated cost.
#[derive(Debug, Eq, PartialEq)]
pub struct CostContext<T> {
    /// Wrapped operation's return value.
    pub value: T,
    /// Cost of the operation.
    pub cost: OperationCost,
}

/// General combinators for `CostContext`.
impl<T> CostContext<T> {
    /// Take wrapped value out adding its cost to provided accumulator.
    pub fn unwrap_add_cost(self, acc_cost: &mut OperationCost) -> T {
        *acc_cost += self.cost;
        self.value
    }

    /// Take wrapped value out dropping cost data.
    pub fn unwrap(self) -> T {
        self.value
    }

    /// Borrow costs data.
    pub fn cost(&self) -> &OperationCost {
        &self.cost
    }

    /// Borrow wrapped data.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Applies function to wrapped value keeping cost the same as before.
    pub fn map<B>(self, f: impl FnOnce(T) -> B) -> CostContext<B> {
        let cost = self.cost;
        let value = f(self.value);
        CostContext { value, cost }
    }

    /// Applies function to wrapped value adding costs.
    pub fn flat_map<B>(self, f: impl FnOnce(T) -> CostContext<B>) -> CostContext<B> {
        let mut cost = self.cost;
        let value = f(self.value).unwrap_add_cost(&mut cost);
        CostContext { value, cost }
    }

    /// Adds previously accumulated cost
    pub fn add_cost(mut self, cost: OperationCost) -> Self {
        self.cost += cost;
        self
    }
}

/// Type alias for `Result` wrapped into `CostContext`.
pub type CostResult<T, E> = CostContext<Result<T, E>>;

/// Combinators to use with `Result` wrapped in `CostContext`.
impl<T, E> CostResult<T, E> {
    /// Applies function to wrapped value in case of `Ok` keeping cost the same
    /// as before.
    pub fn map_ok<B>(self, f: impl FnOnce(T) -> B) -> CostResult<B, E> {
        self.map(|result| result.map(f))
    }

    /// Applies function to wrapped value in case of `Err` keeping cost the same
    /// as before.
    pub fn map_err<B>(self, f: impl FnOnce(E) -> B) -> CostResult<T, B> {
        self.map(|result| result.map_err(f))
    }

    /// Applies function to wrapped result in case of `Ok` adding costs.
    pub fn flat_map_ok<B>(self, f: impl FnOnce(T) -> CostResult<B, E>) -> CostResult<B, E> {
        let mut cost = self.cost;
        let result = match self.value {
            Ok(x) => f(x).unwrap_add_cost(&mut cost),
            Err(e) => Err(e),
        };
        CostContext {
            value: result,
            cost,
        }
    }
}

impl<T, E> CostResult<Result<T, E>, E> {
    /// Flattens nested errors inside `CostContext`
    pub fn flatten(self) -> CostResult<T, E> {
        self.map(|value| match value {
            Err(e) => Err(e),
            Ok(Err(e)) => Err(e),
            Ok(Ok(v)) => Ok(v),
        })
    }
}

impl<T> CostContext<CostContext<T>> {
    /// Flattens nested `CostContext`s adding costs.
    pub fn flatten(self) -> CostContext<T> {
        let mut cost = OperationCost::default();
        let inner = self.unwrap_add_cost(&mut cost);
        inner.add_cost(cost)
    }
}

/// Extension trait to add costs context to values.
pub trait CostsExt {
    /// Wraps any value into a `CostContext` object with provided costs.
    fn wrap_with_cost(self, cost: OperationCost) -> CostContext<Self>
    where
        Self: Sized,
    {
        CostContext { value: self, cost }
    }

    /// Wraps any value into `CostContext` object with costs computed using the
    /// value getting wrapped.
    fn wrap_fn_cost(self, f: impl FnOnce(&Self) -> OperationCost) -> CostContext<Self>
    where
        Self: Sized,
    {
        CostContext {
            cost: f(&self),
            value: self,
        }
    }
}

impl<T> CostsExt for T {}

/// Macro to achieve a kind of what `?` operator does, but with `CostContext` on
/// top. Main properties are:
/// 1. Early termination on error;
/// 2. Because of 1. `Result` is removed from the equation;
/// 3. `CostContext` if removed too because it is added to external cost
///    accumulator;
/// 4. Early termination uses external cost accumulator so previous
///    costs won't be lost.
#[macro_export]
macro_rules! cost_return_on_error {
    ( &mut $cost:ident, $($body:tt)+ ) => {
        {
            use $crate::CostsExt;
            let result_with_cost = { $($body)+ };
            let result = result_with_cost.unwrap_add_cost(&mut $cost);
            match result {
                Ok(x) => x,
                Err(e) => return Err(e).wrap_with_cost($cost),
            }
        }
    };
}

/// Macro to achieve a kind of what `?` operator does, but with `CostContext` on
/// top. The difference between this macro and `cost_return_on_error` is that it
/// is intended to use it on `Result` rather than `CostContext<Result<..>>`, so
/// no costs will be added except previously accumulated.
#[macro_export]
macro_rules! cost_return_on_error_no_add {
    ( &$cost:ident, $($body:tt)+ ) => {
        {
            use $crate::CostsExt;
            let result = { $($body)+ };
            match result {
                Ok(x) => x,
                Err(e) => return Err(e).wrap_with_cost($cost),
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map() {
        let initial = CostContext {
            value: 75,
            cost: OperationCost {
                loaded_bytes: 3,
                ..Default::default()
            },
        };

        let mapped = initial.map(|x| x + 25);
        assert_eq!(
            mapped,
            CostContext {
                value: 100,
                cost: OperationCost {
                    loaded_bytes: 3,
                    ..Default::default()
                },
            }
        );
    }

    #[test]
    fn test_flat_map() {
        let initial = CostContext {
            value: 75,
            cost: OperationCost {
                loaded_bytes: 3,
                ..Default::default()
            },
        };

        let mapped = initial.flat_map(|x| CostContext {
            value: x + 25,
            cost: OperationCost {
                loaded_bytes: 7,
                ..Default::default()
            },
        });
        assert_eq!(
            mapped,
            CostContext {
                value: 100,
                cost: OperationCost {
                    loaded_bytes: 10,
                    ..Default::default()
                },
            }
        );
    }

    #[test]
    fn test_map_ok() {
        let initial: CostResult<usize, ()> = CostContext {
            value: Ok(75),
            cost: OperationCost {
                loaded_bytes: 3,
                ..Default::default()
            },
        };

        let mapped = initial.map_ok(|x| x + 25);
        assert_eq!(
            mapped,
            CostContext {
                value: Ok(100),
                cost: OperationCost {
                    loaded_bytes: 3,
                    ..Default::default()
                },
            }
        );
    }

    #[test]
    fn test_map_ok_err() {
        let initial: CostResult<usize, ()> = CostContext {
            value: Err(()),
            cost: OperationCost {
                loaded_bytes: 3,
                ..Default::default()
            },
        };

        let mapped = initial.map_ok(|x| x + 25);
        assert_eq!(
            mapped,
            CostContext {
                value: Err(()),
                cost: OperationCost {
                    loaded_bytes: 3,
                    ..Default::default()
                },
            }
        );
    }

    #[test]
    fn test_flat_map_ok() {
        let initial: CostResult<usize, ()> = CostContext {
            value: Ok(75),
            cost: OperationCost {
                loaded_bytes: 3,
                ..Default::default()
            },
        };

        let mapped = initial.flat_map_ok(|x| CostContext {
            value: Ok(x + 25),
            cost: OperationCost {
                loaded_bytes: 7,
                ..Default::default()
            },
        });
        assert_eq!(
            mapped,
            CostContext {
                value: Ok(100),
                cost: OperationCost {
                    loaded_bytes: 10,
                    ..Default::default()
                },
            }
        );
    }

    #[test]
    fn test_flat_map_err_first() {
        let initial: CostResult<usize, ()> = CostContext {
            value: Err(()),
            cost: OperationCost {
                loaded_bytes: 3,
                ..Default::default()
            },
        };
        let mut executed = false;
        let mapped = initial.flat_map_ok(|x| {
            executed = true;
            CostContext {
                value: Ok(x + 25),
                cost: OperationCost {
                    loaded_bytes: 7,
                    ..Default::default()
                },
            }
        });

        // Second function won't be executed and thus no costs added.
        assert!(!executed);
        assert_eq!(
            mapped,
            CostContext {
                value: Err(()),
                cost: OperationCost {
                    loaded_bytes: 3,
                    ..Default::default()
                },
            }
        );
    }

    #[test]
    fn test_flat_map_err_second() {
        let initial: CostResult<usize, ()> = CostContext {
            value: Ok(75),
            cost: OperationCost {
                loaded_bytes: 3,
                ..Default::default()
            },
        };
        let mut executed = false;
        let mapped: CostResult<usize, ()> = initial.flat_map_ok(|_| {
            executed = true;
            CostContext {
                value: Err(()),
                cost: OperationCost {
                    loaded_bytes: 7,
                    ..Default::default()
                },
            }
        });

        // Second function should be executed and costs should increase. Result is error
        // though.
        assert!(executed);
        assert_eq!(
            mapped,
            CostContext {
                value: Err(()),
                cost: OperationCost {
                    loaded_bytes: 10,
                    ..Default::default()
                },
            }
        );
    }

    #[test]
    fn test_flatten_nested_errors() {
        let initial: CostResult<usize, &str> = CostContext {
            value: Ok(75),
            cost: OperationCost {
                loaded_bytes: 3,
                ..Default::default()
            },
        };
        // We use function that has nothing to do with costs but returns a result, we're
        // trying to flatten nested errors inside CostContext.
        let ok = initial.map_ok(|x| Ok(x + 25));
        assert_eq!(ok.flatten().unwrap(), Ok(100));

        let initial: CostResult<usize, &str> = CostContext {
            value: Ok(75),
            cost: OperationCost {
                loaded_bytes: 3,
                ..Default::default()
            },
        };
        let error_inner: CostResult<Result<usize, &str>, &str> = initial.map_ok(|_| Err("latter"));
        assert_eq!(error_inner.flatten().unwrap(), Err("latter"));

        let initial: CostResult<usize, &str> = CostContext {
            value: Err("inner"),
            cost: OperationCost {
                loaded_bytes: 3,
                ..Default::default()
            },
        };
        let error_inner: CostResult<Result<usize, &str>, &str> = initial.map_ok(|x| Ok(x + 25));
        assert_eq!(error_inner.flatten().unwrap(), Err("inner"));
    }

    #[test]
    fn test_wrap_fn_cost() {
        // Imagine this one is loaded from storage.
        let loaded_value = b"ayylmao";
        let costs_ctx = loaded_value.wrap_fn_cost(|x| OperationCost {
            seek_count: 1,
            loaded_bytes: x.len() as u32,
            ..Default::default()
        });
        assert_eq!(
            costs_ctx,
            CostContext {
                value: loaded_value,
                cost: OperationCost {
                    seek_count: 1,
                    loaded_bytes: 7,
                    ..Default::default()
                }
            }
        )
    }

    #[test]
    fn test_map_err() {
        let initial: CostResult<usize, ()> = CostContext {
            value: Err(()),
            cost: OperationCost {
                loaded_bytes: 3,
                ..Default::default()
            },
        };

        let mapped = initial.map_err(|_| "ayyerror");
        assert_eq!(
            mapped,
            CostContext {
                value: Err("ayyerror"),
                cost: OperationCost {
                    loaded_bytes: 3,
                    ..Default::default()
                },
            }
        );
    }
}
