//! Reine Text-Helfer ohne Abhängigkeiten (lokal mit `rustc --test` prüfbar).

/// Entfernt einfache Markdown-Auszeichnung aus einer Zeile.
pub fn strip_md(s: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // Bild ![alt](url) -> weg
        if c == '!' && i + 1 < chars.len() && chars[i + 1] == '[' {
            if let Some(end) = skip_link(&chars, i + 1) {
                i = end;
                continue;
            }
        }
        // Link [text](url) -> text
        if c == '[' {
            if let Some(close) = chars[i + 1..].iter().position(|&x| x == ']') {
                let close = i + 1 + close;
                if close + 1 < chars.len() && chars[close + 1] == '(' {
                    if let Some(p) = chars[close + 1..].iter().position(|&x| x == ')') {
                        let text: String = chars[i + 1..close].iter().collect();
                        out.push_str(&strip_md(&text));
                        i = close + 1 + p + 1;
                        continue;
                    }
                }
            }
        }
        if c == '*' || c == '`' || c == '_' && (i == 0 || i + 1 == chars.len() || !chars[i - 1].is_alphanumeric()) {
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    // HTML-Tags grob entfernen
    let mut res = String::new();
    let mut in_tag = false;
    for c in out.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => res.push(c),
            _ => {}
        }
    }
    res.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn skip_link(chars: &[char], open: usize) -> Option<usize> {
    let close = open + 1 + chars[open + 1..].iter().position(|&x| x == ']')?;
    if close + 1 < chars.len() && chars[close + 1] == '(' {
        let p = chars[close + 1..].iter().position(|&x| x == ')')?;
        return Some(close + 1 + p + 1);
    }
    Some(close + 1)
}

/// Kürzt auf höchstens `max` Zeichen an einer Wortgrenze.
pub fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    let cut = match cut.rfind(' ') {
        Some(i) if i > max / 2 => cut[..i].to_string(),
        _ => cut,
    };
    format!("{}…", cut.trim_end_matches(|c: char| c == ',' || c == '.' || c == ';' || c == ':' || c == ' '))
}

/// Erster sinnvoller Absatz einer README als Kurzbeschreibung.
pub fn readme_excerpt(md: &str, max: usize) -> Option<String> {
    let mut in_code = false;
    let mut para: Vec<String> = Vec::new();
    let mut found: Option<String> = None;
    let flush = |para: &mut Vec<String>, found: &mut Option<String>| {
        if found.is_none() && !para.is_empty() {
            let text = strip_md(&para.join(" "));
            if text.chars().filter(|c| c.is_alphabetic()).count() >= 30 {
                *found = Some(text);
            }
        }
        para.clear();
    };
    for raw in md.lines() {
        let line = raw.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            in_code = !in_code;
            flush(&mut para, &mut found);
            continue;
        }
        if in_code {
            continue;
        }
        let skip = line.is_empty()
            || line.starts_with('#')
            || line.starts_with('|')
            || line.starts_with("---")
            || line.starts_with("===")
            || line.starts_with("- ")
            || line.starts_with("* ")
            || line.starts_with("!")
            || line.starts_with("<")
            || line.starts_with("[!")
            || line.starts_with("> [!");
        if skip {
            flush(&mut para, &mut found);
            if found.is_some() {
                break;
            }
            continue;
        }
        para.push(line.trim_start_matches('>').trim().to_string());
    }
    flush(&mut para, &mut found);
    found.map(|t| shorten(&t, max))
}

/// Sucht in einer Prüfsummen-Datei den SHA-256 für `file`.
pub fn parse_checksum(text: &str, file: &str) -> Option<String> {
    let is_hex = |s: &str| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit());
    let mut lines: Vec<&str> = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    for l in &lines {
        let mut parts = l.split_whitespace();
        if let (Some(h), Some(name)) = (parts.next(), parts.next()) {
            let name = name.trim_start_matches('*').trim_start_matches("./");
            if is_hex(h) && name == file {
                return Some(h.to_lowercase());
            }
        }
    }
    // Einzeilige Datei nur mit Hash (oder Hash + anderer Name bei einer einzelnen Datei)
    if lines.len() == 1 {
        let h = lines.remove(0).split_whitespace().next().unwrap_or("");
        if is_hex(h) {
            return Some(h.to_lowercase());
        }
    }
    None
}

/// Quoting für das Exec-Feld einer .desktop-Datei.
pub fn desktop_exec_quote(path: &str) -> String {
    let mut s = String::from("\"");
    for c in path.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                s.push('\\');
                s.push(c);
            }
            '%' => s.push_str("%%"),
            _ => s.push(c),
        }
    }
    s.push('"');
    s
}

/// Wert in einer .desktop-Datei: Zeilenumbrüche raus.
pub fn desktop_value(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpt() {
        let md = "# LakuenCast\n\n![logo](x.png) [![CI](b.svg)](ci)\n\nLakuenCast überträgt Bild und **Ton** auf deinen [Chromecast](https://x) im Heimnetz, ganz ohne Browser.\nZweite Zeile.\n\n## Features\n- a\n";
        let e = readme_excerpt(md, 400).unwrap();
        assert_eq!(e, "LakuenCast überträgt Bild und Ton auf deinen Chromecast im Heimnetz, ganz ohne Browser. Zweite Zeile.");
        let short = readme_excerpt(md, 40).unwrap();
        assert!(short.ends_with('…') && short.chars().count() <= 41, "{short}");
        assert!(readme_excerpt("# Nur Titel\n\n```\ncode code code code code code code code\n```\n", 100).is_none());
    }

    #[test]
    fn checksum() {
        let h = "a".repeat(64);
        assert_eq!(parse_checksum(&format!("{h}  App-x86_64.AppImage\n"), "App-x86_64.AppImage"), Some(h.clone()));
        assert_eq!(parse_checksum(&format!("{h} *./App.AppImage\n{} other\n", "b".repeat(64)), "App.AppImage"), Some(h.clone()));
        assert_eq!(parse_checksum(&h, "x"), Some(h.clone()));
        assert_eq!(parse_checksum("nope", "x"), None);
    }

    #[test]
    fn desktop() {
        assert_eq!(desktop_exec_quote("/home/deck/My $Apps/a%b"), "\"/home/deck/My \\$Apps/a%%b\"");
    }
}
