//! `ItemStack.stackTagCompound`: the NBT of a stack, interned so that
//! `Stack` stays `Copy`. Equal compounds (in any key order, as
//! `NBTTagCompound.equals` compares maps) share one id, so comparing ids is
//! `ItemStack.areItemStackTagsEqual`. Interned tags live for the whole run.

use crate::world::anvil::Nbt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// A stack's tag; `Tag::NONE` is `stackTagCompound == null`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Tag(pub u32);

struct Table {
    list: Vec<Arc<Nbt>>,
    index: HashMap<Vec<u8>, u32>,
}

fn table() -> &'static Mutex<Table> {
    static T: OnceLock<Mutex<Table>> = OnceLock::new();
    T.get_or_init(|| {
        Mutex::new(Table {
            // id 0 is "no tag"
            list: vec![Arc::new(Nbt::Compound(Vec::new()))],
            index: HashMap::new(),
        })
    })
}

/// Compound keys sorted at every level: one form per map.
fn canonical(n: &Nbt) -> Nbt {
    match n {
        Nbt::Compound(v) => {
            let mut v: Vec<(String, Nbt)> =
                v.iter().map(|(k, x)| (k.clone(), canonical(x))).collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            // a later duplicate key wins, as NBTTagCompound.setTag does
            v.dedup_by(|b, a| {
                if a.0 == b.0 {
                    std::mem::swap(a, b);
                    true
                } else {
                    false
                }
            });
            Nbt::Compound(v)
        }
        Nbt::List(t, v) => Nbt::List(*t, v.iter().map(canonical).collect()),
        o => o.clone(),
    }
}

impl Tag {
    pub const NONE: Tag = Tag(0);

    /// Interns a compound.
    pub fn of(n: &Nbt) -> Tag {
        let c = canonical(n);
        let key = crate::world::anvil::write_nbt(&c);
        let mut t = table().lock().unwrap();
        if let Some(&i) = t.index.get(&key) {
            return Tag(i);
        }
        let i = t.list.len() as u32;
        t.list.push(Arc::new(c));
        t.index.insert(key, i);
        Tag(i)
    }

    pub fn get(self) -> Option<Arc<Nbt>> {
        if self == Tag::NONE {
            return None;
        }
        table().lock().unwrap().list.get(self.0 as usize).cloned()
    }

    pub fn is_none(self) -> bool {
        self == Tag::NONE
    }
}

#[cfg(test)]
mod tests {
    use crate::item::Stack;
    use crate::world::anvil::Nbt;

    #[test]
    fn tags_compare_like_compounds() {
        let a = Stack::new(276, 0, 1)
            .with_name(Some("Blade"))
            .with_repair_cost(3);
        let b = Stack::new(276, 0, 1)
            .with_repair_cost(3)
            .with_name(Some("Blade"));
        assert_eq!(a, b);
        assert_ne!(a, Stack::new(276, 0, 1));
        assert_eq!(a.custom_name().as_deref(), Some("Blade"));
        assert_eq!(a.repair_cost(), 3);
        // clearing the name leaves only RepairCost
        let c = a.with_name(None);
        assert_eq!(c.custom_name(), None);
        assert_eq!(c.repair_cost(), 3);
        // nothing left: back to no tag at all
        let d = Stack::new(1, 0, 1).with_name(Some("x")).with_name(None);
        assert!(d.tag.is_none());
    }

    #[test]
    fn save_forms_round_trip() {
        let s = Stack::new(310, 7, 1)
            .with_enchantments(&[(0, 4), (34, 3)])
            .with_name(Some("Helm"));
        assert_eq!(Stack::from_code(&s.to_code()), Some(s));
        let n = crate::world::anvil::item_nbt(Some(2), &s);
        assert_eq!(crate::world::anvil::item_from_nbt(&n), Some(s));
        // the old packed enchantment number: Sharpness V
        let old = Stack::from_code(&format!("276:0:1:{}", 16 | 5 << 6)).unwrap();
        assert_eq!(old.enchantments(), vec![(16, 5)]);
        assert!(old.is_enchanted());
        // books keep theirs as StoredEnchantments
        let b = Stack::new(403, 0, 1).with_enchantments(&[(48, 2)]);
        assert!(!b.is_enchanted());
        assert!(matches!(
            b.nbt().unwrap().get("StoredEnchantments"),
            Some(Nbt::List(..))
        ));
    }
}
