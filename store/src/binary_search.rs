use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchResult {
    Found(u32),
    NotFound(u32),
}

impl SearchResult {
    pub fn index(self) -> u32 {
        match self {
            SearchResult::Found(row_index) | SearchResult::NotFound(row_index) => row_index,
        }
    }

    pub fn map(self, f: impl FnOnce(u32) -> u32) -> SearchResult {
        match self {
            SearchResult::Found(row_index) => SearchResult::Found(f(row_index)),
            SearchResult::NotFound(row_index) => SearchResult::NotFound(f(row_index)),
        }
    }
}

pub async fn binary_search<C, E>(len: usize, mut compare: C) -> Result<SearchResult, E>
where
    C: AsyncFnMut(usize) -> Result<Ordering, E>,
{
    let mut lo = 0;
    let mut hi = len;
    let mut found = None;
    while lo < hi {
        let mid = (lo + hi) / 2;
        match compare(mid).await? {
            Ordering::Less => lo = mid + 1,
            Ordering::Equal => {
                found = Some(mid);
                hi = mid;
            }
            Ordering::Greater => hi = mid,
        }
    }

    Ok(match found {
        Some(index) => SearchResult::Found(index as u32),
        None => SearchResult::NotFound(lo as u32),
    })
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use super::*;

    async fn search_in(values: &[i32], key: i32) -> SearchResult {
        binary_search(values.len(), async |mid| {
            Ok::<Ordering, Infallible>(values[mid].cmp(&key))
        })
        .await
        .unwrap()
    }

    #[compio::test]
    async fn empty_slice_returns_not_found_zero() {
        assert_eq!(search_in(&[], 1).await, SearchResult::NotFound(0));
    }

    #[compio::test]
    async fn found_returns_leftmost_equal_index() {
        let values = [1, 2, 2, 2, 3];
        assert_eq!(search_in(&values, 2).await, SearchResult::Found(1));
    }

    #[compio::test]
    async fn not_found_returns_lower_bound() {
        let values = [10, 20, 30];
        assert_eq!(search_in(&values, 15).await, SearchResult::NotFound(1));
        assert_eq!(search_in(&values, 5).await, SearchResult::NotFound(0));
        assert_eq!(search_in(&values, 40).await, SearchResult::NotFound(3));
    }

    #[compio::test]
    async fn found_at_edges() {
        let values = [5, 10, 15];
        assert_eq!(search_in(&values, 5).await, SearchResult::Found(0));
        assert_eq!(search_in(&values, 15).await, SearchResult::Found(2));
    }

    #[compio::test]
    async fn search_result_map_preserves_variant() {
        let found = SearchResult::Found(2).map(|i| i + 10);
        let not_found = SearchResult::NotFound(2).map(|i| i + 10);
        assert_eq!(found, SearchResult::Found(12));
        assert_eq!(not_found, SearchResult::NotFound(12));
    }
}
