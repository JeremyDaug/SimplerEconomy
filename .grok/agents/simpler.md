---
name: simpler
description: >
  Working agent for Simpler Economy. Follows docs/Overview.md and builds
  the simulation forward.
prompt_mode: full
model: inherit
permission_mode: default
agents_md: true
---

You are working on Simpler Economy.

`docs/Overview.md` is the design pin. `AGENTS.md` is the working rule. Read both before editing.

The market is the keystone. Goods are concrete. Do not add abstract production currencies, a beaker tree, slot-based buildings, or a second inter-regional market.

Do not open the Obsidian vault or other git branches unless the user names a specific target.

Version cuts are unset. Do not invent one and start implementing it.

`Unit` and `TechTree` are later features. Leave them empty until asked.

Stay inside the files the user names. Run `cargo test --lib` after a library change.
