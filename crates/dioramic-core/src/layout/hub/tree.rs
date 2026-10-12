//! The zone tree seen by the hub layout: every container (a zone, or the root) holds members, and a member is
//! either a node or a nested zone.

use std::collections::BTreeMap;

use dioramic_spec::Model;

use super::super::arrange::valid_parents;

/// Something placed inside a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Member {
    /// A node, by index in `model.nodes`.
    Node(usize),
    /// A zone, by index in `model.zones`.
    Zone(usize),
}

/// A place that holds members.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Container {
    /// The plan itself: top-level zones and zone-less nodes.
    Root,
    /// A zone.
    Zone(usize),
}

/// Zones, their members and the zone of every node.
#[derive(Debug, Clone)]
pub(super) struct ZoneTree {
    /// Valid parent of each zone (unknown or cyclic parents make it top-level).
    pub parent: Vec<Option<usize>>,
    /// Members of each zone: its nodes (spec order), then its nested zones (spec order).
    zone_members: Vec<Vec<Member>>,
    /// Members of the root: top-level zones (spec order), then zone-less nodes (spec order).
    root_members: Vec<Member>,
    /// Zone directly holding each node.
    pub node_zone: Vec<Option<usize>>,
}

impl ZoneTree {
    /// Indexes the zone tree of `model`.
    pub fn new(model: &Model) -> Self {
        let zone_index: BTreeMap<&str, usize> =
            model.zones.iter().enumerate().map(|(index, zone)| (zone.id.as_str(), index)).collect();
        let parent = valid_parents(model, &zone_index);
        let node_zone: Vec<Option<usize>> = model
            .nodes
            .iter()
            .map(|node| node.zone.as_ref().and_then(|zone| zone_index.get(zone.as_str()).copied()))
            .collect();
        let mut zone_members = vec![Vec::new(); model.zones.len()];
        let mut root_members: Vec<Member> = parent
            .iter()
            .enumerate()
            .filter(|(_, parent)| parent.is_none())
            .map(|(zone, _)| Member::Zone(zone))
            .collect();
        for (node, zone) in node_zone.iter().enumerate() {
            match zone {
                Some(zone) => zone_members[*zone].push(Member::Node(node)),
                None => root_members.push(Member::Node(node)),
            }
        }
        for (zone, parent) in parent.iter().enumerate() {
            if let Some(parent) = parent {
                zone_members[*parent].push(Member::Zone(zone));
            }
        }
        Self { parent, zone_members, root_members, node_zone }
    }

    /// Members of a container.
    pub fn members(&self, container: Container) -> &[Member] {
        match container {
            Container::Root => &self.root_members,
            Container::Zone(zone) => &self.zone_members[zone],
        }
    }

    /// Every node under a member (the node itself, or the nodes of the zone and its nested zones).
    pub fn subtree_nodes(&self, member: Member) -> Vec<usize> {
        let mut nodes = Vec::new();
        self.collect_nodes(member, &mut nodes);
        nodes
    }

    fn collect_nodes(&self, member: Member, into: &mut Vec<usize>) {
        match member {
            Member::Node(node) => into.push(node),
            Member::Zone(zone) => {
                for &child in &self.zone_members[zone] {
                    self.collect_nodes(child, into);
                }
            }
        }
    }

    /// The root member a node belongs to: its top-level zone, or the node itself when it has no zone.
    pub fn top_member(&self, node: usize) -> Member {
        match self.node_zone[node] {
            None => Member::Node(node),
            Some(mut zone) => {
                while let Some(parent) = self.parent[zone] {
                    zone = parent;
                }
                Member::Zone(zone)
            }
        }
    }

    /// Containers with nested ones first (post-order) and the root last.
    pub fn bottom_up(&self) -> Vec<Container> {
        let mut order = Vec::with_capacity(self.zone_members.len() + 1);
        for &member in &self.root_members {
            self.post_order(member, &mut order);
        }
        order.push(Container::Root);
        order
    }

    fn post_order(&self, member: Member, into: &mut Vec<Container>) {
        if let Member::Zone(zone) = member {
            for &child in &self.zone_members[zone] {
                self.post_order(child, into);
            }
            into.push(Container::Zone(zone));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::fixtures::{ModelBuilder, zone};

    #[test]
    fn members_post_order_and_top_members() {
        let mut inner = zone("inner");
        inner.parent = Some("outer".into());
        let model = ModelBuilder::new()
            .zone(zone("outer"))
            .zone(inner)
            .nodes_in("outer", &["a"])
            .nodes_in("inner", &["b"])
            .nodes(&["free"])
            .build();
        let tree = ZoneTree::new(&model);
        assert_eq!(tree.members(Container::Root), &[Member::Zone(0), Member::Node(2)]);
        assert_eq!(tree.members(Container::Zone(0)), &[Member::Node(0), Member::Zone(1)]);
        assert_eq!(tree.bottom_up(), vec![Container::Zone(1), Container::Zone(0), Container::Root]);
        assert_eq!(tree.subtree_nodes(Member::Zone(0)), vec![0, 1]);
        assert_eq!(tree.top_member(1), Member::Zone(0));
        assert_eq!(tree.top_member(2), Member::Node(2));
    }
}
