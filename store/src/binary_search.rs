use std::cmp::Ordering;

/// `Ok` is an equal entry. `Err` is the insertion point.
pub async fn binary_search<C, F>(len: u32, mut compare: C) -> Result<u32, u32>
where
    C: FnMut(u32) -> F,
    F: Future<Output = Ordering>,
{
    let mut low = 0;
    let mut high = len;

    while low < high {
        let mid = low + (high - low) / 2;

        match compare(mid).await {
            Ordering::Less => low = mid + 1,
            Ordering::Greater => high = mid,
            Ordering::Equal => return Ok(mid),
        }
    }

    Err(low)
}
