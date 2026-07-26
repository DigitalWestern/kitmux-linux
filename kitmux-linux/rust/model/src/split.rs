use crate::{PaneId, SplitId};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SplitAxis {
    #[serde(rename = "leftRight")]
    LeftRight,
    #[serde(rename = "topBottom")]
    TopBottom,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Split {
    pub id: SplitId,
    pub axis: SplitAxis,
    pub ratio: f64,
    pub first: Box<SplitNode>,
    pub second: Box<SplitNode>,
}

impl Split {
    #[must_use]
    pub fn new(axis: SplitAxis, ratio: f64, first: SplitNode, second: SplitNode) -> Self {
        Self::with_id(SplitId::new(), axis, ratio, first, second)
    }

    #[must_use]
    pub fn with_id(
        id: SplitId,
        axis: SplitAxis,
        ratio: f64,
        first: SplitNode,
        second: SplitNode,
    ) -> Self {
        Self {
            id,
            axis,
            ratio: finite_ratio_or_default(ratio),
            first: Box::new(first),
            second: Box::new(second),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SplitNode {
    Pane(PaneId),
    Split(Split),
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum SplitNodeWire {
    Pane {
        #[serde(rename = "_0")]
        value: PaneId,
    },
    Split {
        #[serde(rename = "_0")]
        value: Split,
    },
}

impl Serialize for SplitNode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Pane(value) => SplitNodeWire::Pane { value: *value }.serialize(serializer),
            Self::Split(value) => SplitNodeWire::Split {
                value: value.clone(),
            }
            .serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for SplitNode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match SplitNodeWire::deserialize(deserializer)? {
            SplitNodeWire::Pane { value } => Self::Pane(value),
            SplitNodeWire::Split { value } => Self::Split(value),
        })
    }
}

impl SplitNode {
    #[must_use]
    pub const fn pane(id: PaneId) -> Self {
        Self::Pane(id)
    }

    #[must_use]
    pub fn pane_ids(&self) -> Vec<PaneId> {
        let mut result = Vec::new();
        self.collect_pane_ids(&mut result);
        result
    }

    fn collect_pane_ids(&self, result: &mut Vec<PaneId>) {
        match self {
            Self::Pane(id) => result.push(*id),
            Self::Split(split) => {
                split.first.collect_pane_ids(result);
                split.second.collect_pane_ids(result);
            }
        }
    }

    #[must_use]
    pub fn contains(&self, pane_id: PaneId) -> bool {
        match self {
            Self::Pane(id) => *id == pane_id,
            Self::Split(split) => split.first.contains(pane_id) || split.second.contains(pane_id),
        }
    }

    #[must_use]
    pub fn has_unique_ids_and_valid_ratios(&self) -> bool {
        fn visit(
            node: &SplitNode,
            pane_ids: &mut HashSet<PaneId>,
            split_ids: &mut HashSet<SplitId>,
        ) -> bool {
            match node {
                SplitNode::Pane(id) => pane_ids.insert(*id),
                SplitNode::Split(split) => {
                    split.ratio.is_finite()
                        && (0.0..=1.0).contains(&split.ratio)
                        && split_ids.insert(split.id)
                        && visit(&split.first, pane_ids, split_ids)
                        && visit(&split.second, pane_ids, split_ids)
                }
            }
        }

        visit(self, &mut HashSet::new(), &mut HashSet::new())
    }

    pub fn split_pane(&mut self, pane_id: PaneId, axis: SplitAxis, new_pane_id: PaneId) -> bool {
        if self.contains(new_pane_id) {
            return false;
        }
        self.split_pane_unchecked(pane_id, axis, new_pane_id)
    }

    fn split_pane_unchecked(
        &mut self,
        pane_id: PaneId,
        axis: SplitAxis,
        new_pane_id: PaneId,
    ) -> bool {
        match self {
            Self::Pane(id) if *id == pane_id => {
                *self = Self::Split(Split::new(
                    axis,
                    0.5,
                    Self::Pane(*id),
                    Self::Pane(new_pane_id),
                ));
                true
            }
            Self::Pane(_) => false,
            Self::Split(split) => {
                split.first.split_pane_unchecked(pane_id, axis, new_pane_id)
                    || split
                        .second
                        .split_pane_unchecked(pane_id, axis, new_pane_id)
            }
        }
    }

    #[must_use]
    pub fn removing_pane(&self, pane_id: PaneId) -> Option<Self> {
        let (node, removed) = self.removing(pane_id);
        if removed { node } else { Some(self.clone()) }
    }

    fn removing(&self, pane_id: PaneId) -> (Option<Self>, bool) {
        match self {
            Self::Pane(id) if *id == pane_id => (None, true),
            Self::Pane(_) => (Some(self.clone()), false),
            Self::Split(split) => {
                let (first, removed) = split.first.removing(pane_id);
                if removed {
                    return match first {
                        Some(first) => {
                            let mut updated = split.clone();
                            updated.first = Box::new(first);
                            (Some(Self::Split(updated)), true)
                        }
                        None => (Some((*split.second).clone()), true),
                    };
                }

                let (second, removed) = split.second.removing(pane_id);
                if removed {
                    return match second {
                        Some(second) => {
                            let mut updated = split.clone();
                            updated.second = Box::new(second);
                            (Some(Self::Split(updated)), true)
                        }
                        None => (Some((*split.first).clone()), true),
                    };
                }
                (Some(self.clone()), false)
            }
        }
    }

    pub fn swap_panes(&mut self, first: PaneId, second: PaneId) -> bool {
        if first == second || !self.contains(first) || !self.contains(second) {
            return false;
        }
        self.swap_panes_unchecked(first, second);
        true
    }

    fn swap_panes_unchecked(&mut self, first: PaneId, second: PaneId) {
        match self {
            Self::Pane(id) if *id == first => *id = second,
            Self::Pane(id) if *id == second => *id = first,
            Self::Pane(_) => {}
            Self::Split(split) => {
                split.first.swap_panes_unchecked(first, second);
                split.second.swap_panes_unchecked(first, second);
            }
        }
    }

    #[must_use]
    pub fn split(&self, split_id: SplitId) -> Option<&Split> {
        match self {
            Self::Pane(_) => None,
            Self::Split(split) if split.id == split_id => Some(split),
            Self::Split(split) => split
                .first
                .split(split_id)
                .or_else(|| split.second.split(split_id)),
        }
    }

    pub fn adjust_ratio(&mut self, split_id: SplitId, delta: f64, bounds: (f64, f64)) -> bool {
        let Some(split) = self.split_mut(split_id) else {
            return false;
        };
        let candidate = if delta.is_finite() {
            split.ratio + delta
        } else {
            split.ratio
        };
        split.ratio = clamp_to_bounds(candidate, bounds);
        true
    }

    pub fn set_ratio(&mut self, split_id: SplitId, ratio: f64, bounds: (f64, f64)) -> bool {
        let Some(split) = self.split_mut(split_id) else {
            return false;
        };
        if ratio.is_finite() {
            split.ratio = clamp_to_bounds(ratio, bounds);
        }
        true
    }

    fn split_mut(&mut self, split_id: SplitId) -> Option<&mut Split> {
        match self {
            Self::Pane(_) => None,
            Self::Split(split) => {
                if split.id == split_id {
                    return Some(split);
                }
                if let Some(found) = split.first.split_mut(split_id) {
                    return Some(found);
                }
                split.second.split_mut(split_id)
            }
        }
    }

    #[must_use]
    pub fn resize_target(&self, pane_id: PaneId, direction: Direction) -> Option<ResizeTarget> {
        match self {
            Self::Pane(_) => None,
            Self::Split(split) => {
                let in_first = split.first.contains(pane_id);
                if !in_first && !split.second.contains(pane_id) {
                    return None;
                }
                let child = if in_first {
                    &split.first
                } else {
                    &split.second
                };
                if let Some(target) = child.resize_target(pane_id, direction) {
                    return Some(target);
                }
                let ratio_delta_sign = match (split.axis, direction, in_first) {
                    (SplitAxis::LeftRight, Direction::Right, true)
                    | (SplitAxis::TopBottom, Direction::Down, true) => 1,
                    (SplitAxis::LeftRight, Direction::Left, false)
                    | (SplitAxis::TopBottom, Direction::Up, false) => -1,
                    _ => return None,
                };
                Some(ResizeTarget {
                    split_id: split.id,
                    ratio_delta_sign,
                })
            }
        }
    }

    #[must_use]
    pub fn minimum_size(&self, gap: i32, leaf: PixelSize) -> PixelSize {
        match self {
            Self::Pane(_) => leaf,
            Self::Split(split) => {
                let first = split.first.minimum_size(gap, leaf);
                let second = split.second.minimum_size(gap, leaf);
                match split.axis {
                    SplitAxis::LeftRight => PixelSize::new(
                        first.width + gap.max(0) + second.width,
                        first.height.max(second.height),
                    ),
                    SplitAxis::TopBottom => PixelSize::new(
                        first.width.max(second.width),
                        first.height + gap.max(0) + second.height,
                    ),
                }
            }
        }
    }

    #[must_use]
    pub fn layout(&self, rect: PixelRect, gap: i32, minimum_leaf_size: PixelSize) -> SplitLayout {
        let mut result = SplitLayout::default();
        self.fill_layout(rect, gap.max(0), minimum_leaf_size, &mut result);
        result
    }

    fn fill_layout(
        &self,
        rect: PixelRect,
        gap: i32,
        minimum_leaf_size: PixelSize,
        result: &mut SplitLayout,
    ) {
        match self {
            Self::Pane(id) => {
                result.pane_frames.insert(*id, rect);
            }
            Self::Split(split) => {
                result.split_frames.insert(split.id, rect);
                let along = match split.axis {
                    SplitAxis::LeftRight => rect.width,
                    SplitAxis::TopBottom => rect.height,
                };
                let actual_gap = gap.min(along);
                let usable = (along - actual_gap).max(0);
                let first_minimum = split.first.minimum_size(gap, minimum_leaf_size);
                let second_minimum = split.second.minimum_size(gap, minimum_leaf_size);
                let first_min_along = match split.axis {
                    SplitAxis::LeftRight => first_minimum.width,
                    SplitAxis::TopBottom => first_minimum.height,
                };
                let second_min_along = match split.axis {
                    SplitAxis::LeftRight => second_minimum.width,
                    SplitAxis::TopBottom => second_minimum.height,
                };
                let desired = (f64::from(usable) * split.ratio).round() as i32;
                let first_length = if usable >= first_min_along + second_min_along {
                    desired.clamp(first_min_along, usable - second_min_along)
                } else if usable >= 2 {
                    let minimum_total = (first_min_along + second_min_along).max(1);
                    ((f64::from(usable) * f64::from(first_min_along) / f64::from(minimum_total))
                        .round() as i32)
                        .clamp(1, usable - 1)
                } else {
                    usable
                };
                let second_length = usable - first_length;

                let (first_rect, divider_rect, second_rect) = match split.axis {
                    SplitAxis::LeftRight => (
                        PixelRect::new(rect.x, rect.y, first_length, rect.height),
                        PixelRect::new(rect.x + first_length, rect.y, actual_gap, rect.height),
                        PixelRect::new(
                            rect.x + first_length + actual_gap,
                            rect.y,
                            second_length,
                            rect.height,
                        ),
                    ),
                    SplitAxis::TopBottom => (
                        PixelRect::new(rect.x, rect.y, rect.width, first_length),
                        PixelRect::new(rect.x, rect.y + first_length, rect.width, actual_gap),
                        PixelRect::new(
                            rect.x,
                            rect.y + first_length + actual_gap,
                            rect.width,
                            second_length,
                        ),
                    ),
                };
                result.divider_frames.insert(split.id, divider_rect);
                split
                    .first
                    .fill_layout(first_rect, gap, minimum_leaf_size, result);
                split
                    .second
                    .fill_layout(second_rect, gap, minimum_leaf_size, result);
            }
        }
    }

    #[must_use]
    pub fn ratio_bounds(
        &self,
        split_id: SplitId,
        rect: PixelRect,
        gap: i32,
        minimum_leaf_size: PixelSize,
    ) -> Option<(f64, f64)> {
        let split = self.split(split_id)?;
        let along = match split.axis {
            SplitAxis::LeftRight => rect.width,
            SplitAxis::TopBottom => rect.height,
        };
        let usable = along - gap.max(0);
        if usable <= 0 {
            return None;
        }
        let first = split.first.minimum_size(gap, minimum_leaf_size);
        let second = split.second.minimum_size(gap, minimum_leaf_size);
        let first_along = match split.axis {
            SplitAxis::LeftRight => first.width,
            SplitAxis::TopBottom => first.height,
        };
        let second_along = match split.axis {
            SplitAxis::LeftRight => second.width,
            SplitAxis::TopBottom => second.height,
        };
        let lower = 0.1_f64.max(f64::from(first_along) / f64::from(usable));
        let upper = 0.9_f64.min(1.0 - f64::from(second_along) / f64::from(usable));
        (lower <= upper).then_some((lower, upper))
    }
}

fn finite_ratio_or_default(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.5
    }
}

fn clamp_to_bounds(value: f64, bounds: (f64, f64)) -> f64 {
    let lower = bounds.0.clamp(0.0, 1.0);
    let upper = bounds.1.clamp(lower, 1.0);
    value.clamp(lower, upper)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResizeTarget {
    pub split_id: SplitId,
    pub ratio_delta_sign: i8,
}

impl ResizeTarget {
    #[must_use]
    pub fn ratio_delta(self, step: f64) -> f64 {
        f64::from(self.ratio_delta_sign) * step
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelSize {
    pub width: i32,
    pub height: i32,
}

impl PixelSize {
    #[must_use]
    pub const fn new(width: i32, height: i32) -> Self {
        Self {
            width: if width < 0 { 0 } else { width },
            height: if height < 0 { 0 } else { height },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl PixelRect {
    #[must_use]
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width: if width < 0 { 0 } else { width },
            height: if height < 0 { 0 } else { height },
        }
    }

    const fn min_x(self) -> i32 {
        self.x
    }

    const fn max_x(self) -> i32 {
        self.x + self.width
    }

    const fn min_y(self) -> i32 {
        self.y
    }

    const fn max_y(self) -> i32 {
        self.y + self.height
    }

    const fn center_x2(self) -> i32 {
        self.x * 2 + self.width
    }

    const fn center_y2(self) -> i32 {
        self.y * 2 + self.height
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SplitLayout {
    pub pane_frames: HashMap<PaneId, PixelRect>,
    pub split_frames: HashMap<SplitId, PixelRect>,
    pub divider_frames: HashMap<SplitId, PixelRect>,
}

#[must_use]
pub fn directional_neighbor(
    pane_id: PaneId,
    direction: Direction,
    frames: &HashMap<PaneId, PixelRect>,
    ordered_pane_ids: &[PaneId],
) -> Option<PaneId> {
    let source = *frames.get(&pane_id)?;
    let order: HashMap<PaneId, usize> = ordered_pane_ids
        .iter()
        .copied()
        .enumerate()
        .map(|(index, id)| (id, index))
        .collect();
    let mut candidates: Vec<(PaneId, PixelRect)> = frames
        .iter()
        .filter_map(|(id, rect)| {
            let is_in_direction = match direction {
                Direction::Left => rect.center_x2() < source.center_x2(),
                Direction::Right => rect.center_x2() > source.center_x2(),
                Direction::Up => rect.center_y2() < source.center_y2(),
                Direction::Down => rect.center_y2() > source.center_y2(),
            };
            (*id != pane_id && is_in_direction).then_some((*id, *rect))
        })
        .collect();
    if candidates.is_empty() {
        return None;
    }

    let overlaps = |rect: PixelRect| match direction {
        Direction::Left | Direction::Right => {
            rect.min_y().max(source.min_y()) < rect.max_y().min(source.max_y())
        }
        Direction::Up | Direction::Down => {
            rect.min_x().max(source.min_x()) < rect.max_x().min(source.max_x())
        }
    };
    if candidates.iter().any(|(_, rect)| overlaps(*rect)) {
        candidates.retain(|(_, rect)| overlaps(*rect));
    }
    candidates
        .into_iter()
        .min_by_key(|(id, rect)| {
            let (primary, perpendicular) = match direction {
                Direction::Left => (
                    (source.min_x() - rect.max_x()).max(0),
                    (source.center_y2() - rect.center_y2()).abs(),
                ),
                Direction::Right => (
                    (rect.min_x() - source.max_x()).max(0),
                    (source.center_y2() - rect.center_y2()).abs(),
                ),
                Direction::Up => (
                    (source.min_y() - rect.max_y()).max(0),
                    (source.center_x2() - rect.center_x2()).abs(),
                ),
                Direction::Down => (
                    (rect.min_y() - source.max_y()).max(0),
                    (source.center_x2() - rect.center_x2()).abs(),
                ),
            };
            (
                primary,
                perpendicular,
                order.get(id).copied().unwrap_or(usize::MAX),
            )
        })
        .map(|(id, _)| id)
}
