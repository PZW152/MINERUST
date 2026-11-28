//! `GameRules`: the world's rules by name, as strings, read and written
//! under `Data.GameRules` in `level.dat`.
//!
//! The rules live with the server thread that runs the `Sim` (every
//! dimension shares the Overworld's, as `DerivedWorldInfo` does), so code
//! deep in the world and entity updates reads them without a reference
//! passed down. `Sim::step` installs the loaded rules on its thread first.

use std::cell::RefCell;
use std::collections::BTreeMap;

/// `GameRules()`: the rules and their defaults (`TreeMap`, sorted by name).
pub const DEFAULTS: [(&str, &str); 9] = [
    ("doFireTick", "true"),
    ("mobGriefing", "true"),
    ("keepInventory", "false"),
    ("doMobSpawning", "true"),
    ("doMobLoot", "true"),
    ("doTileDrops", "true"),
    ("commandBlockOutput", "true"),
    ("naturalRegeneration", "true"),
    ("doDaylightCycle", "true"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct GameRules(pub BTreeMap<String, String>);

impl Default for GameRules {
    fn default() -> Self {
        GameRules(
            DEFAULTS
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }
}

impl GameRules {
    /// `readGameRulesFromNBT`: every key of the compound, known or not
    /// (`setOrCreateGameRule`).
    pub fn from_nbt(t: Option<&crate::world::anvil::Nbt>) -> GameRules {
        let mut g = GameRules::default();
        if let Some(crate::world::anvil::Nbt::Compound(v)) = t {
            for (k, val) in v {
                if let crate::world::anvil::Nbt::String(s) = val {
                    g.0.insert(k.clone(), s.clone());
                }
            }
        }
        g
    }

    /// `writeGameRulesToNBT`
    pub fn to_nbt(&self) -> crate::world::anvil::Nbt {
        use crate::world::anvil::Nbt;
        Nbt::Compound(
            self.0
                .iter()
                .map(|(k, v)| (k.clone(), Nbt::String(v.clone())))
                .collect(),
        )
    }
}

thread_local! {
    static RULES: RefCell<GameRules> = RefCell::new(GameRules::default());
}

/// Makes `g` the rules of this thread's world.
pub fn install(g: GameRules) {
    RULES.with(|r| *r.borrow_mut() = g);
}

/// The rules as they are now (for saving).
pub fn current() -> GameRules {
    RULES.with(|r| r.borrow().clone())
}

/// `getGameRuleBooleanValue`: `Boolean.parseBoolean`, false for a rule
/// that does not exist.
pub fn bool(name: &str) -> bool {
    RULES.with(|r| {
        r.borrow()
            .0
            .get(name)
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
    })
}

/// `getGameRuleStringValue`: "" for a rule that does not exist.
pub fn string(name: &str) -> String {
    RULES.with(|r| r.borrow().0.get(name).cloned().unwrap_or_default())
}

/// `hasRule`
pub fn has(name: &str) -> bool {
    RULES.with(|r| r.borrow().0.contains_key(name))
}

/// `setOrCreateGameRule`
pub fn set(name: &str, value: &str) {
    RULES.with(|r| {
        r.borrow_mut().0.insert(name.to_string(), value.to_string());
    });
}

/// `getRules`, in key order.
pub fn names() -> Vec<String> {
    RULES.with(|r| r.borrow().0.keys().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_parse_as_the_game_does() {
        install(GameRules::default());
        assert!(bool("doDaylightCycle"));
        assert!(!bool("keepInventory"));
        set("keepInventory", "TRUE");
        assert!(bool("keepInventory"), "Boolean.parseBoolean ignores case");
        set("keepInventory", "yes");
        assert!(!bool("keepInventory"));
        assert!(!bool("noSuchRule"));
        assert_eq!(names()[0], "commandBlockOutput", "TreeMap order");
        let g = GameRules::from_nbt(Some(&current().to_nbt()));
        assert_eq!(g, current());
    }
}
