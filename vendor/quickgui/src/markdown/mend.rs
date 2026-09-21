//! Display-only repair for incomplete inline Markdown in an actively streaming tail.

/// Destination assigned to a link whose URL has not finished streaming.
pub const PENDING_LINK_URL: &str = "quickgui:pending-link";

const ZERO_WIDTH_SPACE: char = '\u{200B}';

#[derive(Debug)]
struct OpenDelimiter {
    marker: char,
    owed: usize,
    opened_at: usize,
}

/// Close hanging emphasis, code, strikethrough, and link markers.
///
/// The canonical source remains untouched. This synthetic copy is parsed only for display while
/// the source is streaming, preventing wrap points from jumping when the real closer arrives.
pub fn close_hanging(text: &str) -> Option<String> {
    let chars = text.char_indices().collect::<Vec<_>>();
    let at = |index: usize| chars.get(index).map(|&(_, character)| character);
    let mut delimiters = Vec::<OpenDelimiter>::new();
    let mut brackets = Vec::<usize>::new();
    let mut code: Option<(usize, usize)> = None;
    let mut last_content: Option<usize> = None;
    let mut pending_url: Option<usize> = None;
    let mut index = 0;

    while index < chars.len() {
        let character = chars[index].1;
        if code.is_none() && character == '\\' {
            if index + 1 < chars.len() {
                last_content = Some(index + 1);
            }
            index += 2;
            continue;
        }
        if character == '`' {
            let run = run_length(&chars, index);
            match code {
                Some((open, _)) if run == open => {
                    code = None;
                    last_content = Some(index + run - 1);
                }
                Some(_) => last_content = Some(index + run - 1),
                None => code = Some((run, index + run)),
            }
            index += run;
            continue;
        }
        if code.is_some() {
            last_content = Some(index);
            index += 1;
            continue;
        }

        match character {
            '*' | '_' | '~' => {
                let run = run_length(&chars, index);
                scan_delimiter(
                    &mut delimiters,
                    character,
                    run,
                    index,
                    &mut last_content,
                    &at,
                );
                index += run;
            }
            '[' => {
                brackets.push(index);
                index += 1;
            }
            ']' => {
                if let Some(open) = brackets.pop() {
                    delimiters.retain(|delimiter| delimiter.opened_at < open);
                    if at(index + 1) == Some('(') {
                        let mut scan = index + 2;
                        let mut depth = 0usize;
                        loop {
                            match at(scan) {
                                Some('(') => depth += 1,
                                Some(')') if depth == 0 => break,
                                Some(')') => depth -= 1,
                                Some(_) => {}
                                None => {
                                    pending_url = Some(index);
                                    break;
                                }
                            }
                            scan += 1;
                        }
                        if pending_url.is_some() {
                            break;
                        }
                        last_content = Some(scan);
                        index = scan + 1;
                        continue;
                    }
                }
                last_content = Some(index);
                index += 1;
            }
            character if character.is_whitespace() => index += 1,
            _ => {
                last_content = Some(index);
                index += 1;
            }
        }
    }

    let content_end = last_content
        .map(|index| chars[index].0 + chars[index].1.len_utf8())
        .unwrap_or(text.len());

    if let Some(bracket) = pending_url {
        let cut = chars[bracket].0;
        let mut closers = String::new();
        close_delimiters(&mut closers, &delimiters, last_content, bracket);
        let mut mended = String::with_capacity(cut + closers.len() + PENDING_LINK_URL.len() + 4);
        mended.push_str(&text[..content_end.min(cut)]);
        mended.push_str(&closers);
        mended.push_str(&text[content_end.min(cut)..cut]);
        mended.push_str("](");
        mended.push_str(PENDING_LINK_URL);
        mended.push(')');
        return Some(mended);
    }

    let mut closers = String::new();
    if let Some((run, content_at)) = code
        && last_content.is_some_and(|content| content >= content_at)
    {
        closers.extend(std::iter::repeat_n('`', run));
    }
    close_delimiters(&mut closers, &delimiters, last_content, chars.len());
    if let Some(bracket) = brackets.last().copied()
        && last_content.is_some_and(|content| content > bracket)
    {
        closers.push_str("](");
        closers.push_str(PENDING_LINK_URL);
        closers.push(')');
    }

    let setext_guard = needs_setext_guard(text);
    if closers.is_empty() && !setext_guard {
        return None;
    }
    let mut mended = String::with_capacity(text.len() + closers.len() + 3);
    mended.push_str(&text[..content_end]);
    mended.push_str(&closers);
    mended.push_str(&text[content_end..]);
    if setext_guard {
        mended.push(ZERO_WIDTH_SPACE);
    }
    Some(mended)
}

fn close_delimiters(
    output: &mut String,
    delimiters: &[OpenDelimiter],
    last_content: Option<usize>,
    limit: usize,
) {
    for delimiter in delimiters.iter().rev() {
        if delimiter.opened_at >= limit
            || last_content.is_none_or(|content| content < delimiter.opened_at)
        {
            continue;
        }
        output.extend(std::iter::repeat_n(delimiter.marker, delimiter.owed));
    }
}

fn run_length(chars: &[(usize, char)], start: usize) -> usize {
    let marker = chars[start].1;
    chars[start..]
        .iter()
        .take_while(|&&(_, character)| character == marker)
        .count()
}

fn scan_delimiter(
    delimiters: &mut Vec<OpenDelimiter>,
    marker: char,
    run: usize,
    index: usize,
    last_content: &mut Option<usize>,
    at: &impl Fn(usize) -> Option<char>,
) {
    if marker == '~' && run < 2 {
        *last_content = Some(index + run - 1);
        return;
    }
    let before = index.checked_sub(1).and_then(at);
    let after = at(index + run);
    let can_close = before.is_some_and(|character| !character.is_whitespace());
    let can_open = after.is_some_and(|character| !character.is_whitespace());

    if can_close
        && let Some(position) = delimiters
            .iter()
            .rposition(|delimiter| delimiter.marker == marker)
        && last_content.is_some_and(|content| content >= delimiters[position].opened_at)
    {
        let owed = delimiters[position].owed;
        if run >= owed {
            delimiters.truncate(position);
        } else {
            delimiters[position].owed = owed - run;
            delimiters.truncate(position + 1);
        }
        *last_content = Some(index + run - 1);
        return;
    }
    if marker == '_' && before.is_some_and(char::is_alphanumeric) {
        *last_content = Some(index + run - 1);
        return;
    }
    if can_open {
        delimiters.push(OpenDelimiter {
            marker,
            owed: run,
            opened_at: index + run,
        });
    } else {
        *last_content = Some(index + run - 1);
    }
}

fn needs_setext_guard(text: &str) -> bool {
    if text.ends_with('\n') {
        return false;
    }
    let mut lines = text.lines().rev();
    let Some(last) = lines.next() else {
        return false;
    };
    let trimmed = last.trim_end();
    if trimmed.is_empty()
        || !(trimmed.chars().all(|character| character == '-')
            || trimmed.chars().all(|character| character == '='))
    {
        return false;
    }
    lines.next().is_some_and(|previous| {
        let previous = previous.trim();
        !previous.is_empty() && !previous.starts_with(['-', '=', '#', '>', '`'])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closes_streaming_inline_markers_without_changing_settled_text() {
        assert_eq!(close_hanging("plain **bold**"), None);
        assert_eq!(close_hanging("now **bold").as_deref(), Some("now **bold**"));
        assert_eq!(close_hanging("call `foo").as_deref(), Some("call `foo`"));
        assert_eq!(
            close_hanging("see [docs](https://exa").as_deref(),
            Some("see [docs](quickgui:pending-link)")
        );
    }

    #[test]
    fn every_utf8_prefix_is_safe_and_one_pass_converges() {
        let source = "Mixed **bold `code`** and *em*, [link](https://example.com), ~~strike~~ 🎉";
        for end in 0..=source.len() {
            if !source.is_char_boundary(end) {
                continue;
            }
            if let Some(mended) = close_hanging(&source[..end]) {
                assert_eq!(
                    close_hanging(&mended),
                    None,
                    "repair did not converge: {mended:?}"
                );
            }
        }
    }
}
