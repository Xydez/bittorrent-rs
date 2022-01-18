pub(crate) fn to_hex(data: &[u8]) -> String {
	data.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join("")
}
