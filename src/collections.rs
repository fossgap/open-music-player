//! Positions identify collection entries, including repeated copies of a song.
pub fn reorder<T: Clone>(
    items: &mut Vec<T>,
    selected: &[usize],
    destination: usize,
) -> Option<Vec<usize>> {
    let selected = positions(selected, items.len())?;
    if destination > items.len() {
        return None;
    }
    let at = destination - selected.iter().filter(|&&p| p < destination).count();
    let moved: Vec<_> = selected.iter().map(|&p| items[p].clone()).collect();
    let mut index = 0;
    items.retain(|_| {
        let keep = selected.binary_search(&index).is_err();
        index += 1;
        keep
    });
    items.splice(at..at, moved);
    Some((at..at + selected.len()).collect())
}

pub fn step<T>(items: &mut [T], selected: &[usize], down: bool) -> Option<Vec<usize>> {
    let mut selected = positions(selected, items.len())?;
    let mut marked = vec![false; items.len()];
    for &p in &selected {
        marked[p] = true;
    }
    let order: Vec<_> = if down {
        (0..selected.len()).rev().collect()
    } else {
        (0..selected.len()).collect()
    };
    for index in order {
        let p = selected[index];
        let next = if down {
            p.checked_add(1)
        } else {
            p.checked_sub(1)
        };
        if let Some(next) = next.filter(|&n| n < items.len() && !marked[n]) {
            items.swap(p, next);
            marked.swap(p, next);
            selected[index] = next;
        }
    }
    Some(selected)
}

pub fn remove<T>(items: &mut Vec<T>, selected: &[usize]) {
    let Some(selected) = positions(selected, items.len()) else {
        return;
    };
    let mut index = 0;
    items.retain(|_| {
        let keep = selected.binary_search(&index).is_err();
        index += 1;
        keep
    });
}

fn positions(selected: &[usize], len: usize) -> Option<Vec<usize>> {
    if selected.is_empty() || selected.iter().any(|&p| p >= len) {
        return None;
    }
    let mut selected = selected.to_vec();
    selected.sort_unstable();
    selected.dedup();
    Some(selected)
}

/// Map a filtered display back to individual occurrences in its source collection.
pub fn source_positions(source: &[String], visible: &[String]) -> Vec<usize> {
    let mut cursor = 0;
    visible
        .iter()
        .filter_map(|id| {
            let offset = source[cursor..].iter().position(|v| v == id)?;
            cursor += offset + 1;
            Some(cursor - 1)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reorder_and_remove_address_occurrences_and_preserve_order() {
        let mut values = vec!["a", "b", "a", "c", "d"];
        assert_eq!(reorder(&mut values, &[2, 4], 1), Some(vec![1, 2]));
        assert_eq!(values, ["a", "a", "d", "b", "c"]);
        remove(&mut values, &[1]);
        assert_eq!(values, ["a", "d", "b", "c"]);
        let before = values.clone();
        assert!(reorder(&mut values, &[99], 0).is_none());
        assert_eq!(values, before);
        assert_eq!(step(&mut values, &[0, 2], true), Some(vec![1, 3]));
        assert_eq!(values, ["d", "a", "c", "b"]);
        assert_eq!(step(&mut values, &[1, 3], false), Some(vec![0, 2]));
        assert_eq!(values, before);
        let source = ["a", "hidden", "a", "b"].map(String::from);
        let visible = ["a", "a"].map(String::from);
        assert_eq!(source_positions(&source, &visible), [0, 2]);
    }
}
