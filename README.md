# avatar-anim

A Rust library for parsing, inspecting and transforming Second Life avatar animation (`.anim`) files and Firestorm poser LLSD XML.

## Features (brief)

- Parse & write `.anim` files (binary) using `binrw`
- Import poser LLSD XML (`Animation::from_llsd_file`)
- Safe quaternion reconstruction & normalization
- Key utilities: drop, filter, duplicate cleanup strategies (first/last/average)
- SL avatar_skeleton.xml loader for default bone positions
- Quantization helpers with documented error bounds
- Unified `AnimError` + `Result<T>` alias
- Minimal fluent editing API (priority, stripping rotations/positions)
- Example CLI (`examples/animctl.rs`) for info, convert, joints, completions

## Quick Start

Add to your project:

```bash
cargo add avatar-anim
```

Example (load, tweak priority, write):

```rust
use avatar_anim::Animation;
let mut anim = Animation::from_file("walk.anim")?;
anim.set_priority(4).cleanup_keys_with(avatar_anim::DuplicateKeyStrategy::KeepLast);
anim.to_file("walk_p4.anim")?;
Ok::<_, avatar_anim::Error>(())
```

## CLI (example)

Build and run the example tool:

```bash
cargo run --example animctl -- info walk.anim
cargo run --example animctl -- convert -i pose.xml -o pose.anim --insert Head:rot@42
cargo run --example animctl -- convert -i pose.xml -o pose.anim --drop-zero-positions --add-skeleton-positions avatar_skeleton.xml
cargo run --example animctl -- joints -j Spine walk.anim
cargo run --example animctl -- skeleton-bones --skeleton avatar_skeleton.xml --prefix mTail
cargo run --example animctl -- position-reset --skeleton avatar_skeleton.xml --prefix mTail -o reset.anim
```

`position-reset` creates a priority 6, position-only reset pose for selected
bones using local `pos` values from Second Life's `avatar_skeleton.xml`. Select
bones with `--joint`, `--prefix`, `--group`, or `--all`. Normal `convert` output
remains unchanged and only includes position keys when the source or explicit
`--insert joint:pos...` requests them.

Use `convert --add-skeleton-positions avatar_skeleton.xml` when poser XML
position values are deltas from the default skeleton and need to become local
bone positions in the written `.anim`. Add `--drop-zero-positions` first when
zero deltas should remain omitted instead of becoming skeleton reset positions.

Shell completion script:

```bash
cargo run --example animctl -- complete --shell bash > animctl.bash
```

## License

LGPL-2.1
