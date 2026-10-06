# Native BNP support

This branch supports installing and storing BCML BNP archives without creating
an intermediate UKMM ZIP. BNP and ZIP mods supply typed resources to the same
UKMM merge engine and can share a profile and load order.

The BNP archive remains unchanged. Its logs are evaluated against the configured
game dump in a temporary directory, then decoded into memory, including option
layers. This implementation still reconstructs resources and computes UKMM
differences; it is not yet a direct translation of every BNP log into UKMM patch
operations. Opening a large BNP may take time and consume substantial memory.
Up to two decoded archives are cached within the running process. A versioned,
checksummed disk cache under `<platform storage>/cache/bnp` reuses decoded
resources after restarting. Identical BNP copies share the same content key, so
copying a mod into installed storage does not decode it again.

The key includes the BNP SHA-256, platform/language, game-dump configuration and
file size/modification stamps. Dump stamps are checked at settings registration;
restart/reload settings after changing a dump. A damaged cache falls back to
decoding the original BNP. Cache-write failure does not prevent installation.
The cache can be deleted to reclaim disk space or force decoding. If resource
decoding or cache serialization semantics change, bump `SCHEMA` in
`crates/uk-manager/src/bnp/cache.rs`.

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
- Ignore unused empty AAMP file-table slots while preserving every nonempty edit.
  Invalid paths report their quoted value and the archive/log/option context;
  absolute paths, parent traversal, and empty nested path components remain errors.
- Fix empty C++ substring handling in the vendored `ryml` dependency. Provenance
  and the small local change are documented in `vendor/ryml/LOCAL_PATCH.md`.
- Read variable-length AAMP StringRef values without the former 598-byte limit,
  validate UTF-8, and report truncated strings. The pinned `roead` source and
  local build adjustment are documented in `vendor/roead/LOCAL_PATCH.md`.
- Preserve custom top-level AIProgram objects/lists through parsing, difference
  generation, merging, writing, and native BNP caches. Indexed AI graph nodes
  can be renumbered during export; their graph semantics are checked separately.
- Keep embedded BFRES models keyed by their full archive ancestry. An external
  model and two nested models with the same filename retain their own bytes.
  Older ZIPs with flattened model keys retain the existing fallback behavior.
- Remove both CRC and named RSTB entries for explicit removals, including the
  duplicate Armor_160_Head model-list entry present in the stock Switch table.
- Resolve RSTB declarations by CRC: same-mod positive aliases contribute their
  largest floor; explicit removals win within a mod, and later mods override
  earlier declarations. This prevents synthetic port floor names and real
  resource names with the same CRC from competing in unordered map traversal.

## Validation

Targeted tests cover native resource layering, mixed BNP/ZIP load order, RSTB
floors/removals, dependency checks, archive storage/restart, default options,
invalid-resource errors, small coordinate edits, and YAML empty substrings.
Performance regression tests additionally cover cache corruption/invalidation,
original/archive-copy identity, shared native resource references, manifest
invalidation after settings changes, and parallel ZIP dictionary decompression.
Opaque binary replacements avoid reading/decompressing the vanilla asset when
all applicable mod layers are binary. A candidate nested-output cache was
discarded after the Second Wind benchmark showed slower merging.
Additional regressions cover nested models sharing names with external files,
logical RSTB names for scoped models, duplicate hash/named RSTB removals, and
long custom AIProgram fields across parsing, merging, and persistent caching.

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

The user-provided Switch port of Second Wind v1.9.14-alpha-6 was installed and
exported with both defaults and CNzh using an independent storage directory.
Checks against its original logs passed for 201 quests, 2,608 ActorInfo patches,
2,271 game-data additions, 97 map sections (5,519 objects/rails), 10,984 RSTB
declarations, and 7,103 CNzh text entries. Four empty text fragments are omitted
by MSBT encoding without changing text or control commands. The complete
ElementalRodAttack AI graph and its custom 1,024-byte StringRef were preserved.
This fixes the archive's previously reproducible parser panic without editing
the original BNP or truncating its parameters.

## Settings-save performance

The Save button converts edited platform forms on a background worker. Changes
to deployment preferences reuse the existing game reader when its actual dump
directories are unchanged (empty and absent optional paths are equivalent).
Ordinary preferences save the existing settings object without reparsing the
YAML, reloading installed mods, invalidating BNP caches, or clearing ROM caches.
Platform/storage changes rebuild the managers; profile, game-language and dump
changes refresh the affected state. Failed writes leave runtime settings intact,
and failed manager initialization restores the previous saved configuration.

Targeted tests cover the GUI form-to-settings path, cache reuse, profile/storage/
language/dump changes, optional empty paths, failed writes, and failed reloads.

## Current scope

The native archive route targets BNPs with BCML 3 logs; the existing 2.x upgrade
helper remains available. Game-platform and language settings still apply.
Text logs are evaluated for the configured game language, with the existing
nearest-language fallback. Additional language preservation, arbitrary nested
archive depth, and Cemu code patch files are not newly implemented here.
