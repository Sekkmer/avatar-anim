# avatar-anim

A Rust library for parsing, inspecting and transforming Second Life avatar animation (`.anim`) files and Firestorm poser LLSD XML.

## Features (brief)

- Parse & write `.anim` files (binary) using `binrw`
- Import poser LLSD XML (`Animation::from_llsd_file`)
- Safe quaternion reconstruction & normalization
- Key utilities: drop, filter, duplicate cleanup strategies (first/last/average)
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
cargo run --example animctl -- joints -j Spine walk.anim
```

Shell completion script:

```bash
cargo run --example animctl -- complete --shell bash > animctl.bash
```

## License

LGPL-2.1
