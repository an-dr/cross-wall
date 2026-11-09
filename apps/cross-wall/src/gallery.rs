//! Which photos to load ahead of the one being previewed. The gallery's
//! justified rows are bones' `gallery` layout.

/// The photos around `position` to load ahead, nearest first, forward before back.
pub fn neighbours(position: usize, count: usize, radius: usize) -> Vec<usize> {
    let mut around = Vec::new();
    for distance in 1..=radius {
        if position + distance < count {
            around.push(position + distance);
        }
        if distance <= position {
            around.push(position - distance);
        }
    }
    around
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbours_are_nearest_first_and_stay_in_range() {
        assert_eq!(neighbours(5, 20, 3), [6, 4, 7, 3, 8, 2]);
        assert_eq!(neighbours(0, 3, 3), [1, 2]);
        assert_eq!(neighbours(2, 3, 3), [1, 0]);
        assert!(neighbours(0, 1, 3).is_empty());
    }
}
