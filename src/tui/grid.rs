//! Pure math and text helpers for the repo card grid.

pub const MIN_CARD_WIDTH: u16 = 34;
pub const CARD_HEIGHT: u16 = 7;

/// Number of card columns that fit `width` (at least 1).
pub fn columns(width: u16) -> usize {
    ((width / MIN_CARD_WIDTH) as usize).max(1)
}

/// Layout: entry 0 is a full-width card alone in row 0; entries 1.. are
/// repo cards filling rows of `cols` starting at row 1.
///
/// Previous/next card within the repo cards; the wide card has no row
/// neighbors, and moving left never leaves the repo cards.
pub fn move_horizontal(sel: usize, delta: isize, len: usize) -> usize {
    if len == 0 || sel == 0 {
        return 0;
    }
    (sel as isize + delta).clamp(1, len as isize - 1) as usize
}

/// One row down (`down`) or up. Down from the wide card goes to the first
/// repo card, up from the first repo row goes back to it; a shorter last row
/// is entered at its last card.
pub fn move_vertical(sel: usize, down: bool, cols: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let cols = cols.max(1);
    let sel = sel.min(len - 1);
    if down {
        if sel == 0 {
            return if len > 1 { 1 } else { 0 };
        }
        let last_row = (len - 2) / cols;
        if (sel - 1) / cols >= last_row {
            sel
        } else {
            (sel + cols).min(len - 1)
        }
    } else {
        sel.saturating_sub(cols)
    }
}

/// Grid row of entry `sel` (row 0 holds only the wide card).
pub fn row_of(sel: usize, cols: usize) -> usize {
    if sel == 0 {
        0
    } else {
        1 + (sel - 1) / cols.max(1)
    }
}

/// Total grid rows for `len` entries.
pub fn total_rows(len: usize, cols: usize) -> usize {
    if len == 0 {
        0
    } else {
        1 + (len - 1).div_ceil(cols.max(1))
    }
}

/// Keeps `sel_row` inside the window of `visible` rows starting at `offset`.
pub fn scroll_offset(offset: usize, sel_row: usize, visible: usize) -> usize {
    let visible = visible.max(1);
    if sel_row < offset {
        sel_row
    } else if sel_row >= offset + visible {
        sel_row + 1 - visible
    } else {
        offset
    }
}

/// Cuts `s` to `width` chars, ending with `…` when shortened.
pub fn truncate(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out: String = s.chars().take(width - 1).collect();
    out.push('…');
    out
}

/// Greedy word wrap into at most `max_lines` lines of `width` chars; the
/// last line is truncated with `…` if text remains.
pub fn wrap_lines(s: &str, width: usize, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    if width == 0 || max_lines == 0 {
        return lines;
    }
    let mut cur = String::new();
    for w in s.split_whitespace() {
        let sep = usize::from(!cur.is_empty());
        if cur.chars().count() + sep + w.chars().count() <= width {
            if sep == 1 {
                cur.push(' ');
            }
            cur.push_str(w);
            continue;
        }
        if !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            if lines.len() == max_lines {
                return ellipsize_last(lines, width);
            }
        }
        // Word longer than a whole line (or first word): hard cut.
        let mut rest: String = w.to_string();
        while rest.chars().count() > width {
            if lines.len() + 1 == max_lines {
                lines.push(truncate(&rest, width));
                return lines;
            }
            lines.push(rest.chars().take(width).collect());
            rest = rest.chars().skip(width).collect();
        }
        cur = rest;
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Marks the last line as cut short, staying within `width`.
fn ellipsize_last(mut lines: Vec<String>, width: usize) -> Vec<String> {
    if let Some(last) = lines.last_mut() {
        if last.chars().count() >= width {
            last.pop();
        }
        last.push('…');
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_floor_min_one() {
        assert_eq!(columns(0), 1);
        assert_eq!(columns(33), 1);
        assert_eq!(columns(34), 1);
        assert_eq!(columns(68), 2);
        assert_eq!(columns(103), 3);
        assert_eq!(columns(140), 4);
    }

    #[test]
    fn horizontal_clamps_and_skips_wide_card() {
        // index 0 is the full-width card; repos are 1..len
        assert_eq!(move_horizontal(0, 1, 5), 0);
        assert_eq!(move_horizontal(0, -1, 5), 0);
        assert_eq!(move_horizontal(1, -1, 5), 1);
        assert_eq!(move_horizontal(4, 1, 5), 4);
        assert_eq!(move_horizontal(2, 1, 5), 3);
        assert_eq!(move_horizontal(0, 1, 0), 0);
    }

    #[test]
    fn vertical_moves_by_row_and_clamps() {
        // 3 cols, 7 entries: [0 wide] / [1 2 3] [4 5 6]
        assert_eq!(move_vertical(0, true, 3, 7), 1);
        assert_eq!(move_vertical(0, false, 3, 7), 0);
        assert_eq!(move_vertical(2, true, 3, 7), 5);
        assert_eq!(move_vertical(5, false, 3, 7), 2);
        assert_eq!(move_vertical(1, false, 3, 7), 0);
        assert_eq!(move_vertical(3, false, 3, 7), 0);
        assert_eq!(move_vertical(6, true, 3, 7), 6);
        // shorter last row: [0] / [1 2 3] [4 5]
        assert_eq!(move_vertical(3, true, 3, 6), 5);
        assert_eq!(move_vertical(2, true, 3, 6), 5);
        assert_eq!(move_vertical(1, true, 3, 6), 4);
        // only the wide card
        assert_eq!(move_vertical(0, true, 3, 1), 0);
        assert_eq!(move_vertical(0, true, 3, 0), 0);
        // single column behaves like a list
        assert_eq!(move_vertical(0, true, 1, 3), 1);
        assert_eq!(move_vertical(1, true, 1, 3), 2);
        assert_eq!(move_vertical(2, true, 1, 3), 2);
        assert_eq!(move_vertical(1, false, 1, 3), 0);
    }

    #[test]
    fn rows_put_wide_card_alone_first() {
        assert_eq!(row_of(0, 3), 0);
        assert_eq!(row_of(1, 3), 1);
        assert_eq!(row_of(3, 3), 1);
        assert_eq!(row_of(4, 3), 2);
        assert_eq!(row_of(2, 1), 2);
        assert_eq!(total_rows(1, 3), 1);
        assert_eq!(total_rows(7, 3), 3);
        assert_eq!(total_rows(8, 3), 4);
        assert_eq!(total_rows(3, 1), 3);
        assert_eq!(total_rows(0, 3), 0);
    }

    #[test]
    fn scroll_keeps_selection_visible() {
        assert_eq!(scroll_offset(0, 0, 2), 0);
        assert_eq!(scroll_offset(0, 1, 2), 0);
        assert_eq!(scroll_offset(0, 2, 2), 1);
        assert_eq!(scroll_offset(3, 1, 2), 1);
        assert_eq!(scroll_offset(1, 2, 2), 1);
        assert_eq!(scroll_offset(0, 5, 0), 5);
    }

    #[test]
    fn truncate_adds_ellipsis() {
        assert_eq!(truncate("hello", 5), "hello");
        assert_eq!(truncate("hello world", 6), "hello…");
        assert_eq!(truncate("abc", 0), "");
    }

    #[test]
    fn wrap_two_lines_with_ellipsis() {
        assert_eq!(wrap_lines("", 10, 2), Vec::<String>::new());
        assert_eq!(wrap_lines("short", 10, 2), vec!["short"]);
        assert_eq!(wrap_lines("aaa bbb ccc", 7, 2), vec!["aaa bbb", "ccc"]);
        let l = wrap_lines("aaa bbb ccc ddd eee fff", 7, 2);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0], "aaa bbb");
        assert!(l[1].ends_with('…') && l[1].chars().count() <= 7);
        let l = wrap_lines(&"x".repeat(50), 10, 2);
        assert_eq!(l.len(), 2);
        assert!(l.iter().all(|s| s.chars().count() <= 10));
    }
}
