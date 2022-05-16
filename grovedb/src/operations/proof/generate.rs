use std::io::Write;

use merk::{
    proofs::{encode_into, Node},
    Merk, proofs::Op,
};
use storage::{rocksdb_storage::PrefixedRocksDbStorageContext, Storage, StorageContext};

use crate::{subtree::raw_decode, Element, Error, GroveDb, PathQuery, Query};
use crate::operations::proof::util::ProofType;

const EMPTY_TREE_HASH: [u8; 32] = [0; 32];

impl GroveDb {
    pub fn prove(&self, query: PathQuery) -> Result<Vec<u8>, Error> {
        // TODO: should it be possible to generate proofs for tree items (currently yes)
        let mut proof_result: Vec<u8> = vec![];
        let mut current_limit: Option<u16> = query.query.limit;
        let mut current_offset: Option<u16> = query.query.offset;

        let path_slices = query.path.iter().map(|x| x.as_slice()).collect::<Vec<_>>();
        if path_slices.len() < 1 {
            return Err(Error::InvalidPath("can't generate proof for empty path"));
        }
        self.check_subtree_exists_path_not_found(path_slices.clone(), None, None)?;

        self.prove_subqueries(
            &mut proof_result,
            path_slices.clone(),
            query.clone(),
            &mut current_limit,
            &mut current_offset,
        )?;
        self.prove_path(&mut proof_result, path_slices)?;

        Ok(proof_result)
    }

    fn prove_subqueries(
        &self,
        proofs: &mut Vec<u8>,
        path: Vec<&[u8]>,
        query: PathQuery,
        current_limit: &mut Option<u16>,
        current_offset: &mut Option<u16>,
    ) -> Result<(), Error> {
        // there is a chance that the subquery key would lead to something that is not a
        // tree same thing for the subquery itself
        let reached_limit = query.query.limit.is_some() && query.query.limit.unwrap() == 0;
        if reached_limit {
            return Ok(());
        }

        let subtree = self.open_subtree(&path)?;
        let mut is_leaf_tree = true;

        // iterate over each child of the current node
        // TODO: you should only care about kv pairs that are part of the query
        // if there is no subquery for that key then move on to the next
        // if there is we need to recurse with that as the new node

        // TODO: shouldn't you get the kv pairs based on the query??
        for (key, value_bytes) in subtree.get_kv_pairs(query.query.query.left_to_right).iter() {
            let (subquery_key, subquery_value) =
                Element::subquery_paths_for_sized_query(&query.query, key);

            if subquery_key.is_none() && subquery_value.is_none() {
                continue;
            }

            let element = raw_decode(value_bytes)?;
            match element {
                Element::Tree(tree_hash) => {
                    if tree_hash == EMPTY_TREE_HASH {
                        continue;
                    }

                    // if the element is a non empty tree then current tree is not a leaf tree
                    if is_leaf_tree {
                        is_leaf_tree = false;
                        // TODO: this should not be an all key query??
                        let mut all_key_query =
                            Query::new_with_direction(query.query.query.left_to_right);
                        all_key_query.insert_all();

                        self.generate_and_store_merk_proof(
                            &subtree,
                            all_key_query,
                            None,
                            None,
                            ProofType::MerkProof,
                            proofs,
                        )?;
                    }

                    let mut new_path = path.clone();
                    new_path.push(key.as_ref());

                    let mut query = subquery_value.clone();
                    let sub_key = subquery_key.clone();

                    if query.is_some() {
                        if subquery_key.is_some() {
                            // intermediate step here, generate a proof that show
                            // the existence or absence of the subquery key
                            let inner_subtree = self.open_subtree(&new_path)?;

                            let mut key_as_query = Query::new();
                            key_as_query.insert_key(sub_key.clone().unwrap());

                            self.generate_and_store_merk_proof(
                                &inner_subtree,
                                key_as_query,
                                None,
                                None,
                                ProofType::MerkProof,
                                proofs,
                            )?;

                            new_path.push(sub_key.as_ref().unwrap());
                        }
                    } else {
                        let mut key_as_query = Query::new();
                        key_as_query.insert_key(sub_key.unwrap());
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

                    // if we hit the limit, we should kill the loop
                    if current_limit.is_some() && current_limit.unwrap() == 0 {
                        break;
                    }
                }
                _ => {
                    // Current implementation makes the assumption that all elements of
                    // a result set are of the same type i.e either all trees, all items
                    // e.t.c and not mixed types.
                    // This ensures that invariant is preserved
                    debug_assert!(is_leaf_tree == false);
                }
            }
        }

        // TODO: Explore the chance that a subquery key might lead to non tree element
        if is_leaf_tree {
            // if no useful subtree, then we care about the result set of this subtree.
            // apply the sized query
            let limit_offset = self.generate_and_store_merk_proof(
                &subtree,
                query.query.query,
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

    fn open_subtree(
        &self,
        path: &Vec<&[u8]>,
    ) -> Result<Merk<PrefixedRocksDbStorageContext>, Error> {
        let storage = self.db.get_storage_context(path.clone());
        let subtree = Merk::open(storage)
            .map_err(|_| Error::CorruptedData("cannot open a subtree".to_owned()))?;
        Ok(subtree)
    }

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
                    query,
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

    fn generate_and_store_merk_proof<'a, S: 'a>(
        &self,
        subtree: &'a Merk<S>,
        query: Query,
        limit: Option<u16>,
        offset: Option<u16>,
        proof_type: ProofType,
        proofs: &mut Vec<u8>,
    ) -> Result<(Option<u16>, Option<u16>), Error>
    where
        S: StorageContext<'a, 'a>,
    {
        // TODO: How do you handle mixed tree types?
        let mut proof_result = subtree
            .prove_without_encoding(query, limit, offset)
            .expect("should generate proof");

        // Perform reference substitution for kv nodes
        for op in proof_result.proof.iter_mut() {
            match op {
                Op::Push(node) | Op::PushInverted(node) => match node {
                    Node::KV(_, value) => {
                        let elem = Element::deserialize(value);
                        if let Ok(Element::Reference(reference_path)) = elem {
                            let referenced_elem = self.follow_reference(reference_path, None)?;
                            *value = referenced_elem.serialize().unwrap();
                        }
                    }
                    _ => continue,
                },
                _ => continue,
            }
        }

        let mut proof_bytes = Vec::with_capacity(128);
        encode_into(proof_result.proof.iter(), &mut proof_bytes);

        // explicitly preventing proof generation as verification would fail
        // also a good way to detect if the needs of the system get past this point
        if proof_bytes.len() >= usize::MAX {
            return Err(Error::InvalidProof("proof too large"));
        }

        let proof_len_bytes: [u8; 8] = proof_bytes.len().to_be_bytes();
        write_to_vec(proofs, &[proof_type.into()]);
        write_to_vec(proofs, &proof_len_bytes);
        write_to_vec(proofs, &proof_bytes);

        Ok((proof_result.limit, proof_result.offset))
    }
}

fn write_to_vec<W: Write>(dest: &mut W, value: &[u8]) {
    dest.write_all(value);
}