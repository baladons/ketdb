use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchResult {
    Found(usize),
    NotFound(usize),
}

impl SearchResult {
    pub fn index(self) -> usize {
        match self {
            SearchResult::Found(index) | SearchResult::NotFound(index) => index,
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
        Some(index) => SearchResult::Found(index),
        None => SearchResult::NotFound(lo),
    })
}
