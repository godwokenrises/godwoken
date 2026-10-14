//! Source-preserving receipt routing. Integer rounding happens at each approved
//! protocol boundary; held weights are never removed from a denominator.
use anyhow::{ensure, Result};
use num_bigint::BigUint as B;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Node {
    pub owner: String,
    pub token: String,
}
#[derive(Debug)]
pub struct Edge {
    pub node: Node,
    pub weight: B,
}
#[derive(Debug)]
struct Rule {
    denominator: B,
    edges: Vec<Edge>,
    reason: String,
}
#[derive(Debug)]
pub struct Lot {
    pub node: Node,
    pub amount: B,
    pub status: String,
    pub path: Vec<Node>,
}
#[derive(Default)]
pub struct Graph {
    rules: BTreeMap<Node, Rule>,
}
impl Graph {
    pub fn add(
        &mut self,
        node: Node,
        denominator: B,
        edges: Vec<Edge>,
        reason: impl Into<String>,
    ) -> Result<()> {
        let mut seen = BTreeSet::new();
        for e in &edges {
            ensure!(seen.insert(&e.node), "Duplicate edge recipient");
        }
        let sum = edges.iter().map(|e| &e.weight).sum::<B>();
        ensure!(
            sum <= denominator,
            "Receipt weights exceed full denominator"
        );
        ensure!(!self.rules.contains_key(&node), "Duplicate ownership rule");
        self.rules.insert(
            node,
            Rule {
                denominator,
                edges,
                reason: reason.into(),
            },
        );
        Ok(())
    }
    pub fn allocate(
        &self,
        node: Node,
        amount: B,
        terminal: impl Fn(&Node) -> Option<String>,
    ) -> Result<Vec<Lot>> {
        let mut lots = vec![];
        let mut path = vec![];
        self.walk(&node, &amount, &terminal, &mut path, &mut lots)?;
        ensure!(
            lots.iter().map(|l| &l.amount).sum::<B>() == amount,
            "Source conservation failed"
        );
        Ok(lots)
    }
    fn walk(
        &self,
        node: &Node,
        amount: &B,
        terminal: &impl Fn(&Node) -> Option<String>,
        path: &mut Vec<Node>,
        lots: &mut Vec<Lot>,
    ) -> Result<()> {
        if *amount == B::default() {
            return Ok(());
        }
        ensure!(
            path.len() < 32 && !path.contains(node),
            "Ownership cycle or excessive depth"
        );
        path.push(node.clone());
        if let Some(status) = terminal(node) {
            lots.push(Lot {
                node: node.clone(),
                amount: amount.clone(),
                status,
                path: path.clone(),
            });
        } else if let Some(rule) = self.rules.get(node).or_else(|| {
            self.rules.get(&Node {
                owner: node.owner.clone(),
                token: "*".into(),
            })
        }) {
            let mut used = B::default();
            if rule.denominator != B::default() {
                for edge in &rule.edges {
                    let share = amount * &edge.weight / &rule.denominator;
                    used += &share;
                    self.walk(&edge.node, &share, terminal, path, lots)?;
                }
            }
            ensure!(used <= *amount, "Source overdraw");
            let remaining = amount - &used;
            if remaining != B::default() {
                lots.push(Lot {
                    node: node.clone(),
                    amount: remaining,
                    status: format!("RETAINED:{}", rule.reason),
                    path: path.clone(),
                });
            }
        } else {
            lots.push(Lot {
                node: node.clone(),
                amount: amount.clone(),
                status: "UNRESOLVED".into(),
                path: path.clone(),
            });
        }
        path.pop();
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn node(s: &str) -> Node {
        Node {
            owner: s.into(),
            token: "T".into(),
        }
    }
    fn edge(s: &str, n: u8) -> Edge {
        Edge {
            node: node(s),
            weight: n.into(),
        }
    }
    #[test]
    fn no_renormalization_or_cross_source_subsidy() {
        let mut g = Graph::default();
        g.add(
            node("pool"),
            3u8.into(),
            vec![edge("user", 1)],
            "unclaimed shares",
        )
        .unwrap();
        let a = g
            .allocate(node("pool"), 10u8.into(), |n| {
                (n.owner == "user").then(|| "REFUND".into())
            })
            .unwrap();
        assert_eq!(a[0].amount, B::from(3u8));
        assert_eq!(a[1].amount, B::from(7u8));
        let b = g
            .allocate(node("pool"), 2u8.into(), |n| {
                (n.owner == "user").then(|| "REFUND".into())
            })
            .unwrap();
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].amount, B::from(2u8));
    }
    #[test]
    fn reject_duplicate_rules_edges_overweights_and_cycles() {
        let mut g = Graph::default();
        assert!(g
            .add(node("p"), 1u8.into(), vec![edge("x", 2)], "x")
            .is_err());
        assert!(g
            .add(node("p"), 2u8.into(), vec![edge("x", 1), edge("x", 1)], "x")
            .is_err());
        g.add(node("p"), 1u8.into(), vec![edge("q", 1)], "x")
            .unwrap();
        assert!(g.add(node("p"), 1u8.into(), vec![], "x").is_err());
        g.add(node("q"), 1u8.into(), vec![edge("p", 1)], "x")
            .unwrap();
        assert!(g.allocate(node("p"), 1u8.into(), |_| None).is_err());
    }
}
