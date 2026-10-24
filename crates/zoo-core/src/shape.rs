//! Habitat enclosure topology (#95).
//!
//! A habitat is the set of tiles one closed fence encloses. The fence runs along grid edges
//! between tile corners: corner `(x, y)` is the north-west corner of tile `(x, y)`, so a park
//! of `w × h` tiles has corners `0..=w × 0..=h`. A drawn outline is a list of corners; Rust
//! joins consecutive corners with unit grid edges (a diagonal drag becomes a staircase), then
//! requires one closed loop that never visits a corner twice. Such a loop encloses one
//! connected interior without holes or pinches, which this module derives by row parity, so
//! drawing direction and the starting corner never change the result.
//!
//! Rectangles are the same representation: [`rect_tiles`] is a convenience over it.
use super::{FenceSegment, FenceSide, Position};
use std::collections::HashSet;

/// The most corner pairs one outline gesture may send.
pub(crate) const MAX_OUTLINE_POINTS: usize = 512;

/// Why a drawn outline is not one closed, simple fence loop.
#[derive(Debug)]
pub(crate) struct OutlineError {
    pub message: String,
    /// The corner where the outline fails, for the preview.
    pub corner: Option<Position>,
}

impl OutlineError {
    fn new(message: impl Into<String>, corner: Option<Position>) -> Self {
        Self {
            message: message.into(),
            corner,
        }
    }
}

/// The tiles of the `width × height` rectangle at `(x, y)`, row-major.
pub(crate) fn rect_tiles(x: u32, y: u32, width: u32, height: u32) -> Vec<Position> {
    (y..y + height)
        .flat_map(|y| (x..x + width).map(move |x| Position { x, y }))
        .collect()
}

/// The interior of the closed outline through `points` (flat `x, y` corner pairs), row-major.
/// `max_edges` bounds the fence length so oversized input fails before any interior is built.
pub(crate) fn outline_tiles(
    points: &[u32],
    park_width: u32,
    park_height: u32,
    max_edges: usize,
) -> Result<Vec<Position>, OutlineError> {
    if points.is_empty() || !points.len().is_multiple_of(2) {
        return Err(OutlineError::new(
            "A fence outline needs whole x, y corner pairs",
            None,
        ));
    }
    if points.len() / 2 > MAX_OUTLINE_POINTS {
        return Err(OutlineError::new(
            format!("A fence outline may have at most {MAX_OUTLINE_POINTS} corners"),
            None,
        ));
    }
    let too_long = || OutlineError::new(format!("The fence may be at most {max_edges} long"), None);
    let corners: Vec<Position> = points
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| Position {
            x: pair[0],
            y: pair[1],
        })
        .collect();
    if let Some(outside) = corners
        .iter()
        .find(|corner| corner.x > park_width || corner.y > park_height)
    {
        return Err(OutlineError::new(
            "The fence would extend outside the park",
            Some(*outside),
        ));
    }

    // Walk the outline as unit edges; every corner but the closing one is new.
    let start = corners[0];
    let mut visited: HashSet<(u32, u32)> = HashSet::from([(start.x, start.y)]);
    // Vertical edges as (column, row): the edge from corner (x, y) to (x, y + 1).
    let mut vertical: Vec<(u32, u32)> = Vec::new();
    let mut edges = 0_usize;
    let mut here = start;
    let mut closed = false;
    for &to in &corners[1..] {
        if closed {
            if to == start {
                continue;
            }
            return Err(OutlineError::new(
                "The fence continues past its closed loop",
                Some(to),
            ));
        }
        let dx = i64::from(to.x) - i64::from(here.x);
        let dy = i64::from(to.y) - i64::from(here.y);
        let (span_x, span_y) = (dx.abs(), dy.abs());
        if edges as i64 + span_x + span_y > max_edges as i64 {
            return Err(too_long());
        }
        let (mut step_x, mut step_y) = (0, 0);
        while step_x < span_x || step_y < span_y {
            // Step along whichever axis lags the straight line between the corners.
            let next = if (1 + 2 * step_x) * span_y < (1 + 2 * step_y) * span_x {
                step_x += 1;
                Position {
                    x: (i64::from(here.x) + dx.signum()) as u32,
                    y: here.y,
                }
            } else {
                step_y += 1;
                let next = Position {
                    x: here.x,
                    y: (i64::from(here.y) + dy.signum()) as u32,
                };
                vertical.push((here.x, here.y.min(next.y)));
                next
            };
            edges += 1;
            if next == start {
                if step_x < span_x || step_y < span_y {
                    return Err(OutlineError::new(
                        "The fence crosses its starting corner",
                        Some(start),
                    ));
                }
                closed = true;
            } else if !visited.insert((next.x, next.y)) {
                return Err(OutlineError::new(
                    "The fence crosses or touches itself; draw one simple loop",
                    Some(next),
                ));
            }
            here = next;
        }
    }
    if !closed || edges < 4 {
        return Err(OutlineError::new(
            "Close the fence back at its starting corner",
            Some(here),
        ));
    }

    // Row parity: each row's vertical fence edges pair up into interior spans.
    vertical.sort_unstable_by_key(|&(x, y)| (y, x));
    let mut tiles = Vec::new();
    for row in vertical.chunk_by(|a, b| a.1 == b.1) {
        let y = row[0].1;
        for span in row.chunks(2) {
            let [(from, _), (to, _)] = span else {
                return Err(OutlineError::new("The fence outline is not closed", None));
            };
            tiles.extend((*from..*to).map(|x| Position { x, y }));
        }
    }
    Ok(tiles)
}

fn step(position: Position, dx: i64, dy: i64) -> Option<(u32, u32)> {
    let x = u32::try_from(i64::from(position.x) + dx).ok()?;
    let y = u32::try_from(i64::from(position.y) + dy).ok()?;
    Some((x, y))
}

/// The outward unit offset of a side.
fn outward(side: FenceSide) -> (i64, i64) {
    match side {
        FenceSide::North => (0, -1),
        FenceSide::East => (1, 0),
        FenceSide::South => (0, 1),
        FenceSide::West => (-1, 0),
    }
}

/// Walking a fence clockwise along `side`, the direction of travel.
fn travel(side: FenceSide) -> (i64, i64) {
    match side {
        FenceSide::North => (1, 0),
        FenceSide::East => (0, 1),
        FenceSide::South => (-1, 0),
        FenceSide::West => (0, -1),
    }
}

fn clockwise(side: FenceSide) -> FenceSide {
    match side {
        FenceSide::North => FenceSide::East,
        FenceSide::East => FenceSide::South,
        FenceSide::South => FenceSide::West,
        FenceSide::West => FenceSide::North,
    }
}

fn counter_clockwise(side: FenceSide) -> FenceSide {
    match side {
        FenceSide::North => FenceSide::West,
        FenceSide::East => FenceSide::North,
        FenceSide::South => FenceSide::East,
        FenceSide::West => FenceSide::South,
    }
}

const SIDES: [FenceSide; 4] = [
    FenceSide::North,
    FenceSide::East,
    FenceSide::South,
    FenceSide::West,
];

/// The membership set of an enclosure's tiles.
pub(crate) fn tile_set(tiles: &[Position]) -> HashSet<(u32, u32)> {
    tiles.iter().map(|tile| (tile.x, tile.y)).collect()
}

/// Whether the enclosure's neighbour across `side` of `tile` is outside it.
fn is_boundary(set: &HashSet<(u32, u32)>, tile: Position, side: FenceSide) -> bool {
    let (dx, dy) = outward(side);
    step(tile, dx, dy).is_none_or(|neighbour| !set.contains(&neighbour))
}

/// The enclosure's fence segments, clockwise from the north side of its first row-major tile
/// (the north-west corner of a rectangle). `None` when the tiles are not one simple enclosure.
pub(crate) fn clockwise_fence(tiles: &[Position]) -> Option<Vec<FenceSegment>> {
    let first = *tiles.first()?;
    let set = tile_set(tiles);
    let total = tiles
        .iter()
        .map(|tile| {
            SIDES
                .iter()
                .filter(|side| is_boundary(&set, *tile, **side))
                .count()
        })
        .sum::<usize>();
    let start = FenceSegment {
        x: first.x,
        y: first.y,
        side: FenceSide::North,
    };
    let mut fence = Vec::with_capacity(total);
    let mut segment = start;
    loop {
        fence.push(segment);
        if fence.len() > total {
            return None;
        }
        let tile = Position {
            x: segment.x,
            y: segment.y,
        };
        let (tx, ty) = travel(segment.side);
        let (ox, oy) = outward(segment.side);
        segment =
            if let Some((x, y)) = step(tile, tx + ox, ty + oy).filter(|tile| set.contains(tile)) {
                // The fence turns outward around a concave corner.
                FenceSegment {
                    x,
                    y,
                    side: counter_clockwise(segment.side),
                }
            } else if let Some((x, y)) = step(tile, tx, ty).filter(|tile| set.contains(tile)) {
                FenceSegment {
                    x,
                    y,
                    side: segment.side,
                }
            } else {
                FenceSegment {
                    side: clockwise(segment.side),
                    ..segment
                }
            };
        if segment == start {
            break;
        }
    }
    (fence.len() == total && !has_pinch(&set)).then_some(fence)
}

/// Two tiles touching only at a corner make the fence meet itself there.
fn has_pinch(set: &HashSet<(u32, u32)>) -> bool {
    set.iter().any(|&(x, y)| {
        let east = set.contains(&(x + 1, y));
        let south = set.contains(&(x, y + 1));
        let south_east = set.contains(&(x + 1, y + 1));
        let south_west = x > 0 && set.contains(&(x - 1, y + 1));
        let west = x > 0 && set.contains(&(x - 1, y));
        (south_east && !east && !south) || (south_west && !west && !south)
    })
}

/// Tiles whose eight neighbours all lie inside the enclosure, row-major; animals roam there
/// when the enclosure has any, so they keep clear of the fence.
pub(crate) fn inner_tiles(tiles: &[Position]) -> Vec<Position> {
    let set = tile_set(tiles);
    tiles
        .iter()
        .copied()
        .filter(|tile| {
            (-1..=1)
                .all(|dy| (-1..=1).all(|dx| step(*tile, dx, dy).is_some_and(|n| set.contains(&n))))
        })
        .collect()
}

/// The bounding box `(x, y, width, height)` of non-empty tiles.
pub(crate) fn bounds(tiles: &[Position]) -> (u32, u32, u32, u32) {
    let min_x = tiles.iter().map(|t| t.x).min().unwrap_or(0);
    let min_y = tiles.iter().map(|t| t.y).min().unwrap_or(0);
    let max_x = tiles.iter().map(|t| t.x).max().unwrap_or(0);
    let max_y = tiles.iter().map(|t| t.y).max().unwrap_or(0);
    (min_x, min_y, max_x - min_x + 1, max_y - min_y + 1)
}

/// Whether a tile lies in a row-major sorted enclosure.
pub(crate) fn contains(tiles: &[Position], tile: Position) -> bool {
    tiles
        .binary_search_by_key(&(tile.y, tile.x), |t| (t.y, t.x))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(tiles: &[Position]) -> Vec<(u32, u32)> {
        tiles.iter().map(|t| (t.x, t.y)).collect()
    }

    /// An L: 4×2 on top, then a 2×2 leg under its west half.
    const L_CLOCKWISE: [u32; 14] = [1, 1, 5, 1, 5, 3, 3, 3, 3, 5, 1, 5, 1, 1];

    fn l_tiles() -> Vec<(u32, u32)> {
        vec![
            (1, 1),
            (2, 1),
            (3, 1),
            (4, 1),
            (1, 2),
            (2, 2),
            (3, 2),
            (4, 2),
            (1, 3),
            (2, 3),
            (1, 4),
            (2, 4),
        ]
    }

    #[test]
    fn l_shapes_have_the_same_interior_in_any_direction_and_start() {
        let clockwise = outline_tiles(&L_CLOCKWISE, 20, 14, 200).unwrap();
        assert_eq!(flat(&clockwise), l_tiles());

        let mut reversed: Vec<u32> = L_CLOCKWISE
            .as_chunks::<2>()
            .0
            .iter()
            .rev()
            .flatten()
            .copied()
            .collect();
        assert_eq!(
            flat(&outline_tiles(&reversed, 20, 14, 200).unwrap()),
            l_tiles()
        );

        // Start the same loop at another corner.
        reversed = vec![5, 3, 3, 3, 3, 5, 1, 5, 1, 1, 5, 1, 5, 3];
        assert_eq!(
            flat(&outline_tiles(&reversed, 20, 14, 200).unwrap()),
            l_tiles()
        );

        let fence = clockwise_fence(&clockwise).unwrap();
        assert_eq!(fence.len(), 16, "perimeter of the L");
        assert_eq!(
            (fence[0].x, fence[0].y, fence[0].side),
            (1, 1, FenceSide::North)
        );
    }

    #[test]
    fn rotated_shapes_keep_area_and_perimeter() {
        // The L rotated a quarter turn: a 2×4 column with a 2×2 foot to its east.
        let rotated = [1, 1, 3, 1, 3, 3, 5, 3, 5, 5, 1, 5, 1, 1];
        let tiles = outline_tiles(&rotated, 20, 14, 200).unwrap();
        assert_eq!(tiles.len(), 12);
        assert_eq!(clockwise_fence(&tiles).unwrap().len(), 16);
    }

    #[test]
    fn rectangles_trace_their_perimeter_clockwise() {
        let tiles = rect_tiles(2, 3, 3, 2);
        let fence = clockwise_fence(&tiles).unwrap();
        assert_eq!(fence.len(), 10);
        let order: Vec<_> = fence.iter().map(|s| (s.x, s.y, s.side)).collect();
        assert_eq!(
            order[..4],
            [
                (2, 3, FenceSide::North),
                (3, 3, FenceSide::North),
                (4, 3, FenceSide::North),
                (4, 3, FenceSide::East),
            ]
        );
        assert_eq!(
            flat(&outline_tiles(&[2, 3, 5, 3, 5, 5, 2, 5, 2, 3], 20, 14, 200).unwrap()),
            flat(&tiles)
        );
    }

    #[test]
    fn diagonal_drags_become_staircases() {
        let tiles = outline_tiles(&[0, 0, 3, 0, 3, 3, 2, 3, 0, 1, 0, 0], 20, 14, 200).unwrap();
        // A staircase triangle under the diagonal: 3 + 2 + 1 tiles.
        assert_eq!(tiles.len(), 6);
        assert!(clockwise_fence(&tiles).is_some());
    }

    #[test]
    fn broken_outlines_reject_with_bounded_work() {
        let error = |points: &[u32]| outline_tiles(points, 20, 14, 202).unwrap_err().message;
        assert!(error(&[]).contains("pairs"));
        assert!(error(&[1, 1, 4]).contains("pairs"));
        // A gap: the loop never returns to its start.
        assert!(error(&[1, 1, 4, 1, 4, 4, 1, 4]).contains("Close"));
        // A figure eight crosses itself.
        assert!(error(&[0, 0, 2, 0, 2, 4, 4, 4, 4, 2, 0, 2, 0, 0]).contains("crosses"));
        // Two loops touching at one corner.
        assert!(error(&[0, 0, 2, 0, 2, 4, 4, 4, 4, 2, 2, 2, 2, 0]).contains("crosses"));
        // Retracing an edge.
        assert!(error(&[1, 1, 3, 1, 1, 1]).contains("crosses"));
        // Map edges.
        assert!(error(&[18, 10, 21, 10, 21, 12, 18, 12, 18, 10]).contains("outside"));
        // Huge inputs fail on length before building anything.
        assert!(error(&[0, 0, u32::MAX, 0, u32::MAX, 1, 0, 1, 0, 0]).contains("outside"));
        let huge = outline_tiles(&[0, 0, 4000, 0, 4000, 4000, 0, 4000, 0, 0], 5000, 5000, 202);
        assert!(huge.unwrap_err().message.contains("at most"));
        let many: Vec<u32> = (0..=MAX_OUTLINE_POINTS as u32)
            .flat_map(|i| [i % 2, 0])
            .collect();
        assert!(error(&many).contains("corners"));
        // Walking past a closed loop.
        assert!(error(&[1, 1, 3, 1, 3, 3, 1, 3, 1, 1, 1, 2]).contains("past"));
    }

    #[test]
    fn pinched_and_holed_masks_are_not_simple() {
        let pinch = vec![Position { x: 0, y: 0 }, Position { x: 1, y: 1 }];
        assert!(clockwise_fence(&pinch).is_none());
        let mut ring = rect_tiles(0, 0, 3, 3);
        ring.retain(|t| (t.x, t.y) != (1, 1));
        assert!(clockwise_fence(&ring).is_none());
    }

    #[test]
    fn inner_tiles_keep_clear_of_the_fence() {
        assert_eq!(flat(&inner_tiles(&rect_tiles(0, 0, 3, 3))), vec![(1, 1)]);
        let l = outline_tiles(&L_CLOCKWISE, 20, 14, 200).unwrap();
        assert!(inner_tiles(&l).is_empty());
        assert!(contains(&l, Position { x: 2, y: 4 }));
        assert!(!contains(&l, Position { x: 3, y: 4 }));
        assert_eq!(bounds(&l), (1, 1, 4, 4));
    }
}
