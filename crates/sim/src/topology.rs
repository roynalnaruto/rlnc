//! Random D-regular directed mesh graph generation.
//!
//! Each node in the mesh has exactly `degree` outgoing peers, chosen
//! uniformly at random without self-loops or duplicates. The graph is
//! deterministic given the same RNG state.

use rand::Rng;
use rand::seq::SliceRandom;

/// Index type for nodes in the simulation.
pub type NodeId = usize;

/// A random D-regular directed mesh graph.
///
/// `neighbors[i]` contains the `degree` outgoing peers of node `i`.
pub struct Topology {
    neighbors: Vec<Vec<NodeId>>,
    degree: usize,
}

impl Topology {
    /// Generate a random mesh where each node has exactly `degree`
    /// outgoing peers. No self-loops, no duplicate peers.
    ///
    /// Reachability from node 0 is guaranteed: a random Hamiltonian path
    /// `0 → p[0] → p[1] → ... → p[n-2]` is laid down first, then
    /// remaining degree slots are filled with random peers.
    ///
    /// # Panics
    ///
    /// Panics if `degree >= num_nodes` (not enough distinct peers).
    pub fn random_mesh(num_nodes: usize, degree: usize, rng: &mut impl Rng) -> Self {
        assert!(
            degree < num_nodes,
            "degree ({degree}) must be < num_nodes ({num_nodes})"
        );

        let mut neighbors: Vec<Vec<NodeId>> = vec![Vec::with_capacity(degree); num_nodes];

        // Phase 1: Hamiltonian path guarantees reachability from node 0.
        if num_nodes > 1 {
            let mut perm: Vec<NodeId> = (1..num_nodes).collect();
            perm.shuffle(rng);
            neighbors[0].push(perm[0]);
            for i in 0..perm.len() - 1 {
                neighbors[perm[i]].push(perm[i + 1]);
            }
        }

        // Phase 2: Fill remaining degree slots with random peers.
        let mut candidates: Vec<NodeId> = Vec::with_capacity(num_nodes - 1);
        for (node, peers) in neighbors.iter_mut().enumerate() {
            let remaining = degree - peers.len();
            if remaining == 0 {
                continue;
            }
            candidates.clear();
            candidates.extend(
                (0..node)
                    .chain(node + 1..num_nodes)
                    .filter(|&p| !peers.contains(&p)),
            );
            candidates.partial_shuffle(rng, remaining);
            peers.extend_from_slice(&candidates[..remaining]);
        }

        Self { neighbors, degree }
    }

    /// Number of nodes in the mesh.
    pub fn num_nodes(&self) -> usize {
        self.neighbors.len()
    }

    /// Outgoing peers of node `id`.
    pub fn neighbors(&self, id: NodeId) -> &[NodeId] {
        &self.neighbors[id]
    }

    /// Mesh degree (number of outgoing peers per node).
    pub const fn degree(&self) -> usize {
        self.degree
    }
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    use super::*;

    #[test]
    fn correct_degree() {
        let mut rng = ChaCha20Rng::seed_from_u64(42);
        let topo = Topology::random_mesh(100, 8, &mut rng);

        assert_eq!(topo.num_nodes(), 100);
        assert_eq!(topo.degree(), 8);
        for node in 0..100 {
            assert_eq!(topo.neighbors(node).len(), 8);
        }
    }

    #[test]
    fn no_self_loops() {
        let mut rng = ChaCha20Rng::seed_from_u64(99);
        let topo = Topology::random_mesh(50, 10, &mut rng);

        for node in 0..50 {
            assert!(
                !topo.neighbors(node).contains(&node),
                "node {node} has self-loop"
            );
        }
    }

    #[test]
    fn no_duplicate_peers() {
        let mut rng = ChaCha20Rng::seed_from_u64(77);
        let topo = Topology::random_mesh(50, 10, &mut rng);

        for node in 0..50 {
            let peers = topo.neighbors(node);
            let mut sorted = peers.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), peers.len(), "node {node} has duplicate peers");
        }
    }

    #[test]
    fn reachable_from_proposer() {
        use std::collections::VecDeque;

        let mut rng = ChaCha20Rng::seed_from_u64(123);

        for (num_nodes, degree) in [(5, 1), (10, 2), (20, 4), (50, 3), (100, 8)] {
            let topo = Topology::random_mesh(num_nodes, degree, &mut rng);

            let mut visited = vec![false; num_nodes];
            let mut queue = VecDeque::new();
            visited[0] = true;
            queue.push_back(0);

            while let Some(node) = queue.pop_front() {
                for &peer in topo.neighbors(node) {
                    if !visited[peer] {
                        visited[peer] = true;
                        queue.push_back(peer);
                    }
                }
            }

            let reachable = visited.iter().filter(|&&v| v).count();
            assert_eq!(
                reachable, num_nodes,
                "only {reachable}/{num_nodes} nodes reachable (degree={degree})"
            );
        }
    }

    #[test]
    fn deterministic() {
        let mut rng1 = ChaCha20Rng::seed_from_u64(42);
        let mut rng2 = ChaCha20Rng::seed_from_u64(42);
        let topo1 = Topology::random_mesh(20, 5, &mut rng1);
        let topo2 = Topology::random_mesh(20, 5, &mut rng2);

        for node in 0..20 {
            assert_eq!(topo1.neighbors(node), topo2.neighbors(node));
        }
    }
}
