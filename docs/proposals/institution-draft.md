# Institutions (v0)

**Status:** implemented in `src/game/institution.rs`  
**Market membership:** `Market.institution_ids` (ids only; multi-market via `Institution.markets`)

## What an institution is

| Layer | Role | In code (v0) |
|---|---|---|
| **Kind** | Military, Religion, Guild, … | `InstitutionKind` |
| **Instance** | Your Admiralty / their Church | `Institution` in `Actors.institutions` |
| **Arms** | Garrisons, temples-as-firms | `firm_ids` → `Actors.firms` |

Culture / Religion stay demographic identity. Institutions are formal orgs that can later sponsor or steer them; culture points buy progress later, not actors today.

## Runtime (v0)

- `id`, `name`, `owner: Option<usize>` (state id)
- `kind`, `markets`, `firm_ids`
- `level`, `loyalty`, `market_slot`
- `effects: Vec<InstitutionEffect>` (scoped birth/mortality today; expand in `effects.rs`)

Fluent `new` + `with_*` builders. No property, contracts, trees, or mandates yet.

## Architecture rules

1. Do not merge Institution and State (State is the player shell).
2. Prefer controlling firms over giving institutions full production lines.
3. Do not dual-own firms on institution *and* market membership maps.
4. Household / desire changes apply via effects and demographic rebuild — not by rewriting pop data ad hoc in the institution type.

## Deferred

- Property / contracts / market-day buying
- Ability trees (`InstitutionTree` / nodes) and exclusive branches
- Mandates + loyalty scoring
- Wiring passive household effects into `demographic_update`
- State.institution_ids registry (optional convenience)
- Factuals institution trees
