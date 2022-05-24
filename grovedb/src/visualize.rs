use std::io::{Result, Write};

use storage::StorageContext;
use visualize::{Drawer, Visualize};

use crate::{subtree::Element, util::storage_context_optional_tx, GroveDb, TransactionArg};

impl Visualize for Element {
    fn visualize<W: Write>(&self, mut drawer: Drawer<W>) -> Result<Drawer<W>> {
        match self {
            Element::Item(value, _) => {
                drawer.write(b"item: ")?;
                drawer = value.visualize(drawer)?;
            }
            Element::Reference(_ref, _) => {
                drawer.write(b"ref")?;
                // drawer.write(b"ref: [path: ")?;
                // let mut path_iter = path.iter();
                // if let Some(first) = path_iter.next() {
                //     drawer = first.visualize(drawer)?;
                // }
                // for p in path_iter {
                //     drawer.write(b", ")?;
                //     drawer = p.visualize(drawer)?;
                // }
                // drawer.write(b"]")?;
            }
            Element::Tree(hash) => {
                drawer.write(b"tree: ")?;
                drawer = hash.visualize(drawer)?;
            }
        }
        Ok(drawer)
    }
}

impl GroveDb {
    fn draw_subtree<W: Write>(
        &self,
        mut drawer: Drawer<W>,
        path: Vec<Vec<u8>>,
        transaction: TransactionArg,
    ) -> Result<Drawer<W>> {
        drawer.down();

        storage_context_optional_tx!(
            self.db,
            path.iter().map(|x| x.as_slice()),
            transaction,
            storage,
            {
                let mut iter = Element::iterator(storage.raw_iter());
                while let Some((key, element)) = iter.next().expect("cannot get next element") {
                    drawer.write(b"\n[key: ")?;
                    drawer = key.visualize(drawer)?;
                    drawer.write(b" ")?;
                    match element {
                        Element::Tree(_) => {
                            drawer.write(b"tree:")?;
                            drawer.down();
                            let mut inner_path = path.clone();
                            inner_path.push(key);
                            drawer = self.draw_subtree(drawer, inner_path, transaction)?;
                            drawer.up();
                        }
                        other => {
                            drawer = other.visualize(drawer)?;
                        }
                    }
                }
            }
        );

        drawer.up();
        Ok(drawer)
    }

    fn draw_root_tree<W: Write>(
        &self,
        mut drawer: Drawer<W>,
        transaction: TransactionArg,
    ) -> Result<Drawer<W>> {
        drawer.down();
        let root_leaf_keys = self
            .get_root_leaf_keys(transaction)
            .expect("cannot get root leaf keys");
        let keys = root_leaf_keys.iter().fold(
            vec![Vec::new(); root_leaf_keys.len()],
            |mut acc, (key, idx)| {
                acc[*idx] = key.clone();
                acc
            },
        );

        for k in keys {
            drawer.write(b"\n")?;
            drawer = k.visualize(drawer)?;
            drawer.write(b" tree:")?;
            drawer = self.draw_subtree(drawer, vec![k], transaction)?
        }
        drawer.up();
        Ok(drawer)
    }

    fn visualize_start<W: Write>(
        &self,
        mut drawer: Drawer<W>,
        transaction: TransactionArg,
    ) -> Result<Drawer<W>> {
        drawer.write(b"root")?;
        drawer = self.draw_root_tree(drawer, transaction)?;
        drawer.flush()?;
        Ok(drawer)
    }
}

impl Visualize for GroveDb {
    fn visualize<W: Write>(&self, drawer: Drawer<W>) -> Result<Drawer<W>> {
        self.visualize_start(drawer, None)
    }
}

#[cfg(test)]
mod tests {
    use visualize::to_hex;

    use super::*;

    #[test]
    fn test_element_item_str() {
        let v = b"ayylmao".to_vec();
        let e = Element::new_item(v.clone());
        let element_hex = to_hex(&v);
        let mut result = Vec::new();
        let drawer = Drawer::new(&mut result);
        e.visualize(drawer).expect("visualize IO error");
        assert_eq!(
            format!(
                "item: [hex: {element_hex}, str: {}]",
                String::from_utf8_lossy(&v)
            ),
            String::from_utf8_lossy(result.as_ref())
        );
    }

    #[test]
    fn test_element_item_no_tr() {
        let v = vec![1, 3, 3, 7, 255];
        let e = Element::new_item(v.clone());
        let element_hex = to_hex(&v);
        let mut result = Vec::new();
        let drawer = Drawer::new(&mut result);
        e.visualize(drawer).expect("visualize IO error");
        assert_eq!(
            format!("item: [hex: {element_hex}]"),
            String::from_utf8_lossy(result.as_ref())
        );
    }

    #[test]
    #[ignore]
    fn test_visualize_reference() {
        let p1 = b"ayy".to_vec();
        let p2 = b"lmao".to_vec();
        let e = Element::new_reference(vec![p1.clone(), p2.clone()]);
        let mut result = Vec::new();
        let drawer = Drawer::new(&mut result);
        e.visualize(drawer).expect("visualize IO error");
        assert_eq!(
            format!(
                "ref: [path: [hex: {p1_hex}, str: {p1}], [hex: {p2_hex}, str: {p2}]]",
                p1 = String::from_utf8_lossy(&p1),
                p2 = String::from_utf8_lossy(&p2),
                p1_hex = to_hex(&p1),
                p2_hex = to_hex(&p2),
            ),
            String::from_utf8_lossy(result.as_ref())
        );
    }
}
