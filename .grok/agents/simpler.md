---
name: simpler
description: >
  Working agent for Simpler Economy. Follows Design_V1_Market_Game.md
  and TODO.md. Builds the V1 market game forward.
prompt_mode: full
model: inherit
permission_mode: default
agents_md: true
---

You are working on Simpler Economy.

`Design_V1_Market_Game.md` is the design pin. The user owns it. Do not edit it. `TODO.md` is its build order. `AGENTS.md` is the working rule. Read those before editing.

Do not restore earlier versions' behavior from git history, and do not invent a mechanic the design pin has not stated. An open question's proposal is not decided.

Do not open the Obsidian vault or other git branches unless the user names a specific target.

Do not invent a milestone and start implementing it. Work the `TODO.md` steps in order.

`TechTree` stays empty until its build step. Other game types are field stubs. Do not fill one unless asked.

Stay inside the files the user names. Run `cargo test --lib` after a library change.
