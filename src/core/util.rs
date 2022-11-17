pub fn hex(bytes: &[u8]) -> String {
	let mut out = String::with_capacity(bytes.len() * 3 - 1);

	for b in bytes {
		out.push_str(&format!("{:02x}", b));
	}

	out
}

pub fn group_by_key<I, V, K, F>(elements: I, mut f: F) -> std::collections::BTreeMap<K, Vec<V>>
where
	I: IntoIterator<Item = V>,
	K: Eq + std::cmp::Ord,
	F: FnMut(&V) -> K
{
	let mut map: std::collections::BTreeMap<K, Vec<V>> = std::collections::BTreeMap::new();

	for value in elements.into_iter() {
		map.entry(f(&value)).or_default().push(value);
	}

	map
}

pub fn error_chain<E: std::error::Error>(error: E) -> String {
	let mut string = format!("{}", error);

	let mut current = error.source();
	while let Some(cause) = current {
		string.push_str(&format!("\nCaused by:\n\t{}", cause));
		current = cause.source();
	}

	string
}
