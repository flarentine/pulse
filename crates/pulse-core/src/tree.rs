use std::collections::{HashMap, HashSet};

use crate::process::ProcessRow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleRow {
    pub idx: usize,
    pub depth: usize,
    pub has_children: bool,
}

// Windows recycles PIDs, so a recorded parent PID may now belong to an
// unrelated process that started later. Same check Process Explorer uses.
pub fn is_real_parent(parent: &ProcessRow, child: &ProcessRow) -> bool {
    parent.pid != child.pid
        && (parent.start_time_secs == 0
            || child.start_time_secs == 0
            || parent.start_time_secs <= child.start_time_secs)
}

/// Parent/child index over one snapshot. Indices refer to the `rows` slice
/// it was built from.
#[derive(Default)]
pub struct ProcessTree {
    pid_index: HashMap<u32, usize>,
    children: HashMap<Option<u32>, Vec<usize>>,
}

impl ProcessTree {
    pub fn build(rows: &[ProcessRow]) -> Self {
        let pid_index: HashMap<u32, usize> =
            rows.iter().enumerate().map(|(i, r)| (r.pid, i)).collect();
        let mut children: HashMap<Option<u32>, Vec<usize>> = HashMap::new();
        for (idx, row) in rows.iter().enumerate() {
            let parent = row.parent_pid.filter(|ppid| {
                pid_index
                    .get(ppid)
                    .is_some_and(|&p| is_real_parent(&rows[p], row))
            });
            children.entry(parent).or_default().push(idx);
        }
        Self {
            pid_index,
            children,
        }
    }

    pub fn index_of(&self, pid: u32) -> Option<usize> {
        self.pid_index.get(&pid).copied()
    }

    /// `None` lists the roots.
    pub fn children(&self, parent: Option<u32>) -> &[usize] {
        self.children.get(&parent).map_or(&[], Vec::as_slice)
    }

    /// `root` followed by everything below it.
    pub fn descendants(&self, rows: &[ProcessRow], root: usize) -> Vec<usize> {
        // `seen` guards against a parent cycle, which PID reuse can produce
        // when start times are unavailable.
        let mut seen = HashSet::from([root]);
        let mut out = vec![root];
        let mut i = 0;
        while i < out.len() {
            for &child in self.children(Some(rows[out[i]].pid)) {
                if seen.insert(child) {
                    out.push(child);
                }
            }
            i += 1;
        }
        out
    }

    /// `keep` plus every ancestor of each entry in it.
    pub fn with_ancestors(&self, rows: &[ProcessRow], mut keep: HashSet<usize>) -> HashSet<usize> {
        let start: Vec<usize> = keep.iter().copied().collect();
        for idx in start {
            let mut child = &rows[idx];
            while let Some(p) = child.parent_pid.and_then(|ppid| self.index_of(ppid)) {
                let parent = &rows[p];
                if !is_real_parent(parent, child) || !keep.insert(p) {
                    break;
                }
                child = parent;
            }
        }
        keep
    }

    /// Depth-first display order over the rows in `keep`. Children of a pid
    /// for which `expanded` returns false are skipped; `sort` orders siblings.
    pub fn flatten(
        &self,
        rows: &[ProcessRow],
        keep: &HashSet<usize>,
        expanded: impl Fn(u32) -> bool,
        sort: impl Fn(&mut [usize]),
    ) -> Vec<VisibleRow> {
        let kept_children = |parent: Option<u32>| {
            let mut kids: Vec<usize> = self
                .children(parent)
                .iter()
                .copied()
                .filter(|i| keep.contains(i))
                .collect();
            sort(&mut kids);
            kids
        };

        let mut out = Vec::with_capacity(keep.len());
        let mut stack: Vec<(usize, usize)> = kept_children(None)
            .into_iter()
            .rev()
            .map(|i| (i, 0))
            .collect();
        while let Some((idx, depth)) = stack.pop() {
            let pid = rows[idx].pid;
            let kids = kept_children(Some(pid));
            out.push(VisibleRow {
                idx,
                depth,
                has_children: !kids.is_empty(),
            });
            if expanded(pid) {
                stack.extend(kids.into_iter().rev().map(|i| (i, depth + 1)));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::test_row;

    fn all(rows: &[ProcessRow]) -> HashSet<usize> {
        (0..rows.len()).collect()
    }

    fn by_pid(rows: &[ProcessRow]) -> impl Fn(&mut [usize]) + '_ {
        |v: &mut [usize]| v.sort_by_key(|&i| rows[i].pid)
    }

    fn layout(rows: &[ProcessRow], vis: &[VisibleRow]) -> Vec<(u32, usize)> {
        vis.iter().map(|v| (rows[v.idx].pid, v.depth)).collect()
    }

    // 1
    // |- 2
    // |  `- 4
    // `- 3
    fn family() -> Vec<ProcessRow> {
        vec![
            test_row(1, None, "init", 10),
            test_row(2, Some(1), "a", 20),
            test_row(3, Some(1), "b", 20),
            test_row(4, Some(2), "c", 30),
        ]
    }

    #[test]
    fn flattens_depth_first_with_depths() {
        let rows = family();
        let tree = ProcessTree::build(&rows);
        let vis = tree.flatten(&rows, &all(&rows), |_| true, by_pid(&rows));
        assert_eq!(layout(&rows, &vis), [(1, 0), (2, 1), (4, 2), (3, 1)]);
        assert!(vis[0].has_children && vis[1].has_children);
        assert!(!vis[2].has_children && !vis[3].has_children);
    }

    #[test]
    fn collapsed_node_hides_children_but_keeps_its_marker() {
        let rows = family();
        let tree = ProcessTree::build(&rows);
        let vis = tree.flatten(&rows, &all(&rows), |pid| pid != 2, by_pid(&rows));
        assert_eq!(layout(&rows, &vis), [(1, 0), (2, 1), (3, 1)]);
        assert!(vis[1].has_children);
    }

    #[test]
    fn missing_parent_becomes_root() {
        let rows = vec![test_row(7, Some(99), "orphan", 5)];
        let tree = ProcessTree::build(&rows);
        assert_eq!(tree.children(None), [0]);
    }

    #[test]
    fn reused_parent_pid_is_not_a_parent() {
        // 5's real parent exited and PID 1 now belongs to something newer.
        let rows = vec![
            test_row(1, None, "newcomer", 500),
            test_row(5, Some(1), "old child", 100),
        ];
        let tree = ProcessTree::build(&rows);
        assert_eq!(tree.children(None).len(), 2);
        assert!(tree.children(Some(1)).is_empty());
        assert_eq!(tree.descendants(&rows, 0), [0]);
    }

    #[test]
    fn unknown_start_times_still_link() {
        let rows = vec![
            test_row(4, None, "System", 0),
            test_row(100, Some(4), "smss", 0),
        ];
        let tree = ProcessTree::build(&rows);
        assert_eq!(tree.children(Some(4)), [1]);
    }

    #[test]
    fn self_parent_is_a_root() {
        let rows = vec![test_row(0, Some(0), "Idle", 0)];
        let tree = ProcessTree::build(&rows);
        assert_eq!(tree.children(None), [0]);
    }

    #[test]
    fn descendants_covers_whole_subtree() {
        let rows = family();
        let tree = ProcessTree::build(&rows);
        let mut d = tree.descendants(&rows, 0);
        d.sort();
        assert_eq!(d, [0, 1, 2, 3]);
        assert_eq!(tree.descendants(&rows, 1), [1, 3]);
    }

    #[test]
    fn descendants_survives_a_cycle() {
        // Both start times unknown, each claims the other as parent.
        let rows = vec![
            test_row(10, Some(20), "x", 0),
            test_row(20, Some(10), "y", 0),
        ];
        let tree = ProcessTree::build(&rows);
        let mut d = tree.descendants(&rows, 0);
        d.sort();
        assert_eq!(d, [0, 1]);
    }

    #[test]
    fn search_keeps_ancestors_of_matches() {
        let rows = family();
        let tree = ProcessTree::build(&rows);
        let keep = tree.with_ancestors(&rows, HashSet::from([3])); // pid 4
        let vis = tree.flatten(&rows, &keep, |_| true, by_pid(&rows));
        assert_eq!(layout(&rows, &vis), [(1, 0), (2, 1), (4, 2)]);
    }
}
