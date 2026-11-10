use moli_layout::FrozenLayoutTree;

use crate::document_runtime::DomHandle;

/// pilot: what a latest layout was built from. Scrolling records no DOM
/// change but moves every scrolled box, so it has its own counter. A viewport
/// resize records neither, yet reflows the page, so the viewport size is part
/// of it too.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct LayoutSourceVersion {
    pub(crate) dom: u64,
    pub(crate) scroll: u64,
    pub(crate) viewport: (u32, u32),
}

struct LatestFrozenLayout {
    document: DomHandle,
    source: LayoutSourceVersion,
    tree: Box<FrozenLayoutTree<DomHandle>>,
}

/// Single-slot storage for the latest successful frozen layout tree.
///
/// This owns exactly one recursively frozen snapshot. It has no separately
/// keyed child tree, working layout world, source index, hit-test index, Taffy
/// cache, style borrow, pass diagnostics, paint snapshot, timer, or invalidation
/// policy. The first geometry demand initializes it. Screenshots, screencast
/// frames and explicit layout refreshes replace
/// this published layout; print projections never enter this cache.
#[derive(Default)]
pub(super) struct LatestLayoutTreeCache {
    latest: Option<LatestFrozenLayout>,
}

impl LatestLayoutTreeCache {
    pub(super) fn get(&self, document: DomHandle) -> Option<&FrozenLayoutTree<DomHandle>> {
        self.latest
            .as_ref()
            .filter(|snapshot| snapshot.document == document)
            .map(|snapshot| snapshot.tree.as_ref())
    }

    pub(super) fn get_for_root(&self, root: DomHandle) -> Option<&FrozenLayoutTree<DomHandle>> {
        self.latest.as_ref()?.tree.tree_for_root(root)
    }

    pub(super) fn publish(
        &mut self,
        document: DomHandle,
        source: LayoutSourceVersion,
        tree: FrozenLayoutTree<DomHandle>,
    ) {
        self.latest = Some(LatestFrozenLayout {
            document,
            source,
            tree: Box::new(tree),
        });
    }

    /// The first geometry demand may run while the document is still parsing;
    /// without a version check that early tree would serve stale or empty
    /// geometry to every later observable query.
    pub(super) fn is_current(&self, document: DomHandle, source: LayoutSourceVersion) -> bool {
        self.latest
            .as_ref()
            .is_some_and(|snapshot| snapshot.document == document && snapshot.source == source)
    }

    pub(super) fn clear(&mut self) {
        self.latest = None;
    }

    #[cfg(test)]
    pub(super) fn observability(
        &self,
    ) -> Option<(DomHandle, moli_layout::LayoutTreeRetentionMetrics)> {
        self.latest
            .as_ref()
            .map(|snapshot| (snapshot.document, snapshot.tree.retention_metrics()))
    }
}
