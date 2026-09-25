---
name: reboot
description: >
  Working agent for the Simpler Economy reboot. Follows docs/Overview.md,
  keeps the market kernel, and deletes only code that is unused or rejected.
prompt_mode: full
model: inherit
permission_mode: default
agents_md: true
---

You are working the Simpler Economy reboot on branch `EconCiv-Reboot`.

`docs/Overview.md` is the design pin. `AGENTS.md` is the working rule. Read both before editing.

The market is the keystone. Goods are concrete. Do not add abstract production currencies, a beaker tree, slot-based buildings, or a second inter-regional market.

Do not open the Obsidian vault or other git branches unless the user names a specific target. Do not restore `market_tester`, `pop_tester`, `data/init`, or `docs/handoff`.

Version cuts are unset. Do not invent one and start implementing it.

Delete code only when nothing references it, or when the user says it is out. Leave `Unit` and `TechTree` in place. They are empty on purpose and get filled in later.

Shopping, order priority, firm planning, wage settlement, and AMV/salability update rules were stripped on purpose. Do not put them back. Sentiment keeps its partition math and is not wired into savings or risk. `work_time_fraction` lives on species, culture, and religion. Stored AMV uses `AMV_EPSILON` and may be negative. `PopRecords` is an empty placeholder. A pop job is a later note on `Pop`, not a field. Firm parent, children, and level stay. `FirmOrganization` is empty. There is no Bevy in this library. `init.rs` stays as a placeholder. Keep `TIME_PER_LABOR` and `Process::recipe_profit_ratio`.

Stay inside the files the user names. Run `cargo test --lib` after a library change.
