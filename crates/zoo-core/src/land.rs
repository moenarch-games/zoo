//! Purchasable land and park-expansion planning (#99).
//!
//! A scenario may author land parcels inside its bounded map: each has a stable slug ID, a
//! title, a trade-off summary, one or more rectangles, a purchase price and an optional unlock
//! condition (a state-backed [`Goal`]). Every tile outside an unowned parcel is the park's own
//! land; a parcel's tiles become owned land the moment it is bought, and a park never sells
//! land again. The map itself never grows and the active park is never regenerated.
//!
//! Ownership is placement truth: every construction plan (path strokes and plazas, stands, new
//! and reshaped habitats, service gates) rejects unowned tiles through the same evaluator its
//! preview uses, and names the parcel to buy. Decoration on unbought land is presentation only.
//!
//! A purchase is one all-or-nothing transaction over a selection of parcels. The quote lists
//! every selected parcel with its price and status and carries a token derived from them;
//! buying plans again against the current park and refuses, spending nothing, when any parcel
//! is unknown, repeated, already owned or still locked, when the park cannot afford the total,
//! or when the quote no longer matches. Land purchases are final: they are not construction
//! and never enter the undo history.
use super::{
    ActionResult, ExpenseCategory, GameState, Position, TileKind,
    construction::dollars,
    content,
    scenario::Goal,
    session::{DomainEvent, fnv1a64},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// The most parcels a scenario may author.
pub(crate) const MAX_PARCELS: usize = 16;
/// The most rectangles one parcel may be made of.
const MAX_PARCEL_AREAS: usize = 8;
const MAX_ID_BYTES: usize = 64;
const MAX_TEXT_BYTES: usize = 400;

/// One rectangle of a parcel, in park tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Area {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl Area {
    fn contains(self, tile: Position) -> bool {
        (self.x..self.x.saturating_add(self.width)).contains(&tile.x)
            && (self.y..self.y.saturating_add(self.height)).contains(&tile.y)
    }

    fn tiles(self) -> impl Iterator<Item = Position> {
        (self.y..self.y.saturating_add(self.height)).flat_map(move |y| {
            (self.x..self.x.saturating_add(self.width)).map(move |x| Position { x, y })
        })
    }
}

/// When a parcel may be bought; the description is the player-facing restriction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Unlock {
    goal: Goal,
    description: String,
}

/// A parcel as a scenario authors it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ParcelDefinition {
    id: String,
    title: String,
    /// What makes this parcel worth buying, or not: the trade-off in one sentence.
    summary: String,
    areas: Vec<Area>,
    price_cents: i64,
    #[serde(default)]
    unlock: Option<Unlock>,
}

/// A park's own copy of a parcel and whether the park owns it yet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Parcel {
    definition: ParcelDefinition,
    owned: bool,
}

impl Parcel {
    pub(crate) fn unowned(definition: ParcelDefinition) -> Self {
        Self {
            definition,
            owned: false,
        }
    }

    fn contains(&self, tile: Position) -> bool {
        self.definition.areas.iter().any(|area| area.contains(tile))
    }

    /// Every tile of the parcel, row-major and without repeats.
    fn tiles(&self) -> Vec<Position> {
        let mut tiles: Vec<Position> = self
            .definition
            .areas
            .iter()
            .flat_map(|area| area.tiles())
            .collect();
        tiles.sort_by_key(|tile| (tile.y, tile.x));
        tiles.dedup();
        tiles
    }
}

fn text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > MAX_TEXT_BYTES {
        return Err(format!(
            "{field}: expected 1..={MAX_TEXT_BYTES} bytes of text"
        ));
    }
    Ok(())
}

/// Structural checks shared by scenario authoring and loaded saves: unique slug IDs, text,
/// prices, rectangles inside the map, no overlap between parcels, and the entrance and depot
/// always on the park's own land.
pub(crate) fn validate_parcels<'a>(
    parcels: impl ExactSizeIterator<Item = &'a ParcelDefinition>,
    width: u32,
    height: u32,
    landmarks: [Position; 2],
) -> Result<(), String> {
    if parcels.len() > MAX_PARCELS {
        return Err(format!("land: at most {MAX_PARCELS} parcels"));
    }
    let mut ids = HashSet::new();
    let mut claimed: HashSet<(u32, u32)> = HashSet::new();
    for (i, parcel) in parcels.enumerate() {
        let field = format!("land/{i}");
        let slug = !parcel.id.is_empty()
            && parcel.id.len() <= MAX_ID_BYTES
            && parcel
                .id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        if !slug {
            return Err(format!(
                "{field}/id: expected 1..={MAX_ID_BYTES} lowercase letters, digits or hyphens"
            ));
        }
        if !ids.insert(parcel.id.as_str()) {
            return Err(format!("{field}/id: duplicate parcel {}", parcel.id));
        }
        text(&parcel.title, &format!("{field}/title"))?;
        text(&parcel.summary, &format!("{field}/summary"))?;
        if let Some(unlock) = &parcel.unlock {
            text(&unlock.description, &format!("{field}/unlock/description"))?;
        }
        if !(1..=content::MAX_CENTS).contains(&parcel.price_cents) {
            return Err(format!(
                "{field}/price_cents: expected 1..={}",
                content::MAX_CENTS
            ));
        }
        if parcel.areas.is_empty() || parcel.areas.len() > MAX_PARCEL_AREAS {
            return Err(format!(
                "{field}/areas: expected 1..={MAX_PARCEL_AREAS} rectangles"
            ));
        }
        let mut own: HashSet<(u32, u32)> = HashSet::new();
        for (j, area) in parcel.areas.iter().enumerate() {
            let fits = area.width > 0
                && area.height > 0
                && area.x.checked_add(area.width).is_some_and(|r| r <= width)
                && area.y.checked_add(area.height).is_some_and(|b| b <= height);
            if !fits {
                return Err(format!("{field}/areas/{j}: rectangle leaves the map"));
            }
            for tile in area.tiles() {
                if landmarks.contains(&tile) {
                    return Err(format!(
                        "{field}/areas/{j}: covers the entrance or depot, which the park always owns"
                    ));
                }
                if claimed.contains(&(tile.x, tile.y)) {
                    return Err(format!(
                        "{field}/areas/{j}: tile {},{} already belongs to another parcel",
                        tile.x, tile.y
                    ));
                }
                own.insert((tile.x, tile.y));
            }
        }
        claimed.extend(own);
    }
    Ok(())
}

/// Why construction on some tiles needs land first.
pub(crate) struct LandBlock {
    pub message: String,
    pub tiles: Vec<Position>,
    pub parcels: Vec<String>,
}

/// A parcel as the land panel and overlay show it.
#[derive(Serialize)]
pub(crate) struct ParcelView {
    id: String,
    title: String,
    summary: String,
    areas: Vec<Area>,
    price_cents: i64,
    owned: bool,
    /// `owned`, `available`, `unaffordable` or `locked`.
    status: &'static str,
    /// Why the parcel cannot be bought right now, if it cannot.
    restriction: Option<String>,
    /// The unlock condition, met or not.
    unlock: Option<String>,
    tiles: u32,
    /// Open grass a habitat, path or stand could use.
    buildable_tiles: u32,
    water_tiles: u32,
    /// Whether an existing public path or the entrance touches the parcel, so a path can be
    /// extended into it at once.
    path_frontage: bool,
    /// Price per buildable tile, rounded down; `None` when nothing is buildable.
    price_per_buildable_tile_cents: Option<i64>,
}

#[derive(Serialize)]
pub(crate) struct LandView {
    parcels: Vec<ParcelView>,
    /// Tiles the park owns, including every tile outside the authored parcels.
    owned_tiles: u32,
    total_tiles: u32,
}

/// One selected parcel in a purchase quote.
#[derive(Serialize)]
struct QuoteLine {
    id: String,
    title: Option<String>,
    price_cents: i64,
    /// `available`, `owned`, `locked`, `unknown` or `duplicate`.
    status: &'static str,
    reason: Option<String>,
}

/// What buying a selection of parcels would do, from the same plan the purchase runs.
#[derive(Serialize)]
pub(crate) struct LandQuote {
    ok: bool,
    message: String,
    lines: Vec<QuoteLine>,
    /// The price of every buyable parcel in the selection.
    total_cents: i64,
    cash_after_cents: i64,
    /// Identifies the selection's parcels, prices and statuses; a purchase must present it.
    quote: String,
}

impl GameState {
    fn parcel_at(&self, tile: Position) -> Option<&Parcel> {
        self.land.iter().find(|parcel| parcel.contains(tile))
    }

    /// Whether the park owns `tile`: every tile outside an unbought parcel is the park's.
    pub(crate) fn owns(&self, tile: Position) -> bool {
        self.parcel_at(tile).is_none_or(|parcel| parcel.owned)
    }

    /// The unowned tiles among `tiles` and the parcels to buy for them, with a message that
    /// names the parcels and their price; `None` when the park owns every tile.
    pub(crate) fn land_block(&self, tiles: &[Position]) -> Option<LandBlock> {
        let mut blocked = Vec::new();
        let mut parcels: Vec<&Parcel> = Vec::new();
        for &tile in tiles {
            if let Some(parcel) = self.parcel_at(tile).filter(|parcel| !parcel.owned) {
                blocked.push(tile);
                if !parcels
                    .iter()
                    .any(|known| known.definition.id == parcel.definition.id)
                {
                    parcels.push(parcel);
                }
            }
        }
        if parcels.is_empty() {
            return None;
        }
        let names: Vec<&str> = parcels
            .iter()
            .map(|parcel| parcel.definition.title.as_str())
            .collect();
        let price: i64 = parcels
            .iter()
            .map(|parcel| parcel.definition.price_cents)
            .fold(0, i64::saturating_add);
        let names = match names.as_slice() {
            [one] => (*one).to_owned(),
            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
            [] => unreachable!("at least one parcel blocks"),
        };
        Some(LandBlock {
            message: format!("Outside owned land: buy {names} ({}) first", dollars(price)),
            tiles: blocked,
            parcels: parcels
                .iter()
                .map(|parcel| parcel.definition.id.clone())
                .collect(),
        })
    }

    fn parcel_locked(&self, parcel: &Parcel) -> Option<String> {
        parcel
            .definition
            .unlock
            .as_ref()
            .filter(|unlock| !self.goal_reached(&unlock.goal))
            .map(|unlock| format!("Locked: {}", unlock.description))
    }

    fn parcel_view(&self, parcel: &Parcel) -> ParcelView {
        let tiles = parcel.tiles();
        let kinds: Vec<Option<TileKind>> = tiles.iter().map(|t| self.tile(t.x, t.y)).collect();
        let buildable = kinds
            .iter()
            .filter(|kind| **kind == Some(TileKind::Grass))
            .count() as u32;
        let water = kinds
            .iter()
            .filter(|kind| **kind == Some(TileKind::Water))
            .count() as u32;
        let path_frontage = tiles.iter().any(|tile| {
            self.neighbors(*tile).into_iter().any(|neighbor| {
                !parcel.contains(neighbor)
                    && self.owns(neighbor)
                    && self.walkable(super::Mover::Guest, neighbor)
            })
        });
        let price = parcel.definition.price_cents;
        let (status, restriction) = if parcel.owned {
            ("owned", None)
        } else if let Some(locked) = self.parcel_locked(parcel) {
            ("locked", Some(locked))
        } else if self.cash_cents < price {
            (
                "unaffordable",
                Some(format!(
                    "Needs {} more cash",
                    dollars(price.saturating_sub(self.cash_cents))
                )),
            )
        } else {
            ("available", None)
        };
        ParcelView {
            id: parcel.definition.id.clone(),
            title: parcel.definition.title.clone(),
            summary: parcel.definition.summary.clone(),
            areas: parcel.definition.areas.clone(),
            price_cents: price,
            owned: parcel.owned,
            status,
            restriction,
            unlock: parcel
                .definition
                .unlock
                .as_ref()
                .map(|unlock| unlock.description.clone()),
            tiles: tiles.len() as u32,
            buildable_tiles: buildable,
            water_tiles: water,
            path_frontage,
            price_per_buildable_tile_cents: (buildable > 0).then(|| price / i64::from(buildable)),
        }
    }

    pub(crate) fn land_view(&self) -> LandView {
        let total_tiles = self.width * self.height;
        let unowned: u32 = self
            .land
            .iter()
            .filter(|parcel| !parcel.owned)
            .map(|parcel| parcel.tiles().len() as u32)
            .sum();
        LandView {
            parcels: self.land.iter().map(|p| self.parcel_view(p)).collect(),
            owned_tiles: total_tiles - unowned,
            total_tiles,
        }
    }

    /// Quotes buying every parcel in `ids` together, exactly as [`Self::buy_land`] would.
    pub(crate) fn quote_land(&self, ids: &[String]) -> LandQuote {
        let mut seen = HashSet::new();
        let mut lines = Vec::with_capacity(ids.len());
        for id in ids {
            let parcel = self.land.iter().find(|p| &p.definition.id == id);
            let (status, reason) = match parcel {
                _ if !seen.insert(id.as_str()) => {
                    ("duplicate", Some("Selected more than once".to_owned()))
                }
                None => ("unknown", Some(format!("No parcel {id} in this park"))),
                Some(parcel) if parcel.owned => ("owned", Some("Already owned".to_owned())),
                Some(parcel) => match self.parcel_locked(parcel) {
                    Some(locked) => ("locked", Some(locked)),
                    None => ("available", None),
                },
            };
            lines.push(QuoteLine {
                id: id.clone(),
                title: parcel.map(|p| p.definition.title.clone()),
                price_cents: parcel.map_or(0, |p| p.definition.price_cents),
                status,
                reason,
            });
        }
        let total_cents = lines
            .iter()
            .filter(|line| line.status == "available")
            .map(|line| line.price_cents)
            .fold(0, i64::saturating_add);
        let token = lines
            .iter()
            .map(|line| format!("{}:{}:{}", line.id, line.price_cents, line.status))
            .collect::<Vec<_>>()
            .join("|");
        let quote = fnv1a64(format!("{token}|{total_cents}").as_bytes());
        let problem = lines.iter().find_map(|line| {
            line.reason.as_ref().map(|reason| {
                let name = line.title.as_deref().unwrap_or(&line.id);
                format!("{name}: {reason}")
            })
        });
        let message = if lines.is_empty() {
            Some("Select at least one parcel".to_owned())
        } else if let Some(problem) = problem {
            Some(format!("{problem} · nothing is bought"))
        } else if self.cash_cents < total_cents {
            Some(format!(
                "Not enough cash: the selection costs {} and the park has {}",
                dollars(total_cents),
                dollars(self.cash_cents)
            ))
        } else {
            None
        };
        let count = lines.len();
        LandQuote {
            ok: message.is_none(),
            message: message.unwrap_or_else(|| {
                format!(
                    "{count} parcel{} · {}",
                    if count == 1 { "" } else { "s" },
                    dollars(total_cents)
                )
            }),
            lines,
            total_cents,
            cash_after_cents: self.cash_cents.saturating_sub(total_cents),
            quote,
        }
    }

    /// Buys every parcel in `ids` as one transaction when `quote` still describes them.
    pub(crate) fn buy_land(&mut self, ids: &[String], quote: &str) -> ActionResult {
        let plan = self.quote_land(ids);
        if !plan.ok {
            return ActionResult::error(plan.message);
        }
        if plan.quote != quote {
            return ActionResult::error(
                "The quote is out of date: review the selection again · nothing is bought",
            );
        }
        if let Err(message) = self.spend(plan.total_cents, ExpenseCategory::Land) {
            return ActionResult::error(message);
        }
        let mut titles = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(index) = self.land.iter().position(|p| &p.definition.id == id) else {
                continue;
            };
            self.land[index].owned = true;
            titles.push(self.land[index].definition.title.clone());
            self.pending_events.push(DomainEvent {
                tick: self.elapsed_minutes,
                kind: "land_purchased",
                entity_id: u32::try_from(index).ok(),
            });
        }
        // Ownership changes no walkable tile, so every route and assignment stays as it was.
        ActionResult::ok(format!(
            "Bought {} · {}",
            titles.join(", "),
            dollars(plan.total_cents)
        ))
    }

    /// Land checks for a loaded park: valid parcels, and nothing built on land it does not own.
    pub(crate) fn land_violation(&self) -> Result<(), String> {
        validate_parcels(
            self.land.iter().map(|parcel| &parcel.definition),
            self.width,
            self.height,
            [self.entrance, self.depot],
        )?;
        for parcel in self.land.iter().filter(|parcel| !parcel.owned) {
            for tile in parcel.tiles() {
                if !matches!(
                    self.tile(tile.x, tile.y),
                    Some(TileKind::Grass | TileKind::Water)
                ) {
                    return Err(format!(
                        "tile {},{} is built on unowned parcel {}",
                        tile.x, tile.y, parcel.definition.id
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parcel(id: &str, x: u32, y: u32, width: u32, height: u32) -> ParcelDefinition {
        ParcelDefinition {
            id: id.to_owned(),
            title: id.to_owned(),
            summary: "A test parcel".to_owned(),
            areas: vec![Area {
                x,
                y,
                width,
                height,
            }],
            price_cents: 100,
            unlock: None,
        }
    }

    fn lakeside() -> GameState {
        super::super::scenario::Scenario::built_in("partly-prepared")
            .expect("built in")
            .create(super::super::default_catalog())
            .expect("valid scenario")
    }

    #[test]
    fn a_park_that_cannot_afford_the_selection_keeps_its_cash_and_land() {
        let mut park = lakeside();
        park.cash_cents = 900_000;
        let ledger = park.finance_today;
        let view = park.land_view();
        let paddock = view
            .parcels
            .iter()
            .find(|parcel| parcel.id == "east-paddock")
            .unwrap();
        assert_eq!(paddock.status, "unaffordable");
        assert_eq!(paddock.restriction.as_deref(), Some("Needs $500 more cash"));
        let ids = vec!["east-paddock".to_owned()];
        let quote = park.quote_land(&ids);
        assert!(!quote.ok);
        assert_eq!(
            quote.message,
            "Not enough cash: the selection costs $9500 and the park has $9000"
        );
        let result = park.buy_land(&ids, &quote.quote);
        assert!(!result.ok);
        assert_eq!(park.cash_cents, 900_000);
        assert_eq!(park.finance_today, ledger);
        assert!(park.land.iter().all(|parcel| !parcel.owned));
        // Exactly enough is enough.
        park.cash_cents = 950_000;
        let quote = park.quote_land(&ids);
        assert!(park.buy_land(&ids, &quote.quote).ok);
        assert_eq!(park.cash_cents, 0);
        assert_eq!(park.finance_today.land_expense_cents, 950_000);
    }

    #[test]
    fn parcels_must_be_unique_inside_the_map_and_clear_of_landmarks() {
        let landmarks = [Position { x: 0, y: 7 }, Position { x: 2, y: 6 }];
        let check =
            |parcels: Vec<ParcelDefinition>| validate_parcels(parcels.iter(), 20, 14, landmarks);
        assert!(check(vec![parcel("a", 10, 0, 4, 4), parcel("b", 14, 0, 4, 4)]).is_ok());
        let cases = [
            (
                vec![parcel("a", 10, 0, 4, 4), parcel("a", 14, 0, 4, 4)],
                "duplicate",
            ),
            (
                vec![parcel("a", 10, 0, 4, 4), parcel("b", 13, 3, 4, 4)],
                "another parcel",
            ),
            (vec![parcel("a", 18, 0, 4, 4)], "leaves the map"),
            (vec![parcel("a", 0, 6, 3, 3)], "entrance or depot"),
            (vec![parcel("Bad Id", 10, 0, 4, 4)], "lowercase"),
            (vec![parcel("a", 10, 0, 0, 4)], "leaves the map"),
        ];
        for (parcels, expected) in cases {
            let error = check(parcels).expect_err(expected);
            assert!(error.contains(expected), "{expected:?} not in {error:?}");
        }
        let mut free = parcel("a", 10, 0, 4, 4);
        free.price_cents = 0;
        assert!(check(vec![free]).unwrap_err().contains("price_cents"));
    }
}
