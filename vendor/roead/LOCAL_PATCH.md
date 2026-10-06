Source: GingerAvalanche/roead commit 5caec3ee, version 1.0.0, as pinned by
UKMM's existing Cargo.lock. The original license notices and tracked native
compression dependencies are retained. The upstream game-resource test folder
and Git metadata are not included.

Local fix: AAMP StringRef and data-type strings are null-terminated,
variable-length UTF-8. The parser previously copied them into a 0x256-byte array,
panicking for longer custom actor strings. Replace that array with a growing
vector and validate UTF-8. Unterminated/truncated strings return a read error.
Fixed String32/String64/String256 parsing is unchanged.

Build adjustment: configure zlib-ng under Cargo OUT_DIR, check CMake failures,
set the legacy CMake policy floor explicitly for CMake 4, and link the matching
Debug/Release static library. This avoids requiring untracked prebuilt libraries
inside the vendored source directory.
