# Modding

Warcraft Survivors can load extra classes, abilities and passives from TOML files at start. They
sit next to the built-in tables; nothing built in is changed or removed. A class combination is
a new class whose ability pool mixes abilities of two classes. This is stage 1: data only, no
scripting.

## Where the files go

The mod folder is, in this order:

1. the folder given with `--mods <DIR>` or the environment variable `WOW_SURVIVORS_MODS`,
2. otherwise `mods` inside the state folder (`benilla-config`, or whatever `--home` / `BENILLA_HOME`
   points at).

Every `*.toml` file directly in that folder is read, sorted by file name. Subfolders are not
searched. The folder is read once at start, so restart the game after a change. On the web build
there is no folder: only the built-in tables exist.

The log has one `mods:` line per file with what it added, and a warning for every entry or file
it skipped, with the reason. A mistake in a mod never stops the game: a file that is not valid
TOML is skipped whole, any other mistake skips only the entry it is in.

Abilities and passives of all files are read before any class, so a class may use a spell that
another file defines. Ability and passive keys share one namespace together with the built-in
keys, and class names are unique ignoring case.

## Example

`docs/examples/mods/spellblade.toml` adds a warrior-mage hybrid:

```toml
[[ability]]
key = "arcane_blade"
spell = 1449
kind = "Nova"
damage = 18.0
cooldown = 3.0
radius = 7.0
effect = { Slow = [0.5, 2.0] }

[[class]]
name = "Spellblade"
base = "Warrior"
start = "cleave"
pool = ["cleave", "whirlwind", "frostbolt", "blizzard", "arcane_blade"]
```

Copy it into the mod folder and the menu shows Spellblade after the built-in classes.

## `[[ability]]`

Required: `key`, `spell`, `kind`. Everything else is optional and takes the default of the
built-in constructor.

| field | type | default | meaning |
|---|---|---|---|
| `key` | string | | unique name, not a built-in key, not used by another ability or passive |
| `spell` | integer | | a `Spell.dbc` id, for the icon, the name and the visuals; not 0 |
| `kind` | string | | what the ability does, see below |
| `damage` | number | 0 | direct damage; per tick for areas and pulses; absorb, heal or reflect for the self kinds |
| `dot` | number | 0 | damage over time, per second, for `duration` |
| `cooldown` | number | 2.0 | seconds between casts; must be above 0 |
| `range` | number | 20.0 | reach in yards |
| `radius` | number | 0 | splash for single hits, area for the area kinds, the jump for a chain |
| `count` | integer | 1 | how many targets or missiles; at least 1 |
| `duration` | number | 0 | seconds, for areas, dots and the self kinds |
| `effect` | string or table | `"None"` | an effect on what is hit, see below |
| `mark` | integer | 0 | a spell whose state visual marks an affected enemy, 0 for none |
| `totem` | `[display, spell]` | `[0, 0]` | a totem's display id and the spell it casts |
| `overhead` | boolean | false | the area's own visual hangs overhead; play the spell's impact on what it hits instead |

All numbers must be finite and 0 or more.

### Kinds

| kind | what it does |
|---|---|
| `Bolt` | a missile at the nearest enemies (instant if the spell has no speed) |
| `Strike` | an instant hit on the nearest enemies in range |
| `Melee` | a close hit on the nearest enemies in melee reach |
| `Nova` | everything in a radius around the hero |
| `Cone` | everything in a cone toward the nearest enemy |
| `Ground` | a ground area at the densest nearby enemy, ticking for `duration` |
| `GroundSelf` | a ground area under the hero, ticking for `duration` |
| `Chain` | jumps from enemy to enemy |
| `Dot` | a damage over time on several enemies |
| `Aura` | always on: pulses every `cooldown` around the hero, with the spell's state visual on him |
| `Orbit` | always on: zaps one enemy close to the hero every `cooldown` |
| `Shield` | absorbs `damage` for `duration` |
| `Immune` | immune for `duration` |
| `Thorns` | returns `damage` to every attacker for `duration` |
| `Heal` | heals `damage` over `duration` |
| `Totem` | drops a totem that casts the second `totem` spell for `duration` |

### Effects

Written as a string for `None`, or as a one-entry table:

| effect | written as | meaning |
|---|---|---|
| `None` | `effect = "None"` | no effect |
| `Slow` | `effect = { Slow = [0.5, 2.0] }` | speed multiplier, seconds |
| `Root` | `effect = { Root = 2.5 }` | seconds |
| `Stun` | `effect = { Stun = 1.5 }` | seconds |
| `Fear` | `effect = { Fear = 2.0 }` | seconds |
| `Weaken` | `effect = { Weaken = [0.7, 4.0] }` | outgoing damage multiplier, seconds |
| `Drain` | `effect = { Drain = 0.2 }` | heal the hero by this fraction of damage dealt |
| `Execute` | `effect = { Execute = 0.15 }` | kill outright below this health fraction |

## `[[passive]]`

All fields are required. A passive is offered to every class at level-up, like the built-in ones.

| field | type | meaning |
|---|---|---|
| `key` | string | unique, same namespace as ability keys |
| `name` | string | shown on the card |
| `spell` | integer | a `Spell.dbc` id for the icon, and a state visual on the hero where the spell has one |
| `stat` | string | `Might`, `MaxHp`, `Speed`, `Regen`, `Haste`, `Area`, `Magnet`, `Growth`, `Armor` or `Crit` |
| `per_level` | number | what one rank adds to the stat; 0 or more |

```toml
[[passive]]
key = "arcane_edge"
name = "Arcane Edge"
spell = 1459
stat = "Crit"
per_level = 0.03
```

## `[[class]]`

| field | type | meaning |
|---|---|---|
| `name` | string | shown in the menu; unique ignoring case, also against the built-in classes |
| `base` | string | a built-in class (ignoring case); its id, power type, colour, looks, weapons and sheath are inherited |
| `start` | string | the ability the run starts with; must be in `pool` |
| `pool` | list of strings | the abilities the class can find; built-in keys and mod keys, no repeats |
| `color` | integer, optional | `0xRRGGBB`, replaces the base class's colour |

A class is skipped when the name is taken, the `base` is not a built-in class, a key in `pool` or
`start` exists nowhere, `pool` repeats a key, or `start` is not in `pool`.

## Limits

- Visuals come from `Spell.dbc`. An ability shows what its `spell` id shows, so a new ability
  needs a real spell id whose look fits. There are no custom models, textures or sounds.
- The class look comes from `base`: same race and sex choices, same weapons, same colour unless
  `color` is given. A mod class shares the base class's id, so it also shares what is keyed by
  it: the talent trees, the movement ability, the best-per-zone records and the class icon.
- Mod classes are appended after the built-in ones and saved progress (level, unlocks) follows
  the class's position in that list. Adding, removing or reordering mod classes, or renaming
  files so they sort differently, moves that progress to another class.
- Co-op sends the class by id, so a mod class reaches the other players as its base class
  (name and icon). Abilities are sent by key and a key the other side does not know is dropped,
  so run co-op with the same mod files on every machine.
- Evolutions are a built-in table; a mod cannot add recipes yet.
- Scripting (stage 2, Lua) is not there.
