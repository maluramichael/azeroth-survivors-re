//! Mods: classes, abilities and passives loaded from TOML files at start, next to the built-in
//! tables in [`super::data`] (ticket #1103, stage 1). `docs/MODDING.md` is the player's side.
//!
//! The tables in `data.rs` are `&'static` all the way down, so a mod's entries are parsed once
//! and leaked: they live as long as the process, and `ClassDef`, `AbilityDef` and `PassiveDef`
//! stay exactly what they were. `data::CLASSES`, `ABILITIES` and `PASSIVES` are the built-ins
//! followed by what [`mods`] holds. The loader runs from those statics' initialisers, so it
//! needs no Bevy startup order; [`plugin`] only forces it early so its log lines land at start.
//!
//! A bad mod never stops the game: a file that is not TOML is skipped whole, any other mistake
//! skips just its entry, and each skip is a `warn!` that says why.

use std::path::Path;
use std::sync::LazyLock;

use bevy::prelude::*;
use serde::Deserialize;

use super::data::{self, AbilityDef, ClassDef, Effect, Kind, PassiveDef, Stat};

/// What the mod files add, in file order (files sorted by name), after the built-ins.
#[derive(Default)]
pub(crate) struct Mods {
    pub abilities: Vec<AbilityDef>,
    pub passives: Vec<PassiveDef>,
    pub classes: Vec<ClassDef>,
}

static MODS: LazyLock<Mods> = LazyLock::new(|| load(&read_folder_files()));

/// The loaded mods; the first call reads the mod folder.
pub(crate) fn mods() -> &'static Mods {
    &MODS
}

pub(super) fn plugin(_app: &mut App) {
    // Read the folder now, with the log running, rather than on the first table lookup.
    LazyLock::force(&MODS);
}

/// The mod folder: `WOW_SURVIVORS_MODS` when set, else `mods` in the state folder. `None` on the
/// web (no folder to read) and without a state folder. Tests skip the state folder's `mods`, so a
/// player's own mods cannot change what they assert.
fn folder() -> Option<std::path::PathBuf> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    if let Some(dir) = benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_MODS") {
        return Some(dir.into());
    }
    if cfg!(test) {
        return None;
    }
    crate::state_home().map(|h| h.join("mods"))
}

fn read_folder_files() -> Vec<(String, String)> {
    let Some(dir) = folder() else {
        return Vec::new();
    };
    if !dir.is_dir() {
        // A default folder that is not there is the normal case; one that was asked for is not.
        if benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_MODS").is_some() {
            warn!("mods: {} is not a folder", dir.display());
        }
        return Vec::new();
    }
    read_folder(&dir)
}

/// Every `*.toml` directly in `dir` as `(file name, text)`, sorted by file name. A file that
/// cannot be read is skipped with a warning.
fn read_folder(dir: &Path) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.to_ascii_lowercase().ends_with(".toml"))
        .collect();
    names.sort();
    names
        .into_iter()
        .filter_map(|name| match std::fs::read_to_string(dir.join(&name)) {
            Ok(text) => Some((name, text)),
            Err(e) => {
                warn!("mods: cannot read {name}: {e}");
                None
            }
        })
        .collect()
}

// --- The file format ---

/// `[[ability]]`: the key, the spell and the kind are required, the rest takes the defaults the
/// built-in table's constructor gives.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AbilityToml {
    key: String,
    spell: u32,
    kind: Kind,
    damage: Option<f32>,
    dot: Option<f32>,
    cooldown: Option<f32>,
    range: Option<f32>,
    radius: Option<f32>,
    count: Option<u32>,
    duration: Option<f32>,
    effect: Option<Effect>,
    mark: Option<u32>,
    totem: Option<(u32, u32)>,
    overhead: Option<bool>,
}

/// `[[passive]]`: every field is required.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PassiveToml {
    key: String,
    name: String,
    spell: u32,
    stat: Stat,
    per_level: f32,
}

/// `[[class]]`: a built-in class lends its id, power, colour, looks, weapons and sheath.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassToml {
    name: String,
    base: String,
    start: String,
    pool: Vec<String>,
    /// 0xRRGGBB, over the base class's colour.
    color: Option<u32>,
}

/// One file's entries, each already deserialised on its own so one bad entry costs only itself.
struct Parsed {
    file: String,
    abilities: Vec<AbilityToml>,
    passives: Vec<PassiveToml>,
    classes: Vec<ClassToml>,
}

/// Takes `section` out of `table` and deserialises its entries one by one.
fn entries<T: for<'de> Deserialize<'de>>(
    file: &str,
    table: &mut toml::Table,
    section: &str,
) -> Vec<T> {
    let Some(value) = table.remove(section) else {
        return Vec::new();
    };
    let toml::Value::Array(items) = value else {
        warn!("mods: {file}: `{section}` must be a list of tables, written [[{section}]]");
        return Vec::new();
    };
    items
        .into_iter()
        .enumerate()
        .filter_map(|(i, item)| match item.try_into::<T>() {
            Ok(entry) => Some(entry),
            Err(e) => {
                warn!(
                    "mods: {file}: {section} #{} skipped: {}",
                    i + 1,
                    e.message()
                );
                None
            }
        })
        .collect()
}

fn parse(file: &str, text: &str) -> Option<Parsed> {
    let mut table: toml::Table = match text.parse() {
        Ok(table) => table,
        Err(e) => {
            warn!("mods: {file} skipped, not valid TOML: {}", e.message());
            return None;
        }
    };
    let parsed = Parsed {
        file: file.to_string(),
        abilities: entries(file, &mut table, "ability"),
        passives: entries(file, &mut table, "passive"),
        classes: entries(file, &mut table, "class"),
    };
    for left in table.keys() {
        warn!("mods: {file}: unknown section `{left}` ignored");
    }
    Some(parsed)
}

// --- Validation and building ---

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// The numbers an ability can hold without breaking the run: finite, never negative, and a
/// cooldown above zero (a zero cooldown would fire every frame).
fn check_ability(def: &AbilityDef) -> Result<(), String> {
    for (name, v) in [
        ("damage", def.damage),
        ("dot", def.dot),
        ("cooldown", def.cooldown),
        ("range", def.range),
        ("radius", def.radius),
        ("duration", def.duration),
    ] {
        if !v.is_finite() || v < 0.0 {
            return Err(format!("{name} must be a number of zero or more"));
        }
    }
    if def.cooldown <= 0.0 {
        return Err("cooldown must be above zero".into());
    }
    if def.count == 0 {
        return Err("count must be at least 1".into());
    }
    if def.spell == 0 {
        return Err("spell must be a Spell.dbc id".into());
    }
    Ok(())
}

/// Builds the mods from `(file name, text)` pairs, in the order given. Abilities and passives of
/// every file are taken before any class, so a class may use a spell from a later file.
pub(crate) fn load(files: &[(String, String)]) -> Mods {
    let parsed: Vec<Parsed> = files.iter().filter_map(|(f, t)| parse(f, t)).collect();
    let mut mods = Mods::default();
    // Ability and passive keys share one namespace: the hand and the banish list hold both.
    let mut taken: Vec<&'static str> = data::builtin_abilities()
        .iter()
        .map(|a| a.key)
        .chain(data::builtin_passives().iter().map(|p| p.key))
        .collect();
    let mut added: Vec<[usize; 3]> = vec![[0; 3]; parsed.len()];

    for (n, file) in parsed.iter().enumerate() {
        for a in &file.abilities {
            let name = &file.file;
            if a.key.is_empty() || taken.contains(&a.key.as_str()) {
                warn!(
                    "mods: {name}: ability {:?} skipped, the key is empty or already taken",
                    a.key
                );
                continue;
            }
            let key = leak(a.key.clone());
            let mut def = data::ab(key, a.spell, a.kind);
            def.damage = a.damage.unwrap_or(def.damage);
            def.dot = a.dot.unwrap_or(def.dot);
            def.cooldown = a.cooldown.unwrap_or(def.cooldown);
            def.range = a.range.unwrap_or(def.range);
            def.radius = a.radius.unwrap_or(def.radius);
            def.count = a.count.unwrap_or(def.count);
            def.duration = a.duration.unwrap_or(def.duration);
            def.effect = a.effect.unwrap_or(def.effect);
            def.mark = a.mark.unwrap_or(def.mark);
            def.totem = a.totem.unwrap_or(def.totem);
            def.overhead = a.overhead.unwrap_or(def.overhead);
            if let Err(why) = check_ability(&def) {
                warn!("mods: {name}: ability {key:?} skipped, {why}");
                continue;
            }
            taken.push(key);
            mods.abilities.push(def);
            added[n][0] += 1;
        }
        for p in &file.passives {
            let name = &file.file;
            if p.key.is_empty() || taken.contains(&p.key.as_str()) {
                warn!(
                    "mods: {name}: passive {:?} skipped, the key is empty or already taken",
                    p.key
                );
                continue;
            }
            if p.name.is_empty() || p.spell == 0 || !p.per_level.is_finite() || p.per_level < 0.0 {
                warn!("mods: {name}: passive {:?} skipped, needs a name, a spell and a per_level of zero or more", p.key);
                continue;
            }
            let key = leak(p.key.clone());
            taken.push(key);
            mods.passives.push(PassiveDef {
                key,
                name: leak(p.name.clone()),
                spell: p.spell,
                stat: p.stat,
                per_level: p.per_level,
            });
            added[n][1] += 1;
        }
    }

    let ability_key = |key: &str| -> Option<&'static str> {
        data::builtin_abilities()
            .iter()
            .chain(&mods.abilities)
            .find(|a| a.key == key)
            .map(|a| a.key)
    };
    let mut classes: Vec<ClassDef> = Vec::new();
    for (n, file) in parsed.iter().enumerate() {
        let name = &file.file;
        'class: for c in &file.classes {
            let taken_name = |classes: &[ClassDef]| {
                data::builtin_classes()
                    .iter()
                    .chain(classes)
                    .any(|k| k.name.eq_ignore_ascii_case(&c.name))
            };
            if c.name.trim().is_empty() || taken_name(&classes) {
                warn!(
                    "mods: {name}: class {:?} skipped, the name is empty or already taken",
                    c.name
                );
                continue;
            }
            let Some(base) = data::builtin_classes()
                .iter()
                .find(|k| k.name.eq_ignore_ascii_case(&c.base))
            else {
                warn!(
                    "mods: {name}: class {:?} skipped, base {:?} is not a built-in class",
                    c.name, c.base
                );
                continue;
            };
            let mut pool: Vec<&'static str> = Vec::new();
            for key in &c.pool {
                let Some(key) = ability_key(key) else {
                    warn!(
                        "mods: {name}: class {:?} skipped, pool key {key:?} is no ability",
                        c.name
                    );
                    continue 'class;
                };
                if pool.contains(&key) {
                    warn!(
                        "mods: {name}: class {:?} skipped, pool lists {key:?} twice",
                        c.name
                    );
                    continue 'class;
                }
                pool.push(key);
            }
            let Some(start) = ability_key(&c.start).filter(|k| pool.contains(k)) else {
                warn!(
                    "mods: {name}: class {:?} skipped, start {:?} is not in its pool",
                    c.name, c.start
                );
                continue;
            };
            classes.push(ClassDef {
                name: leak(c.name.clone()),
                color: c.color.unwrap_or(base.color),
                start,
                pool: Box::leak(pool.into_boxed_slice()),
                ..*base
            });
            added[n][2] += 1;
        }
    }
    mods.classes = classes;

    for (file, [a, p, c]) in parsed.iter().zip(&added) {
        info!(
            "mods: {} added {a} abilit{}, {p} passive{}, {c} class{}",
            file.file,
            if *a == 1 { "y" } else { "ies" },
            if *p == 1 { "" } else { "s" },
            if *c == 1 { "" } else { "es" },
        );
    }
    mods
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../../docs/examples/mods/spellblade.toml");

    fn one(text: &str) -> Mods {
        load(&[("test.toml".to_string(), text.to_string())])
    }

    fn pool_of<'a>(mods: &'a Mods, class: &str) -> &'a [&'static str] {
        mods.classes.iter().find(|c| c.name == class).unwrap().pool
    }

    #[test]
    fn the_example_mod_makes_a_class_that_mixes_warrior_and_mage() {
        let mods = one(EXAMPLE);
        assert_eq!(mods.abilities.len(), 1);
        let blade = &mods.abilities[0];
        assert_eq!(blade.key, "arcane_blade");
        assert_eq!(blade.kind, Kind::Nova);
        assert_eq!(blade.effect, Effect::Slow(0.5, 2.0));
        assert_eq!(mods.classes.len(), 1);
        let class = &mods.classes[0];
        let warrior = &data::builtin_classes()[0];
        assert_eq!(warrior.name, "Warrior");
        assert_eq!(class.name, "Spellblade");
        assert_eq!(
            (class.id, class.power, class.sheath),
            (warrior.id, warrior.power, warrior.sheath)
        );
        assert!(class.pool.iter().any(|k| warrior.pool.contains(k)));
        let mage = data::builtin_classes()
            .iter()
            .find(|c| c.name == "Mage")
            .unwrap();
        assert!(class.pool.iter().any(|k| mage.pool.contains(k)));
        assert!(class.pool.contains(&"arcane_blade"));
        assert!(class.pool.contains(&class.start));
    }

    #[test]
    fn bad_entries_are_skipped_and_the_rest_is_kept() {
        let mods = one(r#"
            [[ability]]
            key = "good"
            spell = 133
            kind = "Bolt"
            damage = 5          # an integer is a fine number
            [[ability]]
            key = "bad_kind"
            spell = 133
            kind = "Laser"
            [[ability]]
            key = "frostbolt"   # a built-in key
            spell = 116
            kind = "Bolt"
            [[ability]]
            key = "good"        # twice
            spell = 133
            kind = "Bolt"
            [[ability]]
            key = "no_cooldown"
            spell = 133
            kind = "Bolt"
            cooldown = 0.0
            [[ability]]
            key = "typo"
            spell = 133
            kind = "Bolt"
            dammage = 3.0
            [[passive]]
            key = "good_passive"
            name = "Good"
            spell = 1459
            stat = "Crit"
            per_level = 0.02
            [[passive]]
            key = "good"        # shares the ability namespace
            name = "Clash"
            spell = 1459
            stat = "Crit"
            per_level = 0.02
            [[class]]
            name = "Fine"
            base = "warrior"
            start = "good"
            pool = ["cleave", "good", "fireball"]
            color = 0x112233
            [[class]]
            name = "NoBase"
            base = "Necromancer"
            start = "cleave"
            pool = ["cleave"]
            [[class]]
            name = "NoKey"
            base = "Mage"
            start = "cleave"
            pool = ["cleave", "does_not_exist"]
            [[class]]
            name = "NoStart"
            base = "Mage"
            start = "fireball"
            pool = ["cleave"]
            [[class]]
            name = "mage"       # a built-in name
            base = "Mage"
            start = "cleave"
            pool = ["cleave"]
            [[class]]
            name = "Fine"       # twice
            base = "Mage"
            start = "cleave"
            pool = ["cleave"]
            [[class]]
            name = "Twice"
            base = "Mage"
            start = "cleave"
            pool = ["cleave", "cleave"]
            [[class]]
            name = "NoName"
            "#);
        let abilities: Vec<_> = mods.abilities.iter().map(|a| a.key).collect();
        assert_eq!(abilities, ["good"]);
        assert_eq!(mods.abilities[0].damage, 5.0);
        let passives: Vec<_> = mods.passives.iter().map(|p| p.key).collect();
        assert_eq!(passives, ["good_passive"]);
        let classes: Vec<_> = mods.classes.iter().map(|c| c.name).collect();
        assert_eq!(classes, ["Fine"]);
        assert_eq!(pool_of(&mods, "Fine"), ["cleave", "good", "fireball"]);
        assert_eq!(mods.classes[0].color, 0x112233);
    }

    #[test]
    fn a_file_that_is_not_toml_is_skipped_and_the_others_still_load() {
        let mods = load(&[
            ("a.toml".to_string(), "this is = = not toml".to_string()),
            ("b.toml".to_string(), EXAMPLE.to_string()),
            (
                "c.toml".to_string(),
                "[[ability]]\nkey = \"later\"\nspell = 133\nkind = \"Nova\"".to_string(),
            ),
        ]);
        assert_eq!(mods.classes.len(), 1);
        assert_eq!(mods.abilities.len(), 2);
    }

    #[test]
    fn a_class_may_use_an_ability_from_a_later_file() {
        let mods = load(&[
            (
                "a.toml".to_string(),
                "[[class]]\nname = \"Early\"\nbase = \"Rogue\"\nstart = \"late\"\npool = [\"late\", \"gouge\"]".to_string(),
            ),
            (
                "b.toml".to_string(),
                "[[ability]]\nkey = \"late\"\nspell = 133\nkind = \"Bolt\"".to_string(),
            ),
        ]);
        assert_eq!(pool_of(&mods, "Early"), ["late", "gouge"]);
    }

    #[test]
    fn omitted_fields_take_the_builtin_defaults() {
        let mods = one("[[ability]]\nkey = \"plain\"\nspell = 133\nkind = \"Nova\"");
        let plain = &mods.abilities[0];
        let base = data::ab("plain", 133, Kind::Nova);
        assert_eq!(
            (
                plain.cooldown,
                plain.range,
                plain.count,
                plain.effect,
                plain.totem
            ),
            (
                base.cooldown,
                base.range,
                base.count,
                base.effect,
                base.totem
            )
        );
    }

    #[test]
    fn every_effect_and_a_totem_parse() {
        let mods = one(r#"
            [[ability]]
            key = "e1"
            spell = 133
            kind = "Totem"
            totem = [4589, 8227]
            effect = { Root = 2 }
            [[ability]]
            key = "e2"
            spell = 133
            kind = "Strike"
            effect = "None"
            [[ability]]
            key = "e3"
            spell = 133
            kind = "Strike"
            effect = { Execute = 0.2 }
            "#);
        assert_eq!(mods.abilities.len(), 3);
        assert_eq!(mods.abilities[0].totem, (4589, 8227));
        assert_eq!(mods.abilities[0].effect, Effect::Root(2.0));
        assert_eq!(mods.abilities[2].effect, Effect::Execute(0.2));
    }

    #[test]
    fn no_files_add_nothing_and_the_builtin_tables_keep_their_size() {
        let mods = load(&[]);
        assert!(mods.abilities.is_empty() && mods.passives.is_empty() && mods.classes.is_empty());
        // Tests never read the state folder, so the live tables are the built-ins, unless the
        // whole run was pointed at a mod folder on purpose.
        if benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_MODS").is_some() {
            return;
        }
        assert_eq!(data::CLASSES.len(), data::builtin_classes().len());
        assert_eq!(data::ABILITIES.len(), data::builtin_abilities().len());
        assert_eq!(data::PASSIVES.len(), data::builtin_passives().len());
    }

    #[test]
    fn a_missing_or_empty_folder_reads_as_no_files() {
        let dir = std::env::temp_dir().join(format!("survivors-mods-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(read_folder(&dir).is_empty());
        std::fs::create_dir_all(&dir).unwrap();
        assert!(read_folder(&dir).is_empty());
        std::fs::write(dir.join("b.toml"), "").unwrap();
        std::fs::write(dir.join("A.TOML"), "").unwrap();
        std::fs::write(dir.join("notes.txt"), "").unwrap();
        let names: Vec<_> = read_folder(&dir).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["A.TOML", "b.toml"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
