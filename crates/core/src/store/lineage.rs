//! Infer copy/fork lineage between sessions in the same project directory from
//! shared message uuids. When Claude Code forks/resumes/background-copies a
//! session it reuses the SAME message uuids for the copied portion, so a large
//! uuid overlap is the reliable lineage signal — Claude Code does not always
//! write an explicit `forkedFrom`.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

/// Per-session inputs for lineage detection.
pub struct SessionUuids {
    /// The session id (the `.jsonl` file stem).
    pub id: Uuid,
    /// The session's first message uuid in file order (its tree root). For a
    /// copy this is the fork point; for an origin it's its own earliest message.
    pub head: Option<Uuid>,
    /// All message uuids in the session.
    pub uuids: HashSet<Uuid>,
    /// Whether the session already has an explicit `forked_from` (skip — the
    /// explicit fork link already covers it).
    pub forked: bool,
}

/// For each non-explicitly-forked session `S`, find its copy-ancestor: another
/// session `A` that CONTAINS `S.head` as a non-root message (`A.head != S.head`),
/// which means `A` had that message before `S` was copied from it. Among
/// candidates, pick the largest uuid overlap (tie: larger uuid set, then smaller
/// id). Returns `descendant id -> (ancestor id, overlap count)`.
///
/// The `A.head != S.head` guard is what makes direction unambiguous and
/// cycle-free: if two sessions merely share the same starting message (siblings
/// or exact dupes) neither is treated as the other's ancestor.
pub fn copy_lineage(sessions: &[SessionUuids]) -> HashMap<Uuid, (Uuid, usize)> {
    let mut out = HashMap::new();
    for s in sessions {
        if s.forked {
            continue;
        }
        let Some(head) = s.head else { continue };
        // best = (overlap, ancestor_uuid_len, ancestor_id)
        let mut best: Option<(usize, usize, Uuid)> = None;
        for a in sessions {
            if a.id == s.id {
                continue;
            }
            if a.head == Some(head) {
                continue; // shared start => not an ancestor
            }
            if !a.uuids.contains(&head) {
                continue;
            }
            let overlap = s.uuids.intersection(&a.uuids).count();
            let cand = (overlap, a.uuids.len(), a.id);
            let better = match best {
                None => true,
                Some((bo, bl, bid)) => {
                    (overlap, a.uuids.len()) > (bo, bl)
                        || ((overlap, a.uuids.len()) == (bo, bl) && a.id < bid)
                }
            };
            if better {
                best = Some(cand);
            }
        }
        if let Some((overlap, _, ancestor_id)) = best {
            out.insert(s.id, (ancestor_id, overlap));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }
    fn set(items: &[u128]) -> HashSet<Uuid> {
        items.iter().map(|n| u(*n)).collect()
    }

    #[test]
    fn descendant_links_to_ancestor_containing_its_head() {
        // A = origin: head A1, also contains the shared message S.
        // B = copy:   head S (the fork point), plus its own new message B2.
        let a = SessionUuids { id: u(0xA), head: Some(u(1)), uuids: set(&[1, 5]), forked: false };
        let b = SessionUuids { id: u(0xB), head: Some(u(5)), uuids: set(&[5, 2]), forked: false };
        let links = copy_lineage(&[a, b]);
        assert_eq!(links.get(&u(0xB)), Some(&(u(0xA), 1)));
        assert_eq!(links.get(&u(0xA)), None);
    }

    #[test]
    fn no_overlap_no_link() {
        let a = SessionUuids { id: u(0xA), head: Some(u(1)), uuids: set(&[1, 2]), forked: false };
        let b = SessionUuids { id: u(0xB), head: Some(u(3)), uuids: set(&[3, 4]), forked: false };
        let links = copy_lineage(&[a, b]);
        assert!(links.is_empty());
    }

    #[test]
    fn explicit_fork_is_skipped() {
        // B would otherwise link to A, but it already has an explicit forked_from.
        let a = SessionUuids { id: u(0xA), head: Some(u(1)), uuids: set(&[1, 5]), forked: false };
        let b = SessionUuids { id: u(0xB), head: Some(u(5)), uuids: set(&[5, 2]), forked: true };
        let links = copy_lineage(&[a, b]);
        assert_eq!(links.get(&u(0xB)), None);
    }

    #[test]
    fn shared_start_is_not_a_link() {
        // Two siblings share the same head (5) and overlap, but neither contains
        // the other's head as a non-root, so neither is the other's ancestor.
        let a = SessionUuids { id: u(0xA), head: Some(u(5)), uuids: set(&[5, 1]), forked: false };
        let b = SessionUuids { id: u(0xB), head: Some(u(5)), uuids: set(&[5, 2]), forked: false };
        let links = copy_lineage(&[a, b]);
        assert!(links.is_empty());
    }

    #[test]
    fn descendant_larger_than_ancestor_still_links() {
        // Ancestor A is small (head A1 + shared S). Descendant B copied S then grew
        // bigger. Direction must come from head-containment, not size.
        let a = SessionUuids { id: u(0xA), head: Some(u(1)), uuids: set(&[1, 5]), forked: false };
        let b =
            SessionUuids { id: u(0xB), head: Some(u(5)), uuids: set(&[5, 2, 3, 4]), forked: false };
        let links = copy_lineage(&[a, b]);
        assert_eq!(links.get(&u(0xB)), Some(&(u(0xA), 1)));
        assert_eq!(links.get(&u(0xA)), None);
    }
}
