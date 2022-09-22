pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 3 - 1);

    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }

    out
}
