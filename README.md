# avatar-anim

A Rust library for parsing, inspecting and transforming Second Life avatar animation (`.anim`) files, BVH animations, and Firestorm poser LLSD XML.

## Features (brief)

- Parse & write `.anim` files (binary) using `binrw`
- Dependency-free, WASM-friendly BVH hierarchy and motion parser
- Convert SL-compatible BVH using Firestorm/LL reference-frame, alias, axis, and timing rules
- Import poser LLSD XML (`Animation::from_llsd_file`)
- Safe quaternion reconstruction & normalization
- Key utilities: drop, filter, duplicate cleanup strategies (first/last/average)
- SL avatar_skeleton.xml loader for default bone positions
- Quantization helpers with documented error bounds
- Unified `AnimError` + `Result<T>` alias
- Minimal fluent editing API (priority, stripping rotations/positions)
- Example CLI (`examples/animctl.rs`) for info, convert, joints, completions
- Browser editor for Firestorm XML/BVH and `.anim` files, built with Leptos and WebAssembly

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
cargo run --example animctl -- convert -i pose.bvh -o pose.anim
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

### BVH import

BVH syntax parsing and Second Life conversion are separate APIs. This keeps the
parsed source hierarchy available to general tools while making SL-specific
conversion explicit:

```rust
use avatar_anim::{Animation, SkeletonDefinition, bvh::BvhImportOptions};

let skeleton = SkeletonDefinition::embedded_avatar()?;
let animation = Animation::from_bvh_file(
    "walk.bvh",
    &skeleton,
    &BvhImportOptions::default(),
)?;
animation.to_file("walk.anim")?;
# Ok::<_, avatar_anim::Error>(())
```

The converter follows the viewer convention that BVH frame zero is a hidden
reference frame. It resolves canonical and legacy joint aliases from the
bundled LL skeleton, handles all Euler channel orders, converts viewer BVH
positions from inches to metres, and optionally removes redundant keys.
Arbitrary mocap skeletons can be parsed, but converting them into useful SL
motion still requires their joints to be named or retargeted to the LL rig.

Shell completion script:

```bash
cargo run --example animctl -- complete --shell bash > animctl.bash
```

## Web editor

The `web/` workspace member is a client-only Leptos application. It embeds the
official Linden Lab avatar skeleton, opens Firestorm poser XML/BVH and Second
Life `.anim` files entirely in the browser, previews the rig, exposes rotations as
Euler degree vectors, and downloads the resulting `.anim` without uploading
source files to a server.

For local development:

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.21.14 --locked
cd web
trunk serve --open
```

Prebuilt Trunk binaries are also available from its official GitHub releases;
the Pages workflow downloads and checksum-verifies the pinned Linux binary.

The GitHub Pages workflow builds with the repository subpath automatically.
Refresh the bundled skeleton from Linden Lab with:

```bash
./scripts/update-avatar-skeleton.sh
```

## License

LGPL-2.1
