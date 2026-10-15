use std::cmp::Ordering;

pub async fn binary_search<C, F>(len: u32, mut compare: C) -> u32
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
            Ordering::Equal => return mid,
        }
    }

    low
}
