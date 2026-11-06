//! Line diffs and three-way merges.
//!
//! Used for `grove diff --text`, and for merging two edits to the same
//! declaration when they change different lines of it: Grove only calls it a
//! conflict when both sides changed the same lines differently.

/// One edit in base coordinates: base lines `start..end` become `lines`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub start: usize,
    pub end: usize,
    pub lines: Vec<String>,
}

/// Above this many line pairs, give up on a minimal diff and treat the whole
/// text as replaced (keeps huge files fast; declarations are small).
const MAX_CELLS: usize = 4_000_000;

/// Matching line pairs of a longest common subsequence.
fn lcs(a: &[&str], b: &[&str]) -> Option<Vec<(usize, usize)>> {
    let (n, m) = (a.len(), b.len());
    if n.saturating_mul(m) > MAX_CELLS {
        return None;
    }
    // trim the common prefix and suffix first; most edits are small
    let mut pre = 0;
    while pre < n && pre < m && a[pre] == b[pre] {
        pre += 1;
    }
    let mut suf = 0;
    while suf < n - pre && suf < m - pre && a[n - 1 - suf] == b[m - 1 - suf] {
        suf += 1;
    }
    let (a2, b2) = (&a[pre..n - suf], &b[pre..m - suf]);
    let (n2, m2) = (a2.len(), b2.len());
    let mut dp = vec![0u32; (n2 + 1) * (m2 + 1)];
    let w = m2 + 1;
    for i in (0..n2).rev() {
        for j in (0..m2).rev() {
            dp[i * w + j] = if a2[i] == b2[j] {
                dp[(i + 1) * w + j + 1] + 1
            } else {
                dp[(i + 1) * w + j].max(dp[i * w + j + 1])
            };
        }
    }
    let mut pairs: Vec<(usize, usize)> = (0..pre).map(|k| (k, k)).collect();
    let (mut i, mut j) = (0, 0);
    while i < n2 && j < m2 {
        if a2[i] == b2[j] {
            pairs.push((pre + i, pre + j));
            i += 1;
            j += 1;
        } else if dp[(i + 1) * w + j] >= dp[i * w + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs.extend((0..suf).map(|k| (n - suf + k, m - suf + k)));
    Some(pairs)
}

/// The edits that turn `base` into `other`, in base coordinates.
pub fn hunks(base: &[&str], other: &[&str]) -> Vec<Hunk> {
    let pairs = lcs(base, other).unwrap_or_default();
    let mut out = vec![];
    let (mut pi, mut pj) = (0usize, 0usize);
    for (i, j) in pairs
        .into_iter()
        .chain(std::iter::once((base.len(), other.len())))
    {
        if i > pi || j > pj {
            out.push(Hunk {
                start: pi,
                end: i,
                lines: other[pj..j].iter().map(|s| s.to_string()).collect(),
            });
        }
        pi = i + 1;
        pj = j + 1;
    }
    out
}

/// Merge two edits of `base`. Returns `None` when both sides changed the same
/// lines differently (or both inserted different lines at the same place).
pub fn merge3(base: &str, ours: &str, theirs: &str) -> Option<String> {
    if ours == theirs {
        return Some(ours.to_string());
    }
    if base == ours {
        return Some(theirs.to_string());
    }
    if base == theirs {
        return Some(ours.to_string());
    }
    let b: Vec<&str> = base.lines().collect();
    let o: Vec<&str> = ours.lines().collect();
    let t: Vec<&str> = theirs.lines().collect();
    let (ho, ht) = (hunks(&b, &o), hunks(&b, &t));
    let overlaps = |x: &Hunk, y: &Hunk| {
        let (xi, yi) = (x.start == x.end, y.start == y.end);
        if xi && yi {
            return x.start == y.start;
        }
        if xi {
            return y.start < x.start && x.start < y.end;
        }
        if yi {
            return x.start < y.start && y.start < x.end;
        }
        x.start.max(y.start) < x.end.min(y.end)
    };
    // every pair of overlapping hunks must be the same edit
    for x in &ho {
        for y in &ht {
            if overlaps(x, y) && x != y {
                return None;
            }
        }
    }
    let mut all: Vec<&Hunk> = ho.iter().chain(ht.iter()).collect();
    all.sort_by_key(|h| (h.start, h.end));
    all.dedup_by(|a, b| a == b);
    let mut out: Vec<String> = vec![];
    let mut pos = 0;
    for h in all {
        if h.start < pos {
            return None;
        }
        out.extend(b[pos..h.start].iter().map(|s| s.to_string()));
        out.extend(h.lines.iter().cloned());
        pos = h.end;
    }
    out.extend(b[pos..].iter().map(|s| s.to_string()));
    let nl = if ours.contains("\r\n") || theirs.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut s = out.join(nl);
    if ours.ends_with('\n') || theirs.ends_with('\n') {
        s.push_str(nl);
    }
    Some(s)
}

/// A unified diff with three lines of context, git style.
pub fn unified(old: &str, new: &str, old_label: &str, new_label: &str) -> String {
    if old == new {
        return String::new();
    }
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let hs = hunks(&a, &b);
    if hs.is_empty() {
        return String::new();
    }
    let ctx = 3;
    let mut out = format!("--- {old_label}\n+++ {new_label}\n");
    // group hunks whose context overlaps
    let mut groups: Vec<Vec<&Hunk>> = vec![];
    for h in &hs {
        match groups.last_mut() {
            Some(g) if h.start <= g.last().unwrap().end + 2 * ctx => g.push(h),
            _ => groups.push(vec![h]),
        }
    }
    // offset of each base line in the new text
    for g in groups {
        let from = g[0].start.saturating_sub(ctx);
        let to = (g.last().unwrap().end + ctx).min(a.len());
        let shift_before: isize = hs
            .iter()
            .filter(|h| h.end <= g[0].start && !std::ptr::eq(*h, g[0]))
            .map(|h| h.lines.len() as isize - (h.end - h.start) as isize)
            .sum();
        let mut body = String::new();
        let (mut old_n, mut new_n) = (0, 0);
        let mut pos = from;
        for h in &g {
            for l in &a[pos..h.start] {
                body.push_str(&format!(" {l}\n"));
                old_n += 1;
                new_n += 1;
            }
            for l in &a[h.start..h.end] {
                body.push_str(&format!("-{l}\n"));
                old_n += 1;
            }
            for l in &h.lines {
                body.push_str(&format!("+{l}\n"));
                new_n += 1;
            }
            pos = h.end;
        }
        for l in &a[pos..to] {
            body.push_str(&format!(" {l}\n"));
            old_n += 1;
            new_n += 1;
        }
        let new_from = (from as isize + shift_before).max(0) as usize;
        out.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            from + 1,
            old_n,
            new_from + 1,
            new_n
        ));
        out.push_str(&body);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "fn retryDelay(attempt: u32) -> u32 {\n    let base: u32 = 100\n    let cap: u32 = 5000\n    return base * attempt\n}\n";

    #[test]
    fn different_lines_merge() {
        let ours = BASE.replace("= 100", "= 250");
        let theirs = BASE.replace("base * attempt", "base * pow2(attempt)");
        let m = merge3(BASE, &ours, &theirs).unwrap();
        assert!(m.contains("= 250") && m.contains("pow2(attempt)"), "{m}");
    }

    #[test]
    fn same_line_conflicts() {
        let ours = BASE.replace("base * attempt", "min(base * attempt, 5000)");
        let theirs = BASE.replace("base * attempt", "base * pow2(attempt)");
        assert_eq!(merge3(BASE, &ours, &theirs), None);
    }

    #[test]
    fn insertions_at_different_places_merge() {
        let ours = BASE.replace("    return", "    log(attempt)\n    return");
        let theirs = BASE.replace("    let cap", "    // capped\n    let cap");
        let m = merge3(BASE, &ours, &theirs).unwrap();
        assert!(m.contains("log(attempt)") && m.contains("// capped"), "{m}");
    }

    #[test]
    fn unified_diff_looks_like_git() {
        let d = unified(BASE, &BASE.replace("= 100", "= 250"), "a/x.0", "b/x.0");
        assert!(
            d.contains("-    let base: u32 = 100\n+    let base: u32 = 250\n"),
            "{d}"
        );
        assert!(
            d.starts_with("--- a/x.0\n+++ b/x.0\n@@ -1,5 +1,5 @@"),
            "{d}"
        );
    }
}
