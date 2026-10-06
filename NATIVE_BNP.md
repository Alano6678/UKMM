# Native BNP support

This branch supports installing and storing BCML BNP archives without creating
an intermediate UKMM ZIP. BNP and ZIP mods supply typed resources to the same
UKMM merge engine and can share a profile and load order.

The BNP archive remains unchanged. Its logs are evaluated against the configured
game dump in a temporary directory, then decoded into memory, including option
layers. This implementation still reconstructs resources and computes UKMM
differences; it is not yet a direct translation of every BNP log into UKMM patch
operations. Opening a large BNP may take time and consume substantial memory.
Up to two decoded archives are cached within the running process. Decoding runs
again after restarting, or changing the configured dump or game language.

## Behavior and fixes

- Preserve BNP `priority: base`, default options, dependency names/versions, and
  author/URL metadata. Dependencies must be installed and enabled for merging.
- Retain `rstb.json` per layer: zero removes an entry; positive declarations are
  memory floors combined with final-resource estimates.
- Evaluate option logs against the reconstructed parent resources, including
  quest and DLC/map packs.
- Write static MainField maps back into their archive and dynamic maps as files.
- Preserve complete map entries when a BNP ID already exists in the configured
  base/DLC copy, and compare map float values exactly when generating differences.
- Surface malformed resources and unsupported logs instead of reporting success
  while discarding changes.
- Fix empty C++ substring handling in the vendored `ryml` dependency. Provenance
  and the small local change are documented in `vendor/ryml/LOCAL_PATCH.md`.

## Validation

Targeted tests cover native resource layering, mixed BNP/ZIP load order, RSTB
floors/removals, dependency checks, archive storage/restart, default options,
invalid-resource errors, small coordinate edits, and YAML empty substrings.

`crates/uk-manager/examples/native_bnp_audit.rs` reads a settings file, opens a BNP,
selects its defaults, and exports to an explicit audit directory. It does not
write the existing settings, installed mods, profiles, or deployment location.
For example:

```powershell
cargo run --release -p uk-manager --example native_bnp_audit -- `
  'C:\path\settings.yml' 'C:\path\mod.bnp' 'E:\audit\new-output'
```

Second Wind v1.9.14-alpha-6 has been exported with both default options using the
configured Wii U dump and JPja language. Independent checks against the original
logs passed for 201 quests, 2,607 ActorInfo patches, 2,271 game-data flag additions,
97 map sections (5,519 checked objects/rails), two separate layout logos, and
10,883 resource-size declarations. Content checks do not establish game-runtime correctness
or compatibility with every other mod. Existing ZIP installations are not
automatically migrated to BNP storage. Use a separate profile/storage for testing.

## Current scope

The native archive route targets BNPs with BCML 3 logs; the existing 2.x upgrade
helper remains available. Game-platform and language settings still apply.
Text logs are evaluated for the configured game language, with the existing
nearest-language fallback. Additional language preservation, arbitrary nested
archive depth, and Cemu code patch files are not newly implemented here.
