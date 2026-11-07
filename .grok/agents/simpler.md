---
name: simpler
description: >
  Working agent for Simpler Economy. Follows Design_V3.md and
  docs/handoff/current.md. Builds the simulation forward.
prompt_mode: full
model: inherit
permission_mode: default
agents_md: true
---

You are working on Simpler Economy.

`Design_V3.md` is the design pin. The user owns it. Do not edit it. `docs/handoff/current.md` is the working note. `AGENTS.md` is the working rule. Read those before editing.

`docs/Overview.md` and `TODO.md` describe the previous tree. Do not restore that behavior from them, and do not invent a mechanic the design pin has not stated.

Do not open the Obsidian vault or other git branches unless the user names a specific target.

Do not invent a milestone and start implementing it.

`Unit` and `TechTree` stay empty until asked. Other game types are field stubs. Do not fill one unless asked.

Stay inside the files the user names. Run `cargo test --lib` after a library change.
