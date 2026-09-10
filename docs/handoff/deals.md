# Deals, tenders, and transport

Read this only for `DealMaker`, baskets, AMV keep, tenders, whole-unit
exchange, or the wagon bill. Match loop: `docs/handoff/market.md`. Names:
vocabulary (deal, take tenders, make change, AMV keep, whole units, friction).

## Landed vs stub

| Piece | Status |
|-------|--------|
| `buy` / `evaluate` / `finalize` | Landed. `buy` and `evaluate` are **read-only**; `finalize` mutates inventory |
| `sell` | Identity. No rewrite / haggling |
| Verdicts | Accept / Reject only. Change / Counteroffer / HardReject exist unused |
| Make change | Reserved, no helper |
| `take_good` | Landed on Pop and Firm |

## Traps

- `ProposedDeal.goods` is the **seller's inventory change** (negative = sold,
  positive = tender). Seller adds the map; buyer subtracts it.
- `buy` ranks the seller's named counter first (any salability), then live
  tenders by salability (pop: free stock above `shop_target.max(reserved)`,
  minus listed offer qty on `current_orders`; firm: `free_for_market`
  minus units `create_orders` would sell or liquidate). Pop offer
  `counter_offer` is a good-only hint (no amount); payment still uses market
  AMV. `take_tenders` then covers from that set plus `HIGH_SALABILITY`
  (`0.8`). Low-sal only if those cannot cover. Shrink fill only after all
  tenders. Payment AMV above `amv_target` or the row buy cap does **not**
  void the basket.
- **Make change** is returning excess, not `take_tenders`.
- Keep = received AMV / given AMV. Given goods are full AMV. Received
  use-goods (pop shop/desire, firm `use_target`) skip salability; others *
  salability. A pop that receives any used/desired good **ignores** the AMV
  floor. Unused-only baskets must keep `0.50` after the haircut. Firm min
  `0.50` with a need-catch to `0.25` when `purchase_target` or `use_target`.
  Merchant restock is a **need**, not a use (still takes the haircut).
  Buyers accept windfalls (`keep >= 1.0`).
- `finalize` does not raise reserve toward stock and does not edit orders.
  Firm records bought/sold AMV and blends `average_cost` at market AMV on inflows.
- `take_good` removes the property row and returns qty (`0` if missing).
- A sell-plan good can still tender its exchange slice; mid-day salability
  reclassify vs the morning sell order can overdraw (later: freeze the morning
  split). Pop offers are frozen out of tenders via `current_orders`.

## Whole units vs fractional

Orders and deal-map **goods** are whole units (including a transport-tagged
good in the deal map). Inventory may hold fractions. Pop **requests** ceil
the shop shortfall; **offers** floor leftover free stock. Payment ceils the
AMV (or named-counter) cost of the largest whole fill on-hand can cover.
Helpers: `util::whole_units`, `whole_units_up`.

**Not whole-unit:** AMV, and the wagon bill (may spend a fraction of cargo).

## Transport / wash

`transport_needed = TRANSACTION_COST + bulk * market.friction` with
`bulk = Sum(|qty| * (mass + 400 * volume))`. Buyer pays in Transport-tagged
goods (cover = `qty * efficiency`). Seller never receives the spent units.

- **Success:** cap fill so post-exchange cover can pay, *then* form the basket,
  then spend the **full** bill. Do not also charge the door fee at the start.
- **Wash** (reject / no proposal): `TRANSACTION_COST` from **on-hand** only,
  then `renew_buy` until `BUY_TRY_LIMIT` 2. A **reject** also cuts that
  sell/offer weight by 10% for the rest of the day.
- **Unavailable:** no meeting, no fee. Lives on `Market.unavailable_goods`.
- No Transport tag in the world => bill 0.

Stale (notify only): `DealMaker::renew_buy` rustdoc still describes const
defaults (live uses `renew_buy_with_limit`).

**Code:** `deal.rs`, `pop/deal.rs`; tunables `factuals.config.deal` / `market`.
