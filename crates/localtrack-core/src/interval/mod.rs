//! Central interval engine (spec §119).
//!
//! Every duration in LocalTrack is computed here. Interval arithmetic is never
//! duplicated in React components or in SQL.
//!
//! Intervals are half-open: `[start_ms, end_ms)`. That makes adjacent intervals
//! join without overlap and prevents a single millisecond being counted twice.

use serde::{Deserialize, Serialize};

/// A half-open time interval `[start_ms, end_ms)` in UTC epoch milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interval {
    pub start_ms: i64,
    pub end_ms: i64,
}

impl Interval {
    /// Create an interval, normalizing inverted bounds to an empty interval.
    pub fn new(start_ms: i64, end_ms: i64) -> Self {
        if end_ms < start_ms {
            Self {
                start_ms,
                end_ms: start_ms,
            }
        } else {
            Self { start_ms, end_ms }
        }
    }

    pub fn duration_ms(&self) -> i64 {
        (self.end_ms - self.start_ms).max(0)
    }

    pub fn is_empty(&self) -> bool {
        self.end_ms <= self.start_ms
    }

    pub fn contains(&self, ts_ms: i64) -> bool {
        ts_ms >= self.start_ms && ts_ms < self.end_ms
    }

    pub fn overlaps(&self, other: &Interval) -> bool {
        self.start_ms < other.end_ms && other.start_ms < self.end_ms
    }

    /// Intersection with another interval, or `None` when disjoint.
    pub fn intersect(&self, other: &Interval) -> Option<Interval> {
        let start = self.start_ms.max(other.start_ms);
        let end = self.end_ms.min(other.end_ms);
        if end > start {
            Some(Interval {
                start_ms: start,
                end_ms: end,
            })
        } else {
            None
        }
    }

    /// Clip this interval into a bounding window.
    pub fn clip(&self, window: &Interval) -> Option<Interval> {
        self.intersect(window)
    }

    /// Remove `other` from this interval, yielding 0, 1 or 2 pieces.
    pub fn subtract(&self, other: &Interval) -> Vec<Interval> {
        if !self.overlaps(other) {
            return if self.is_empty() { vec![] } else { vec![*self] };
        }
        let mut out = Vec::with_capacity(2);
        if other.start_ms > self.start_ms {
            out.push(Interval {
                start_ms: self.start_ms,
                end_ms: other.start_ms,
            });
        }
        if other.end_ms < self.end_ms {
            out.push(Interval {
                start_ms: other.end_ms,
                end_ms: self.end_ms,
            });
        }
        out
    }
}

/// A normalized set of disjoint, sorted, non-empty intervals.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntervalSet {
    intervals: Vec<Interval>,
}

impl IntervalSet {
    pub fn empty() -> Self {
        Self {
            intervals: Vec::new(),
        }
    }

    /// Build a normalized set: empties dropped, sorted, touching/overlapping merged.
    pub fn from_intervals<I: IntoIterator<Item = Interval>>(iter: I) -> Self {
        let mut items: Vec<Interval> = iter.into_iter().filter(|i| !i.is_empty()).collect();
        items.sort_by_key(|i| (i.start_ms, i.end_ms));
        let mut merged: Vec<Interval> = Vec::with_capacity(items.len());
        for item in items {
            match merged.last_mut() {
                Some(last) if item.start_ms <= last.end_ms => {
                    last.end_ms = last.end_ms.max(item.end_ms);
                }
                _ => merged.push(item),
            }
        }
        Self { intervals: merged }
    }

    pub fn single(interval: Interval) -> Self {
        Self::from_intervals([interval])
    }

    pub fn as_slice(&self) -> &[Interval] {
        &self.intervals
    }

    pub fn into_vec(self) -> Vec<Interval> {
        self.intervals
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Interval> {
        self.intervals.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.intervals.is_empty()
    }

    pub fn len(&self) -> usize {
        self.intervals.len()
    }

    /// Total covered duration in milliseconds.
    pub fn duration_ms(&self) -> i64 {
        self.intervals.iter().map(|i| i.duration_ms()).sum()
    }

    pub fn union(&self, other: &IntervalSet) -> IntervalSet {
        IntervalSet::from_intervals(
            self.intervals
                .iter()
                .copied()
                .chain(other.intervals.iter().copied()),
        )
    }

    pub fn intersection(&self, other: &IntervalSet) -> IntervalSet {
        let mut out = Vec::new();
        let (mut i, mut j) = (0usize, 0usize);
        while i < self.intervals.len() && j < other.intervals.len() {
            let a = self.intervals[i];
            let b = other.intervals[j];
            if let Some(hit) = a.intersect(&b) {
                out.push(hit);
            }
            if a.end_ms < b.end_ms {
                i += 1;
            } else {
                j += 1;
            }
        }
        IntervalSet { intervals: out }
    }

    pub fn subtract(&self, other: &IntervalSet) -> IntervalSet {
        let mut out: Vec<Interval> = Vec::new();
        for base in &self.intervals {
            out.extend(self_subtract_one(*base, &other.intervals));
        }
        IntervalSet { intervals: out }
    }

    /// Clip a single interval against this set, returning the covered pieces.
    ///
    /// Both sides are sorted, so the relevant cuts are found by binary search
    /// instead of rescanning the whole set for every interval — that keeps
    /// month-scale reports linear rather than quadratic.
    pub fn clip_interval(&self, interval: Interval) -> Vec<Interval> {
        if interval.is_empty() {
            return Vec::new();
        }
        let start = self
            .intervals
            .partition_point(|candidate| candidate.end_ms <= interval.start_ms);
        let mut out = Vec::new();
        for candidate in &self.intervals[start..] {
            if candidate.start_ms >= interval.end_ms {
                break;
            }
            if let Some(hit) = candidate.intersect(&interval) {
                out.push(hit);
            }
        }
        out
    }

    /// Remove a single interval's covered parts, returning what is left of it.
    pub fn subtract_interval(&self, interval: Interval) -> Vec<Interval> {
        self_subtract_one(interval, &self.intervals)
    }

    /// Clip the whole set into a window.
    pub fn clip(&self, window: &Interval) -> IntervalSet {
        self.intersection(&IntervalSet::single(*window))
    }

    pub fn contains_point(&self, ts_ms: i64) -> bool {
        self.intervals.iter().any(|i| i.contains(ts_ms))
    }

    /// Gaps between covered intervals inside `window`.
    pub fn gaps_within(&self, window: &Interval) -> IntervalSet {
        IntervalSet::single(*window).subtract(self)
    }

    /// Merge intervals separated by no more than `max_gap_ms`.
    pub fn merge_with_tolerance(&self, max_gap_ms: i64) -> IntervalSet {
        let mut out: Vec<Interval> = Vec::new();
        for item in &self.intervals {
            match out.last_mut() {
                Some(last) if item.start_ms - last.end_ms <= max_gap_ms => {
                    last.end_ms = last.end_ms.max(item.end_ms);
                }
                _ => out.push(*item),
            }
        }
        IntervalSet { intervals: out }
    }
}

impl FromIterator<Interval> for IntervalSet {
    fn from_iter<T: IntoIterator<Item = Interval>>(iter: T) -> Self {
        IntervalSet::from_intervals(iter)
    }
}

/// An interval carrying a payload, used for overlay computations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayItem<T> {
    pub interval: Interval,
    pub payload: T,
}

impl<T> OverlayItem<T> {
    pub fn new(interval: Interval, payload: T) -> Self {
        Self { interval, payload }
    }
}

/// Overlay layers by priority (spec §117).
///
/// `layers[0]` has the highest priority; each lower layer is only visible where
/// no higher layer covers the time. Within a single layer, the earlier-starting
/// item wins so the result never double-counts a millisecond.
pub fn overlay_by_priority<T: Clone>(layers: Vec<Vec<OverlayItem<T>>>) -> Vec<OverlayItem<T>> {
    let mut covered = IntervalSet::empty();
    let mut out: Vec<OverlayItem<T>> = Vec::new();

    for layer in layers {
        let mut items = layer;
        items.sort_by_key(|item| (item.interval.start_ms, item.interval.end_ms));

        // Within a layer the earlier-starting item wins: a single sweep with a
        // running cursor resolves that in one pass.
        let mut cursor = i64::MIN;
        let mut layer_pieces: Vec<Interval> = Vec::with_capacity(items.len());
        for item in items {
            if item.interval.is_empty() {
                continue;
            }
            let start = item.interval.start_ms.max(cursor);
            if start >= item.interval.end_ms {
                continue;
            }
            cursor = item.interval.end_ms;
            let visible = Interval {
                start_ms: start,
                end_ms: item.interval.end_ms,
            };

            // Higher-priority layers win: remove what they already cover.
            for piece in covered.subtract_interval(visible) {
                out.push(OverlayItem {
                    interval: piece,
                    payload: item.payload.clone(),
                });
                layer_pieces.push(piece);
            }
        }

        if !layer_pieces.is_empty() {
            covered = covered.union(&IntervalSet::from_intervals(layer_pieces));
        }
    }

    out.sort_by_key(|item| (item.interval.start_ms, item.interval.end_ms));
    out
}

/// Subtract a sorted, disjoint list of cuts from one interval.
fn self_subtract_one(base: Interval, cuts: &[Interval]) -> Vec<Interval> {
    if base.is_empty() {
        return Vec::new();
    }
    let start = cuts.partition_point(|cut| cut.end_ms <= base.start_ms);
    let mut out = Vec::new();
    let mut cursor = base.start_ms;
    for cut in &cuts[start..] {
        if cut.start_ms >= base.end_ms {
            break;
        }
        if cut.start_ms > cursor {
            out.push(Interval {
                start_ms: cursor,
                end_ms: cut.start_ms.min(base.end_ms),
            });
        }
        cursor = cursor.max(cut.end_ms);
        if cursor >= base.end_ms {
            return out;
        }
    }
    if cursor < base.end_ms {
        out.push(Interval {
            start_ms: cursor,
            end_ms: base.end_ms,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iv(a: i64, b: i64) -> Interval {
        Interval::new(a, b)
    }

    #[test]
    fn duration_and_emptiness() {
        assert_eq!(iv(0, 10).duration_ms(), 10);
        assert!(iv(10, 10).is_empty());
        assert!(iv(10, 5).is_empty());
        assert_eq!(iv(10, 5).duration_ms(), 0);
    }

    #[test]
    fn half_open_containment() {
        let i = iv(0, 10);
        assert!(i.contains(0));
        assert!(i.contains(9));
        assert!(!i.contains(10));
    }

    #[test]
    fn intersection_of_intervals() {
        assert_eq!(iv(0, 10).intersect(&iv(5, 20)), Some(iv(5, 10)));
        assert_eq!(iv(0, 10).intersect(&iv(10, 20)), None);
        assert_eq!(iv(0, 10).intersect(&iv(20, 30)), None);
    }

    #[test]
    fn subtract_splits_into_two() {
        assert_eq!(
            iv(0, 100).subtract(&iv(40, 60)),
            vec![iv(0, 40), iv(60, 100)]
        );
        assert_eq!(iv(0, 100).subtract(&iv(0, 100)), vec![]);
        assert_eq!(iv(0, 100).subtract(&iv(100, 200)), vec![iv(0, 100)]);
    }

    #[test]
    fn set_normalizes_overlaps_and_touching() {
        let set = IntervalSet::from_intervals([iv(30, 40), iv(0, 10), iv(5, 20), iv(20, 25)]);
        assert_eq!(set.as_slice(), &[iv(0, 25), iv(30, 40)]);
        assert_eq!(set.duration_ms(), 35);
    }

    #[test]
    fn set_union_intersection_subtract() {
        let a = IntervalSet::from_intervals([iv(0, 50), iv(60, 100)]);
        let b = IntervalSet::from_intervals([iv(40, 70)]);
        assert_eq!(a.union(&b).as_slice(), &[iv(0, 100)]);
        assert_eq!(a.intersection(&b).as_slice(), &[iv(40, 50), iv(60, 70)]);
        assert_eq!(a.subtract(&b).as_slice(), &[iv(0, 40), iv(70, 100)]);
        assert_eq!(a.subtract(&b).duration_ms(), 70);
    }

    #[test]
    fn gaps_are_the_untracked_time() {
        let covered = IntervalSet::from_intervals([iv(0, 10), iv(30, 40)]);
        let gaps = covered.gaps_within(&iv(0, 50));
        assert_eq!(gaps.as_slice(), &[iv(10, 30), iv(40, 50)]);
        assert_eq!(gaps.duration_ms(), 30);
    }

    #[test]
    fn merge_with_tolerance_joins_small_gaps() {
        let set = IntervalSet::from_intervals([iv(0, 10), iv(15, 20), iv(60, 70)]);
        let merged = set.merge_with_tolerance(5);
        assert_eq!(merged.as_slice(), &[iv(0, 20), iv(60, 70)]);
    }

    #[test]
    fn overlay_gives_priority_to_first_layer() {
        // Browser detail (higher) over desktop Chrome (lower), spec §42.
        let desktop = vec![OverlayItem::new(iv(0, 30), "Chrome")];
        let browser = vec![
            OverlayItem::new(iv(0, 12), "github.com"),
            OverlayItem::new(iv(12, 20), "chatgpt.com"),
        ];
        let out = overlay_by_priority(vec![browser, desktop]);
        assert_eq!(
            out,
            vec![
                OverlayItem::new(iv(0, 12), "github.com"),
                OverlayItem::new(iv(12, 20), "chatgpt.com"),
                OverlayItem::new(iv(20, 30), "Chrome"),
            ]
        );
        // No double counting: total covered time is still 30.
        let total: i64 = out.iter().map(|o| o.interval.duration_ms()).sum();
        assert_eq!(total, 30);
    }

    #[test]
    fn overlay_within_layer_prefers_earlier_start() {
        let layer = vec![
            OverlayItem::new(iv(0, 20), "a"),
            OverlayItem::new(iv(10, 30), "b"),
        ];
        let out = overlay_by_priority(vec![layer]);
        assert_eq!(
            out,
            vec![
                OverlayItem::new(iv(0, 20), "a"),
                OverlayItem::new(iv(20, 30), "b")
            ]
        );
    }

    #[test]
    fn overlay_idle_beats_everything() {
        let idle = vec![OverlayItem::new(iv(10, 20), "idle")];
        let browser = vec![OverlayItem::new(iv(0, 30), "github.com")];
        let out = overlay_by_priority(vec![idle, browser]);
        assert_eq!(
            out,
            vec![
                OverlayItem::new(iv(0, 10), "github.com"),
                OverlayItem::new(iv(10, 20), "idle"),
                OverlayItem::new(iv(20, 30), "github.com"),
            ]
        );
    }
}
