pub fn slugify(input: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;

    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(ch.to_ascii_lowercase());
            pending_dash = false;
        } else if !slug.is_empty() {
            pending_dash = true;
        }
    }

    if slug.is_empty() {
        "packet".to_string()
    } else {
        slug
    }
}

pub fn human_title(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return "Untitled Packet".to_string();
    }

    trimmed
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{human_title, slugify};

    #[test]
    fn slugify_collapses_noise() {
        assert_eq!(slugify("Rust TUI / Source Notes"), "rust-tui-source-notes");
        assert_eq!(slugify("  "), "packet");
    }

    #[test]
    fn human_title_expands_slug() {
        assert_eq!(human_title("guide-flow_map"), "Guide Flow Map");
    }
}
