use std::collections::BTreeMap;

pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 3 - 1);

    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }

    out
}

pub fn group_by_key<I, V, K, F>(elements: I, mut f: F) -> BTreeMap<K, Vec<V>>
where
    I: IntoIterator<Item = V>,
    K: Eq + std::cmp::Ord,
    F: FnMut(&V) -> K
{
    let mut map: BTreeMap<K, Vec<V>> = BTreeMap::new();

    for value in elements.into_iter() {
        map.entry(f(&value)).or_default().push(value);
    }

    map
}
