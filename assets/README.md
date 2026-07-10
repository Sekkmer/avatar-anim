# Bundled Second Life skeleton

`avatar_skeleton.xml` is copied from Linden Lab's open-source Second Life
viewer repository:

<https://github.com/secondlife/viewer/blob/develop/indra/newview/character/avatar_skeleton.xml>

The viewer and this project are distributed under LGPL-2.1. The file is
embedded into native and WebAssembly builds with `include_str!`, allowing the
editor to work offline and without asking users to find a viewer source tree.

Run `scripts/update-avatar-skeleton.sh` from anywhere in the repository to
refresh the checked-in copy and run its parser test.
