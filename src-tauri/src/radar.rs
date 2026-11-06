//! Collision radar: which workspaces in one repo change the same files, and the order to merge
//! them in. Pure logic over file lists that `line_counts` already gathered, so it costs no git.

use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

/// One active workspace as the radar sees it.
pub struct Entry<'a> {
    pub id: &'a str,
    pub repo: &'a str,
    pub created: &'a str,
    /// Lines added plus removed against the target branch.
    pub lines: u32,
    pub files: &'a [String],
    /// PR state from GitHub: OPEN, DRAFT, MERGED, CLOSED, or None without a PR.
    pub pr: Option<&'a str>,
}

/// Two workspaces and the files both of them change.
#[derive(Debug, Serialize, PartialEq)]
pub struct Pair {
    pub a: String,
    pub b: String,
    pub files: Vec<String>,
}

/// Workspaces linked by shared files (directly or through each other), in suggested merge order.
#[derive(Debug, Serialize, PartialEq)]
pub struct Group {
    pub repo_id: String,
    pub order: Vec<String>,
    pub pairs: Vec<Pair>,
}

/// Merge-order rank: a PR ready for review first, then a draft, then no PR yet.
fn rank(pr: Option<&str>) -> u8 {
    match pr {
        Some("OPEN") => 0,
        Some("DRAFT") => 1,
        _ => 2,
    }
}

/// Groups of colliding workspaces. Merged and closed PRs are left out: their work is already in,
/// or abandoned. Within a group the suggested order is: ready PRs, then drafts, then the rest;
/// within each, the smaller change first (the bigger one then rebases over less), then the older.
pub fn collisions(entries: &[Entry]) -> Vec<Group> {
    let live: Vec<&Entry> = entries.iter().filter(|e| !matches!(e.pr, Some("MERGED" | "CLOSED")) && !e.files.is_empty()).collect();
    let mut pairs: Vec<(usize, usize, Vec<String>)> = vec![];
    for (i, a) in live.iter().enumerate() {
        let mine: BTreeSet<&String> = a.files.iter().collect();
        for (j, b) in live.iter().enumerate().skip(i + 1) {
            if a.repo != b.repo {
                continue;
            }
            let shared: BTreeSet<&String> = b.files.iter().filter(|f| mine.contains(f)).collect();
            if !shared.is_empty() {
                pairs.push((i, j, shared.into_iter().cloned().collect()));
            }
        }
    }
    // Union-find over the pairs, so A~B and B~C land in one group.
    let mut parent: Vec<usize> = (0..live.len()).collect();
    fn root(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for (i, j, _) in &pairs {
        let (ri, rj) = (root(&mut parent, *i), root(&mut parent, *j));
        parent[ri] = rj;
    }
    let roots: Vec<usize> = (0..live.len()).map(|x| root(&mut parent, x)).collect();
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, j, _) in &pairs {
        for x in [*i, *j] {
            let g = groups.entry(roots[x]).or_default();
            if !g.contains(&x) {
                g.push(x);
            }
        }
    }
    let mut out: Vec<Group> = groups
        .into_iter()
        .map(|(r, mut members)| {
            members.sort_by(|&x, &y| {
                let (a, b) = (live[x], live[y]);
                (rank(a.pr), a.lines, a.created, a.id).cmp(&(rank(b.pr), b.lines, b.created, b.id))
            });
            let pairs = pairs
                .iter()
                .filter(|(i, _, _)| roots[*i] == r)
                .map(|(i, j, f)| Pair { a: live[*i].id.to_string(), b: live[*j].id.to_string(), files: f.clone() })
                .collect();
            Group { repo_id: live[members[0]].repo.to_string(), order: members.iter().map(|&m| live[m].id.to_string()).collect(), pairs }
        })
        .collect();
    out.sort_by(|a, b| (&a.repo_id, &a.order[0]).cmp(&(&b.repo_id, &b.order[0])));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(f: &[&str]) -> Vec<String> {
        f.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_shared_files_or_other_repos_means_no_collision() {
        let (a, b, c) = (files(&["a.go"]), files(&["b.go"]), files(&["a.go"]));
        let e = [
            Entry { id: "w1", repo: "r1", created: "1", lines: 5, files: &a, pr: None },
            Entry { id: "w2", repo: "r1", created: "2", lines: 5, files: &b, pr: None },
            // Same path, different repo: not a conflict.
            Entry { id: "w3", repo: "r2", created: "3", lines: 5, files: &c, pr: None },
        ];
        assert!(collisions(&e).is_empty());
    }

    #[test]
    fn chains_join_one_group_ordered_ready_prs_then_smaller_changes() {
        let (a, b, c, d) = (files(&["http.go", "x.go"]), files(&["http.go", "sla.go"]), files(&["sla.go"]), files(&["other.go"]));
        let e = [
            Entry { id: "big", repo: "r1", created: "1", lines: 150, files: &a, pr: None },
            Entry { id: "draft", repo: "r1", created: "2", lines: 400, files: &b, pr: Some("DRAFT") },
            Entry { id: "ready", repo: "r1", created: "3", lines: 900, files: &c, pr: Some("OPEN") },
            Entry { id: "alone", repo: "r1", created: "4", lines: 1, files: &d, pr: None },
        ];
        let g = collisions(&e);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].order, ["ready", "draft", "big"]);
        assert_eq!(g[0].pairs, vec![
            Pair { a: "big".into(), b: "draft".into(), files: files(&["http.go"]) },
            Pair { a: "draft".into(), b: "ready".into(), files: files(&["sla.go"]) },
        ]);
    }

    #[test]
    fn merged_and_closed_prs_drop_out_and_ties_go_to_the_smaller_then_older() {
        let f = files(&["a.go"]);
        let e = [
            Entry { id: "merged", repo: "r1", created: "0", lines: 1, files: &f, pr: Some("MERGED") },
            Entry { id: "closed", repo: "r1", created: "0", lines: 1, files: &f, pr: Some("CLOSED") },
            Entry { id: "newer", repo: "r1", created: "2", lines: 10, files: &f, pr: None },
            Entry { id: "older", repo: "r1", created: "1", lines: 10, files: &f, pr: None },
            Entry { id: "small", repo: "r1", created: "3", lines: 2, files: &f, pr: None },
        ];
        let g = collisions(&e);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].order, ["small", "older", "newer"]);
        assert_eq!(g[0].pairs.len(), 3);
    }

    #[test]
    fn separate_clusters_stay_separate() {
        let (a, b) = (files(&["a.go"]), files(&["b.go"]));
        let e = [
            Entry { id: "a1", repo: "r1", created: "1", lines: 1, files: &a, pr: None },
            Entry { id: "a2", repo: "r1", created: "2", lines: 2, files: &a, pr: None },
            Entry { id: "b1", repo: "r1", created: "3", lines: 1, files: &b, pr: None },
            Entry { id: "b2", repo: "r1", created: "4", lines: 2, files: &b, pr: None },
        ];
        let g = collisions(&e);
        assert_eq!(g.iter().map(|g| g.order.clone()).collect::<Vec<_>>(), vec![vec!["a1", "a2"], vec!["b1", "b2"]]);
        assert!(g.iter().all(|g| g.pairs.len() == 1));
    }
}
