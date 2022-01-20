// use std::time::Duration;

pub(crate) fn to_hex(data: &[u8]) -> String {
	data.iter()
		.map(|b| format!("{:02X}", b))
		.collect::<Vec<_>>()
		.join("")
}

/*
pub(crate) fn duration_str(duration: &Duration) -> String {
	// #![feature(int_log)]
	// 0+ = ns
	// 3+ = us
	// 6+ = ms
	// 9+ = s

	match duration.as_nanos() {
		0..=999 => 					format!("{} ns", duration.as_nanos()),
		1_000..=999_999 => 			format!("{} \u{00B5}s", duration.as_micros()),
		1_000_000..=999_999_999 => 	format!("{} ms", duration.as_millis()),
		_ => 						format!("{} s", duration.as_secs())
	}
}
*/
