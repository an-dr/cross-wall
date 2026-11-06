//! Justified rows: photos keep their shape, and each full row fills the width.
const MIN_ASPECT: f32 = 0.5;
const MAX_ASPECT: f32 = 2.6;
/// For a source that reports no size.
const DEFAULT_ASPECT: f32 = 1.6;

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub height: f32,
    /// Photo index and width, left to right.
    pub tiles: Vec<(usize, f32)>,
}

pub fn aspect(width: u32, height: u32) -> f32 {
    if width == 0 || height == 0 {
        return DEFAULT_ASPECT;
    }
    (width as f32 / height as f32).clamp(MIN_ASPECT, MAX_ASPECT)
}

/// Fills rows at `target` height, then scales each full row to fit `available`.
/// The last row stays at `target` height, so one photo cannot grow to fill it.
pub fn rows(aspects: &[f32], available: f32, target: f32, gap: f32) -> Vec<Row> {
    let mut result = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut sum = 0.0;
    for (index, aspect) in aspects.iter().enumerate() {
        current.push(index);
        sum += aspect;
        let gaps = gap * (current.len() - 1) as f32;
        if sum * target + gaps >= available {
            let height = (available - gaps) / sum;
            result.push(row(&current, aspects, height));
            current.clear();
            sum = 0.0;
        }
    }
    if !current.is_empty() {
        result.push(row(&current, aspects, target));
    }
    result
}

fn row(indices: &[usize], aspects: &[f32], height: f32) -> Row {
    Row { height, tiles: indices.iter().map(|&i| (i, aspects[i] * height)).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_rows_fill_the_width_exactly() {
        let aspects = [1.5, 1.0, 2.0, 0.7, 1.6, 1.6, 1.6];
        let laid_out = rows(&aspects, 1000.0, 200.0, 10.0);
        for row in &laid_out[..laid_out.len() - 1] {
            let used: f32 = row.tiles.iter().map(|t| t.1).sum::<f32>() + 10.0 * (row.tiles.len() - 1) as f32;
            assert!((used - 1000.0).abs() < 0.01, "{used}");
        }
        let order: Vec<usize> = laid_out.iter().flat_map(|r| r.tiles.iter().map(|t| t.0)).collect();
        assert_eq!(order, [0, 1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn a_short_last_row_is_not_stretched() {
        let laid_out = rows(&[1.0], 1000.0, 200.0, 10.0);
        assert_eq!(laid_out[0].height, 200.0);
        assert_eq!(laid_out[0].tiles[0].1, 200.0);
    }

    #[test]
    fn extreme_or_unknown_shapes_are_bounded() {
        assert_eq!(aspect(10000, 100), MAX_ASPECT);
        assert_eq!(aspect(100, 10000), MIN_ASPECT);
        assert_eq!(aspect(0, 0), DEFAULT_ASPECT);
    }

    #[test]
    fn a_narrow_window_still_lays_out() {
        let laid_out = rows(&[1.6, 1.6], 100.0, 200.0, 10.0);
        assert_eq!(laid_out.len(), 2);
        assert!((laid_out[0].tiles[0].1 - 100.0).abs() < 0.01);
    }
}
