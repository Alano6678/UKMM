This copy comes from GingerAvalanche/ryml, branch no_std_io2, commit
5d542100bc076e378f42b4183772caa92ca8a53b (version 0.3.2).

Local change: empty C++ substrings may have null pointers. Return an empty string
before constructing a Rust slice, and use a non-null dangling pointer for the
mutable empty slice. Rust slices require a non-null pointer even when empty.
The original dependency metadata, source, and header license notices are retained.
