use merk::{
    proofs::{encode_into, Node, Op},
    KVIterator, Merk, ProofWithoutEncodingResult,
};
use storage::{rocksdb_storage::PrefixedRocksDbStorageContext, Storage, StorageContext};

use crate::{
    operations::proof::util::{write_to_vec, ProofType, EMPTY_TREE_HASH},
    subtree::raw_decode,
    Element, Error, GroveDb, PathQuery, Query,
};

impl GroveDb {
    pub fn prove(&self, query: PathQuery) -> Result<Vec<u8>, Error> {
        // TODO: should it be possible to generate proofs for tree items (currently yes)
        let mut proof_result: Vec<u8> = vec![];
        let mut limit: Option<u16> = query.query.limit;
        let mut offset: Option<u16> = query.query.offset;

        let path_slices = query.path.iter().map(|x| x.as_slice()).collect::<Vec<_>>();
        if path_slices.len() < 1 {
            return Err(Error::InvalidPath("can't generate proof for empty path"));
        }
        self.check_subtree_exists_path_not_found(path_slices.clone(), None, None)?;

        self.prove_subqueries(
            &mut proof_result,
            path_slices.clone(),
            query.clone(),
            &mut limit,
            &mut offset,
        )?;
        self.prove_path(&mut proof_result, path_slices)?;

        Ok(proof_result)
    }

    /// Perform a pre-order traversal of the tree based on the provided
    /// subqueries
    fn prove_subqueries(
        &self,
        proofs: &mut Vec<u8>,
        path: Vec<&[u8]>,
        query: PathQuery,
        current_limit: &mut Option<u16>,
        current_offset: &mut Option<u16>,
    ) -> Result<(), Error> {
        let reached_limit = query.query.limit.is_some() && query.query.limit.unwrap() == 0;
        if reached_limit {
            return Ok(());
        }

        let subtree = self.open_subtree(&path)?;
        let mut is_leaf_tree = true;

        let kv_iterator = KVIterator::new(subtree.storage.raw_iter(), &query.query.query);
        for (key, value_bytes) in kv_iterator {
            let (subquery_key, subquery_value) =
                Element::subquery_paths_for_sized_query(&query.query, &key);

            if subquery_value.is_none() && subquery_key.is_none() {
                continue;
            }

            let element = raw_decode(&value_bytes)?;
            match element {
                Element::Tree(tree_hash) => {
                    if tree_hash == EMPTY_TREE_HASH {
                        continue;
                    }

                    // if the element is a non empty tree then current tree is not a leaf tree
                    if is_leaf_tree {
                        is_leaf_tree = false;
                        self.generate_and_store_merk_proof(
                            &subtree,
                            &query.query.query,
                            None,
                            None,
                            ProofType::MerkProof,
                            proofs,
                        )?;
                    }

                    let mut new_path = path.clone();
                    new_path.push(key.as_ref());

                    let mut query = subquery_value;

                    if query.is_some() {
                        if subquery_key.is_some() {
                            // prove the subquery key first
                            let inner_subtree = self.open_subtree(&new_path)?;

                            let mut key_as_query = Query::new();
                            key_as_query.insert_key(subquery_key.clone().unwrap());

                            self.generate_and_store_merk_proof(
                                &inner_subtree,
                                &key_as_query,
                                None,
                                None,
                                ProofType::MerkProof,
                                proofs,
                            )?;

                            new_path.push(subquery_key.as_ref().unwrap());
                        }
                    } else {
                        let mut key_as_query = Query::new();
                        key_as_query.insert_key(subquery_key.unwrap());
                        query = Some(key_as_query);
                    }

                    let new_path_owned = new_path.iter().map(|x| x.to_vec()).collect();
                    let new_path_query = PathQuery::new_unsized(new_path_owned, query.unwrap());

                    if self
                        .check_subtree_exists_path_not_found(new_path.clone(), None, None)
                        .is_err()
                    {
                        continue;
                    }

                    self.prove_subqueries(
                        proofs,
                        new_path,
                        new_path_query,
                        current_limit,
                        current_offset,
                    )?;

                    if *current_limit == Some(0) {
                        break;
                    }
                }
                _ => {
                    // currently not handling trees with mixed types
                    // if a tree has been seen, we should see nothing but tree
                    if is_leaf_tree == false {
                        return Err(Error::InvalidQuery("mixed tree types"));
                    }
                }
            }
        }

        // TODO: Explore the chance that a subquery key might lead to non tree element
        if is_leaf_tree {
            // if no useful subtree, then we care about the result set of this subtree.
            // apply the sized query
            let limit_offset = self.generate_and_store_merk_proof(
                &subtree,
                &query.query.query,
                *current_limit,
                *current_offset,
                ProofType::SizedMerkProof,
                proofs,
            )?;

            // update limit and offset values
            *current_limit = limit_offset.0;
            *current_offset = limit_offset.1;
        }

        Ok(())
    }

    /// Given a path, construct and append a set of proofs that shows there is
    /// a valid path from the root of the db to that point.
    fn prove_path(
        &self,
        mut proof_result: &mut Vec<u8>,
        path_slices: Vec<&[u8]>,
    ) -> Result<(), Error> {
        // generate proof to show that the path leads up to the root
        let mut split_path = path_slices.split_last();
        while let Some((key, path_slice)) = split_path {
            if path_slice.is_empty() {
                // generate root proof
                let meta_storage = self.db.get_storage_context(std::iter::empty());
                let root_leaf_keys = Self::get_root_leaf_keys_internal(&meta_storage)?;
                let mut index_to_prove: Vec<usize> = vec![];
                match root_leaf_keys.get(&key.to_vec()) {
                    Some(index) => index_to_prove.push(*index),
                    None => return Err(Error::InvalidPath("invalid root key")),
                }
                let root_tree = self.get_root_tree(None).expect("should get root tree");
                let root_proof = root_tree.proof(&index_to_prove).to_bytes();

                // explicitly preventing proof generation as verification would fail
                // also a good way to detect if the needs of the system get past this point
                if root_proof.len() >= usize::MAX {
                    return Err(Error::InvalidProof("proof too large"));
                }
                write_to_vec(&mut proof_result, &vec![ProofType::RootProof.into()]);
                write_to_vec(&mut proof_result, &root_proof.len().to_be_bytes());
                write_to_vec(&mut proof_result, &root_proof);

                // write the number of root leafs
                // this makes the assumption that 1 byte is enough to represent the number of
                // root leafs i.e max of 255 root leaf keys
                debug_assert!(root_leaf_keys.len() < 256);
                write_to_vec(&mut proof_result, &[root_leaf_keys.len() as u8]);

                // add the index values required to prove the root
                let index_to_prove_as_bytes = index_to_prove
                    .into_iter()
                    .map(|index| index as u8)
                    .collect::<Vec<u8>>();

                write_to_vec(&mut proof_result, &index_to_prove_as_bytes);
            } else {
                // generate proofs for the intermediate paths
                let path_slices = path_slice.iter().map(|x| *x).collect::<Vec<_>>();

                let subtree = self.open_subtree(&path_slices)?;
                let mut query = Query::new();
                query.insert_key(key.to_vec());

                self.generate_and_store_merk_proof(
                    &subtree,
                    &query,
                    None,
                    None,
                    ProofType::MerkProof,
                    &mut proof_result,
                )?;
            }
            split_path = path_slice.split_last();
        }
        Ok(())
    }

    /// Generates query proof given a subtree and appends the result to a proof
    /// list
    fn generate_and_store_merk_proof<'a, S: 'a>(
        &self,
        subtree: &'a Merk<S>,
        query: &Query,
        limit: Option<u16>,
        offset: Option<u16>,
        proof_type: ProofType,
        proofs: &mut Vec<u8>,
    ) -> Result<(Option<u16>, Option<u16>), Error>
    where
        S: StorageContext<'a>,
    {
        // TODO: How do you handle mixed tree types?
        let mut proof_result = subtree
            .prove_without_encoding(query.clone(), limit, offset)
            .expect("should generate proof");

        self.replace_references(&mut proof_result);

        let mut proof_bytes = Vec::with_capacity(128);
        encode_into(proof_result.proof.iter(), &mut proof_bytes);

        if proof_bytes.len() >= usize::MAX {
            return Err(Error::InvalidProof("proof too large"));
        }

        let proof_len_bytes: [u8; 8] = proof_bytes.len().to_be_bytes();
        write_to_vec(proofs, &[proof_type.into()]);
        write_to_vec(proofs, &proof_len_bytes);
        write_to_vec(proofs, &proof_bytes);

        Ok((proof_result.limit, proof_result.offset))
    }

    /// Replaces references with the base item they point to
    fn replace_references(
        &self,
        proof_result: &mut ProofWithoutEncodingResult,
    ) -> Result<(), Error> {
        for op in proof_result.proof.iter_mut() {
            match op {
                Op::Push(node) | Op::PushInverted(node) => match node {
                    Node::KV(_, value) => {
                        let elem = Element::deserialize(value);
                        if let Ok(Element::Reference(reference_path, _)) = elem {
                            let referenced_elem = self.follow_reference(reference_path, None)?;
                            *value = referenced_elem.serialize().unwrap();
                        }
                    }
                    _ => continue,
                },
                _ => continue,
            }
        }
        Ok(())
    }

    /// Opens merk at a given path without transaction
    fn open_subtree(
        &self,
        path: &Vec<&[u8]>,
    ) -> Result<Merk<PrefixedRocksDbStorageContext>, Error> {
        let storage = self.db.get_storage_context(path.clone());
        let subtree = Merk::open(storage)
            .map_err(|_| Error::CorruptedData("cannot open a subtree".to_owned()))?;
        Ok(subtree)
    }
}
