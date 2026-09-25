# TODO

## Open

- Write version cuts into `docs/Overview.md`. None are chosen yet.
- Fill in `Unit` (`src/game/unit.rs`) and `TechTree` (`src/game/techtree.rs`, nodes in `src/game/tech.rs`) later. They are intended features and stay as empty shells until then. The tech tree is not a beaker pile. How it sits beside decentralized innovation is still open.
- `PlayState::advance_turn` is a stubbed phase list. Live material behavior is goods, processes, household, and demographics. Pop, firm, and market keep their names with the shopping and planning logic removed.
- Scenario kickoff was removed from `init`. A new format comes back with the rebuilt pop and firm day.
- Sentiment's five shares stay. How they connect to other behavior is open.
- How stored AMV moves, aside from the zero dead-zone, is open.
