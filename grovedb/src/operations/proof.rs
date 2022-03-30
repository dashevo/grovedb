use std::env::split_paths;

use crate::{
    util::{merk_optional_tx, meta_storage_context_optional_tx},
    Element, Error,
    Error::InvalidPath,
    GroveDb, PathQuery, Query,
};

impl GroveDb {
    pub fn prove(&self, query: PathQuery) -> Result<Vec<u8>, Error> {
        // A path query has a path and then a query
        // First we find the merk at the defined path
        // if there is no merk found at that path, then we return an error
        // if there is then we construct a proof on the merk with the query
        // then subsequently construct proofs for all parents up to the
        // root tree.
        // As we do this we aggregate the proofs in a reproducible structure

        // 1. Get the merk at the path defined by the query
        let path_slices = query.path.iter().map(|x| x.as_slice()).collect::<Vec<_>>();

        // checks if the subtree exists
        self.check_subtree_exists_path_not_found(path_slices.clone(), None, None)?;

        merk_optional_tx!(self.db, path_slices.clone(), None, subtree, {
            // TODO: Not allowed to create proof for an empty tree (handle this)
            let proof = subtree.prove(query.query.query, None, None);
            dbg!(proof);
        });

        // Generate proof up to root
        let mut split_path = path_slices.split_last();
        while let Some((key, path_slice)) = split_path {
            if path_slice.is_empty() {
                dbg!("gotten to root");
                // generate the root proof
                // rs-merkle stores the root keys as indexes
                // grovedb has a way to convert from readable names to those indexes
                // the goal here is to take the key value and convert it to the correct index
                // insert it into a vector, then use the vector to generate a root proof
                meta_storage_context_optional_tx!(self.db, None, meta_storage, {
                    // TODO: is this correct
                    // if we cannot get the root_left_keys then something is wrong should propagate
                    let root_leaf_keys = Self::get_root_leaf_keys_internal(&meta_storage)?;
                    let mut root_index: Vec<usize> = vec![];
                    match root_leaf_keys.get(&key.to_vec()) {
                        Some(index) => root_index.push(*index),
                        // technically, this should not be possible as the path should
                        // have caught this already
                        None => return Err(InvalidPath("invalid root key")),
                    }
                    let root_tree = self.get_root_tree(None).expect("should get root tree");
                    let root_proof = root_tree.proof(&root_index).to_bytes();
                    dbg!(root_proof);
                })
            } else {
                let path_slices = path_slice.iter().map(|x| *x).collect::<Vec<_>>();

                merk_optional_tx!(self.db, path_slices, None, subtree, {
                    // TODO: Not allowed to create proof for an empty tree (handle this)
                    let mut query = Query::new();
                    query.insert_key(key.to_vec());

                    let proof = subtree.prove(query, None, None);
                    dbg!(proof);
                });
            }
            split_path = path_slice.split_last();
        }

        Err(Error::InvalidQuery("invalid query"))
    }

    pub fn execute_proof(proof: Vec<u8>) -> Result<([u8; 32], Vec<(Vec<u8>, Vec<u8>)>), Error> {
        Err(Error::InvalidProof("proof invalid"))
    }
}
