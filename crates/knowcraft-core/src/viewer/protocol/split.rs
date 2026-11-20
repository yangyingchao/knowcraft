/// Split a colon separated argument string, honouring `\n`, `\\`, `\:` and any other `\X`
/// escape (which yields the literal `X`). Mirrors `command_arg_split` from the C server.
pub fn split(input: &str) -> Vec<String> {
    if input.is_empty() {
        return Vec::new();
    }
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut field = Vec::new();
    let mut i = 0;
    let mut last = false;

    while i < bytes.len() || last {
        field.clear();
        let mut esc = false;
        while i < bytes.len() && (bytes[i] != b':' || esc) {
            if esc {
                if bytes[i] == b'n' {
                    field.push(b'\n');
                } else {
                    field.push(bytes[i]);
                }
                esc = false;
                i += 1;
            } else if bytes[i] == b'\\' {
                esc = true;
                i += 1;
            } else {
                field.push(bytes[i]);
                i += 1;
            }
        }
        // A byte sequence split on ASCII bytes stays valid UTF-8 from the original input.
        out.push(String::from_utf8_lossy(&field).into_owned());
        last = false;
        if i < bytes.len() {
            i += 1;
            if i == bytes.len() {
                last = true;
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::split;

    #[test]
    fn empty_is_no_args() {
        assert!(split("").is_empty());
    }

    #[test]
    fn trailing_colon_adds_empty() {
        assert_eq!(split("a:"), vec!["a", ""]);
    }

    #[test]
    fn consecutive_colons_add_empty() {
        assert_eq!(split("a::b"), vec!["a", "", "b"]);
    }

    #[test]
    fn escapes() {
        assert_eq!(split(r"a\:b\\c\nd"), vec!["a:b\\c\nd"]);
    }
}
