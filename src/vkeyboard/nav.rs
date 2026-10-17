//! Grid navigation, visual column tracking, and cursor clamping math.

use super::layout::{key_px_w, KDef, GAP};

pub fn wrap(i: i32, n: i32) -> i32 {
    ((i % n) + n) % n
}

pub fn key_x(row: &[KDef], col: usize) -> f32 {
    row[..col.min(row.len())].iter().map(|k| key_px_w(k.w) + GAP).sum()
}

pub fn key_center(row: &[KDef], col: usize) -> f32 {
    match row.get(col) {
        Some(k) => key_x(row, col) + key_px_w(k.w) * 0.5,
        None => 0.0,
    }
}

/// The column in `row` whose centre is closest to horizontal position `goal_x`.
pub fn nearest_col(row: &[KDef], goal_x: f32) -> usize {
    nearest_col_clamped(row, goal_x, 0, row.len())
}

/// The column in `row` between `min_col` and `max_col` whose centre is closest to `goal_x`.
pub fn nearest_col_clamped(row: &[KDef], goal_x: f32, min_col: usize, max_col: usize) -> usize {
    if row.is_empty() || min_col >= max_col {
        return min_col;
    }
    let end = max_col.min(row.len());
    let start = min_col.min(end);
    (start..end)
        .min_by(|&a, &b| {
            let da = (key_center(row, a) - goal_x).abs();
            let db = (key_center(row, b) - goal_x).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(min_col)
}
