use anyhow::{anyhow, Error};
use costs::{CostContext, CostsExt, OperationCost};
use ed::{Decode, Encode};
use storage::StorageContext;

use super::Tree;

impl Tree {
    pub fn decode_raw(bytes: &[u8]) -> Result<Self, Error> {
        Decode::decode(bytes).map_err(|e| anyhow!("failed to decode a Tree structure ({})", e))
    }

    pub(crate) fn get<'db, S, K>(storage: &S, key: K) -> CostContext<Result<Option<Self>, Error>>
    where
        S: StorageContext<'db>,
        K: AsRef<[u8]>,
        Error: From<S::Error>,
    {
        let mut cost = OperationCost {
            seek_count: 1,
            ..Default::default()
        };
        let tree_bytes: Result<_, Error> = storage.get(&key).map_err(|e| e.into());
        if let Ok(Some(bytes)) = &tree_bytes {
            cost.loaded_bytes = bytes.len() as u32;
        }
        let tree = tree_bytes.and_then(|raw_opt| raw_opt.map(|x| Tree::decode_raw(&x)).transpose());

        let res = match tree {
            Ok(Some(mut t)) => {
                t.set_key(key.as_ref().to_vec());
                Ok(Some(t))
            }
            other => other,
        };

        res.wrap_with_cost(cost)
    }
}

impl Tree {
    #[inline]
    pub fn encode(&self) -> Vec<u8> {
        // operation is infallible so it's ok to unwrap
        Encode::encode(self).unwrap()
    }

    #[inline]
    pub fn encode_into(&self, dest: &mut Vec<u8>) {
        // operation is infallible so it's ok to unwrap
        Encode::encode_into(self, dest).unwrap()
    }

    #[inline]
    pub fn encoding_length(&self) -> usize {
        // operation is infallible so it's ok to unwrap
        Encode::encoding_length(self).unwrap()
    }

    #[inline]
    pub fn decode_into(&mut self, key: Vec<u8>, input: &[u8]) {
        // operation is infallible so it's ok to unwrap
        Decode::decode_into(self, input).unwrap();
        self.inner.kv.key = key;
    }

    #[inline]
    pub fn decode(key: Vec<u8>, input: &[u8]) -> Self {
        // operation is infallible so it's ok to unwrap
        // TODO: how said that its infallible?
        let mut tree: Self = Decode::decode(input).unwrap();
        tree.inner.kv.key = key;
        tree
    }
}

#[cfg(test)]
mod tests {
    use super::{super::Link, *};

    #[test]
    fn encode_leaf_tree() {
        let tree = Tree::from_fields(vec![0], vec![1], [55; 32], None, None).unwrap();
        assert_eq!(tree.encoding_length(), 67);
        assert_eq!(
            tree.encode(),
            vec![
                0, 0, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55,
                55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 32, 34, 236, 157, 87, 27, 167,
                116, 207, 158, 131, 208, 25, 73, 98, 245, 209, 227, 170, 26, 72, 212, 134, 166,
                126, 39, 98, 166, 199, 149, 144, 21, 1,
            ]
        );
    }

    #[test]
    #[should_panic]
    fn encode_modified_tree() {
        let tree = Tree::from_fields(
            vec![0],
            vec![1],
            [55; 32],
            Some(Link::Modified {
                pending_writes: 1,
                child_heights: (123, 124),
                tree: Tree::new(vec![2], vec![3]).unwrap(),
            }),
            None,
        )
        .unwrap();
        tree.encode();
    }

    #[test]
    fn encode_loaded_tree() {
        let tree = Tree::from_fields(
            vec![0],
            vec![1],
            [55; 32],
            Some(Link::Loaded {
                hash: [66; 32],
                child_heights: (123, 124),
                tree: Tree::new(vec![2], vec![3]).unwrap(),
            }),
            None,
        )
        .unwrap();
        assert_eq!(
            tree.encode(),
            vec![
                1, 1, 2, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66,
                66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 123, 124, 0, 55, 55, 55,
                55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55,
                55, 55, 55, 55, 55, 55, 55, 55, 32, 34, 236, 157, 87, 27, 167, 116, 207, 158, 131,
                208, 25, 73, 98, 245, 209, 227, 170, 26, 72, 212, 134, 166, 126, 39, 98, 166, 199,
                149, 144, 21, 1
            ]
        );
    }

    #[test]
    fn encode_uncommitted_tree() {
        let tree = Tree::from_fields(
            vec![0],
            vec![1],
            [55; 32],
            Some(Link::Uncommitted {
                hash: [66; 32],
                child_heights: (123, 124),
                tree: Tree::new(vec![2], vec![3]).unwrap(),
            }),
            None,
        )
        .unwrap();
        assert_eq!(
            tree.encode(),
            vec![
                1, 1, 2, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66,
                66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 123, 124, 0, 55, 55, 55,
                55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55,
                55, 55, 55, 55, 55, 55, 55, 55, 32, 34, 236, 157, 87, 27, 167, 116, 207, 158, 131,
                208, 25, 73, 98, 245, 209, 227, 170, 26, 72, 212, 134, 166, 126, 39, 98, 166, 199,
                149, 144, 21, 1
            ]
        );
    }

    #[test]
    fn encode_reference_tree() {
        let tree = Tree::from_fields(
            vec![0],
            vec![1],
            [55; 32],
            Some(Link::Reference {
                hash: [66; 32],
                child_heights: (123, 124),
                key: vec![2],
            }),
            None,
        )
        .unwrap();
        assert_eq!(tree.encoding_length(), 103);
        assert_eq!(
            tree.encode(),
            vec![
                1, 1, 2, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66,
                66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 123, 124, 0, 55, 55, 55,
                55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55,
                55, 55, 55, 55, 55, 55, 55, 55, 32, 34, 236, 157, 87, 27, 167, 116, 207, 158, 131,
                208, 25, 73, 98, 245, 209, 227, 170, 26, 72, 212, 134, 166, 126, 39, 98, 166, 199,
                149, 144, 21, 1
            ]
        );
    }

    #[test]
    fn decode_leaf_tree() {
        let bytes = vec![
            0, 0, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 32, 34, 236, 157, 87, 27, 167, 116, 207, 158, 131,
            208, 25, 73, 98, 245, 209, 227, 170, 26, 72, 212, 134, 166, 126, 39, 98, 166, 199, 149,
            144, 21, 1,
        ];
        let tree = Tree::decode(vec![0], bytes.as_slice());
        assert_eq!(tree.key(), &[0]);
        assert_eq!(tree.value(), &[1]);
    }

    #[test]
    fn decode_reference_tree() {
        let bytes = vec![
            1, 1, 2, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66,
            66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 66, 123, 124, 0, 55, 55, 55, 55, 55,
            55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55, 55,
            55, 55, 55, 55, 55, 32, 34, 236, 157, 87, 27, 167, 116, 207, 158, 131, 208, 25, 73, 98,
            245, 209, 227, 170, 26, 72, 212, 134, 166, 126, 39, 98, 166, 199, 149, 144, 21, 1,
        ];
        let tree = Tree::decode(vec![0], bytes.as_slice());
        assert_eq!(tree.key(), &[0]);
        assert_eq!(tree.value(), &[1]);
        if let Some(Link::Reference {
            key,
            child_heights,
            hash,
        }) = tree.link(true)
        {
            assert_eq!(*key, [2]);
            assert_eq!(*child_heights, (123u8, 124u8));
            assert_eq!(*hash, [66u8; 32]);
        } else {
            panic!("Expected Link::Reference");
        }
    }
}
