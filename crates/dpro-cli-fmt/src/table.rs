//! Plain-text tables sized to the terminal width.

/// One column: header text and alignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub header: String,
    pub align_right: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<String>>,
}

impl Table {
    #[must_use]
    pub fn new(headers: &[(String, bool)]) -> Self {
        let columns = headers
            .iter()
            .map(|(h, r)| Column {
                header: h.clone(),
                align_right: *r,
            })
            .collect();
        Self {
            columns,
            rows: Vec::new(),
        }
    }

    pub fn push(&mut self, row: Vec<String>) {
        self.rows.push(row);
    }
}

const GAP: usize = 2;

/// Renders `table` in at most `width` columns. Columns keep their natural width except the
/// last left-aligned one, which is truncated with `…` when the line would overflow.
/// Widths count `char`s (wide CJK glyphs are counted as one; see CAVEATS.md).
#[must_use]
pub fn render(table: &Table, width: usize) -> String {
    let n = table.columns.len();
    if n == 0 {
        return String::new();
    }
    let mut widths: Vec<usize> = table
        .columns
        .iter()
        .map(|c| c.header.chars().count())
        .collect();
    for row in &table.rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.chars().count());
        }
    }
    // Shrink the widest left-aligned column (the "object" column) to fit.
    let total: usize = widths.iter().sum::<usize>() + GAP * (n - 1);
    if total > width
        && let Some(flex) = (0..n)
            .filter(|&i| !table.columns[i].align_right)
            .max_by_key(|&i| widths[i])
    {
        widths[flex] = widths[flex].saturating_sub(total - width).max(8);
    }

    let mut out = String::new();
    let header: Vec<String> = table.columns.iter().map(|c| c.header.clone()).collect();
    for row in std::iter::once(&header).chain(&table.rows) {
        let mut line = String::new();
        for (i, w) in widths.iter().enumerate() {
            let cell = row.get(i).map_or("", String::as_str);
            let text = fit(cell, *w);
            let pad = w.saturating_sub(text.chars().count());
            if table.columns[i].align_right {
                line.push_str(&" ".repeat(pad));
                line.push_str(&text);
            } else {
                line.push_str(&text);
                if i + 1 < n {
                    line.push_str(&" ".repeat(pad));
                }
            }
            if i + 1 < n {
                line.push_str(&" ".repeat(GAP));
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn fit(cell: &str, width: usize) -> String {
    if cell.chars().count() <= width {
        return cell.to_owned();
    }
    let mut s: String = cell.chars().take(width.saturating_sub(1)).collect();
    s.push('…');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Table {
        let mut t = Table::new(&[("Name".into(), false), ("Size".into(), true)]);
        t.push(vec!["a".into(), "1 KB".into()]);
        t.push(vec!["longer-name".into(), "10.5 MB".into()]);
        t
    }

    #[test]
    fn aligns_columns() {
        let expected = "Name            Size\na               1 KB\nlonger-name  10.5 MB\n";
        assert_eq!(render(&sample(), 80), expected);
    }

    #[test]
    fn truncates_the_flexible_column() {
        let mut t = sample();
        t.push(vec!["x".repeat(100), "1 B".into()]);
        let out = render(&t, 40);
        assert!(out.lines().all(|l| l.chars().count() <= 40), "{out}");
        assert!(out.contains('…'));
    }
}
