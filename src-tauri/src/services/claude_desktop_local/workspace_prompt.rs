//! Read the native client's bundled workspace instructions as data.

pub(super) fn from_module(source: &str) -> Option<String> {
    let export = regex::Regex::new(
        r#"DESKTOP_3P_BASE_PROMPT["']?\s*:\s*\(\)\s*=>\s*([A-Za-z_$][A-Za-z0-9_$]*)"#,
    )
    .ok()?;
    let (variable, remaining) = if let Some(capture) = export.captures(source) {
        (capture.get(1)?.as_str(), &source[capture.get(0)?.end()..])
    } else {
        ("DESKTOP_3P_BASE_PROMPT", source)
    };
    let assignment = regex::Regex::new(&format!(
        r#"(?:^|[^A-Za-z0-9_$]){}\s*=\s*([`"'])"#,
        regex::escape(variable)
    ))
    .ok()?;
    let capture = assignment.captures(remaining)?;
    let delimiter = capture.get(1)?.as_str().chars().next()?;
    literal(&remaining[capture.get(0)?.end()..], delimiter).filter(|value| !value.trim().is_empty())
}

fn literal(source: &str, delimiter: char) -> Option<String> {
    let mut chars = source.chars().peekable();
    let mut value = String::new();
    while let Some(character) = chars.next() {
        if character == delimiter {
            return Some(value);
        }
        if delimiter == '`' && character == '$' && chars.peek() == Some(&'{') {
            return None;
        }
        if character != '\\' {
            value.push(character);
            continue;
        }
        match chars.next()? {
            'n' => value.push('\n'),
            'r' => value.push('\r'),
            't' => value.push('\t'),
            'b' => value.push('\u{8}'),
            'f' => value.push('\u{c}'),
            'v' => value.push('\u{b}'),
            '0' => value.push('\0'),
            '\n' => {}
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
            }
            'x' => {
                let digits = chars.by_ref().take(2).collect::<String>();
                if digits.len() != 2 {
                    return None;
                }
                value.push(char::from_u32(u32::from_str_radix(&digits, 16).ok()?)?);
            }
            'u' => {
                let digits = if chars.peek() == Some(&'{') {
                    chars.next();
                    let mut digits = String::new();
                    loop {
                        let digit = chars.next()?;
                        if digit == '}' {
                            break;
                        }
                        if digits.len() == 6 {
                            return None;
                        }
                        digits.push(digit);
                    }
                    digits
                } else {
                    let digits = chars.by_ref().take(4).collect::<String>();
                    if digits.len() != 4 {
                        return None;
                    }
                    digits
                };
                value.push(char::from_u32(u32::from_str_radix(&digits, 16).ok()?)?);
            }
            escaped => value.push(escaped),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_static_bundled_literals_without_running_the_module() {
        assert_eq!(from_module(r#"sideEffect();var module=wrap({DESKTOP_3P_BASE_PROMPT:()=>p}),p=`Hello\n\u4e16\u{754c} \`code\` {{modelName}}`;"#).unwrap(), "Hello\n世界 `code` {{modelName}}");
        assert_eq!(
            from_module(r#"export const DESKTOP_3P_BASE_PROMPT = "Keep \\n literal";"#).unwrap(),
            "Keep \\n literal"
        );
        assert!(
            from_module("var module={DESKTOP_3P_BASE_PROMPT:()=>p},p=`${execute()}`").is_none()
        );
        assert!(from_module("const DESKTOP_3P_BASE_PROMPT = `unterminated").is_none());
        assert!(from_module("const DESKTOP_3P_BASE_PROMPT = ``").is_none());
    }
}
