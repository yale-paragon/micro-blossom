//! Register Transfer Level (RTL) Dual Module
//!
//! This is a software implementation of the Micro Blossom algorithm.
//! We assume all the vertices and edges have a single clock source.
//! On every clock cycle, each vertex/edge generates a new vertex/edge as the register state for the next clock cycle.
//! This directly corresponds to an RTL design in HDL language, but without any optimizations.
//! This is supposed to be an algorithm design for Micro Blossom.
//!

use fusion_blossom::dual_module::*;
use fusion_blossom::util::*;
use fusion_blossom::visualize::*;

#[derive(Debug)]
pub struct DualModuleRTL {
    // always reconstruct the whole graph when reset
    pub initializer: SolverInitializer,
    pub vertices: Vec<Vertex>,
    pub edges: Vec<Edge>,
    pub nodes: Vec<DualNodePtr>,
    /// temporary list of synchronize requests, not used until hardware fusion
    pub sync_requests: Vec<SyncRequest>,
}

pub enum Instruction {
    SetSpeed { node: NodeIndex, speed: DualNodeGrowState },
    SetBlossom { node: NodeIndex, blossom: NodeIndex },
    AddDefectVertex { vertex: VertexIndex, node: NodeIndex },
    FindObstacle { region_preference: usize },
    Grow { length: Weight },
}

pub enum Response {
    NonZeroGrow {
        length: Weight,
    },
    Conflict {
        node_1: NodeIndex,
        node_2: NodeIndex,
        touch_1: NodeIndex,
        touch_2: NodeIndex,
        vertex_1: VertexIndex,
        vertex_2: VertexIndex,
    },
    ConflictVirtual {
        node: NodeIndex,
        touch: NodeIndex,
        vertex: VertexIndex,
        virtual_vertex: VertexIndex,
    },
    BlossomNeedExpand {
        blossom: NodeIndex,
    },
}

impl Response {
    pub fn reduce(resp1: Option<Response>, resp2: Option<Response>) -> Option<Response> {
        None // TODO
    }
}

pub fn get_blossom_roots(dual_node_ptr: &DualNodePtr) -> Vec<NodeIndex> {
    let node = dual_node_ptr.read_recursive();
    match &node.class {
        DualNodeClass::Blossom { nodes_circle, .. } => {
            let mut node_indices = vec![];
            for node_ptr in nodes_circle.iter() {
                node_indices.append(&mut get_blossom_roots(&node_ptr.upgrade_force()));
            }
            node_indices
        }
        DualNodeClass::DefectVertex { .. } => vec![node.index],
    }
}

impl DualModuleImpl for DualModuleRTL {
    fn new_empty(initializer: &SolverInitializer) -> Self {
        let mut dual_module = DualModuleRTL {
            initializer: initializer.clone(),
            vertices: vec![],
            edges: vec![],
            nodes: vec![],
            sync_requests: vec![],
        };
        dual_module.clear();
        dual_module
    }

    fn clear(&mut self) {
        // set vertices
        self.vertices = (0..self.initializer.vertex_num)
            .map(|vertex_index| Vertex {
                vertex_index,
                edge_indices: vec![],
                speed: DualNodeGrowState::Stay,
                grown: 0,
                is_virtual: false,
                is_defect: false,
                node_index: None,
                root_index: None,
            })
            .collect();
        // set virtual vertices
        for &virtual_vertex in self.initializer.virtual_vertices.iter() {
            self.vertices[virtual_vertex].is_virtual = true;
        }
        // set edges
        self.edges.clear();
        for (edge_index, &(i, j, weight)) in self.initializer.weighted_edges.iter().enumerate() {
            self.edges.push(Edge {
                edge_index,
                weight,
                left_index: i,
                right_index: j,
                left_growth: 0,
                right_growth: 0,
            });
            for vertex_index in [i, j] {
                self.vertices[vertex_index].edge_indices.push(edge_index);
            }
        }
        // each vertex must have at least one incident edge
        for vertex in self.vertices.iter() {
            assert!(!vertex.edge_indices.is_empty());
        }
        // clear nodes
        self.nodes.clear();
    }

    fn add_dual_node(&mut self, dual_node_ptr: &DualNodePtr) {
        let node = dual_node_ptr.read_recursive();
        assert_eq!(node.index, self.nodes.len());
        self.nodes.push(dual_node_ptr.clone());
        match &node.class {
            DualNodeClass::Blossom { nodes_circle, .. } => {
                // creating blossom is cheap
                for weak_ptr in nodes_circle.iter() {
                    let node_index = weak_ptr.upgrade_force().read_recursive().index;
                    self.execute_instruction(Instruction::SetBlossom {
                        node: node_index,
                        blossom: node.index,
                    });
                }
                // TODO: use priority queue to track shrinking blossom constraint
            }
            DualNodeClass::DefectVertex { defect_index } => {
                assert!(!self.vertices[*defect_index].is_defect, "cannot set defect twice");
                self.execute_instruction(Instruction::AddDefectVertex {
                    vertex: *defect_index,
                    node: node.index,
                });
            }
        }
    }

    fn remove_blossom(&mut self, dual_node_ptr: DualNodePtr) {
        // remove blossom is expensive because the vertices doesn't remember all the chain of blossom
        let node = dual_node_ptr.read_recursive();
        let nodes_circle = match &node.class {
            DualNodeClass::Blossom { nodes_circle, .. } => nodes_circle.clone(),
            _ => unreachable!(),
        };
        for weak_ptr in nodes_circle.iter() {
            let node_ptr = weak_ptr.upgrade_force();
            let roots = get_blossom_roots(&node_ptr);
            let blossom_index = node_ptr.read_recursive().index;
            for &root_index in roots.iter() {
                self.execute_instruction(Instruction::SetBlossom {
                    node: root_index,
                    blossom: blossom_index,
                });
            }
        }
    }

    fn set_grow_state(&mut self, dual_node_ptr: &DualNodePtr, grow_state: DualNodeGrowState) {
        let node_index = dual_node_ptr.read_recursive().index;
        self.execute_instruction(Instruction::SetSpeed {
            node: node_index,
            speed: grow_state,
        });
    }

    fn compute_maximum_update_length(&mut self) -> GroupMaxUpdateLength {
        let return_value = self
            .execute_instruction(Instruction::FindObstacle { region_preference: 0 })
            .unwrap();
        let max_update_length = match return_value {
            Response::NonZeroGrow { length } => MaxUpdateLength::NonZeroGrow((length, false)),
            Response::Conflict {
                node_1,
                node_2,
                touch_1,
                touch_2,
                ..
            } => MaxUpdateLength::Conflicting(
                (self.nodes[node_1].clone(), self.nodes[touch_1].clone()),
                (self.nodes[node_2].clone(), self.nodes[touch_2].clone()),
            ),
            Response::ConflictVirtual {
                node,
                touch,
                virtual_vertex,
                ..
            } => MaxUpdateLength::TouchingVirtual(
                (self.nodes[node].clone(), self.nodes[touch].clone()),
                (virtual_vertex, false),
            ),
            Response::BlossomNeedExpand { blossom } => MaxUpdateLength::BlossomNeedExpand(self.nodes[blossom].clone()),
            _ => unreachable!(),
        };
        let mut group_max_update_length = GroupMaxUpdateLength::new();
        group_max_update_length.add(max_update_length);
        group_max_update_length
    }

    fn grow(&mut self, length: Weight) {
        assert!(length > 0, "RTL design doesn't allow negative growth");
        self.execute_instruction(Instruction::Grow { length });
    }

    fn prepare_nodes_shrink(&mut self, _nodes_circle: &[DualNodePtr]) -> &mut Vec<SyncRequest> {
        self.sync_requests.clear();
        &mut self.sync_requests
    }
}

macro_rules! pipeline_staged {
    ($dual_module:ident, $instruction:ident, $stage_name:ident) => {
        let vertices_next = $dual_module
            .vertices
            .iter()
            .cloned()
            .map(|mut vertex| {
                vertex.$stage_name($dual_module, &$instruction);
                vertex
            })
            .collect();
        let edges_next = $dual_module
            .edges
            .iter()
            .cloned()
            .map(|mut edge| {
                edge.$stage_name($dual_module, &$instruction);
                edge
            })
            .collect();
        $dual_module.vertices = vertices_next;
        $dual_module.edges = edges_next;
    };
}

impl DualModuleRTL {
    fn execute_instruction(&mut self, instruction: Instruction) -> Option<Response> {
        pipeline_staged!(self, instruction, execute_stage);
        pipeline_staged!(self, instruction, update_stage);
        let response = self
            .vertices
            .iter()
            .map(|vertex| vertex.write_stage(self, &instruction))
            .chain(self.edges.iter().map(|edge| edge.write_stage(self, &instruction)))
            .reduce(Response::reduce);
        None
    }
}

pub trait DualPipelined {
    /// load data from
    fn load_stage(&mut self, dual_module: &DualModuleRTL, instruction: &Instruction) {}
    /// execute growth and respond to speed and blossom updates
    fn execute_stage(&mut self, dual_module: &DualModuleRTL, instruction: &Instruction);
    /// update the node according to the updated speed and length after growth
    fn update_stage(&mut self, dual_module: &DualModuleRTL, instruction: &Instruction);
    /// generate a response after the update stage (and optionally, write back to memory)
    fn write_stage(&self, dual_module: &DualModuleRTL, instruction: &Instruction) -> Option<Response>;
}

#[derive(Clone, Debug)]
pub struct Vertex {
    pub vertex_index: VertexIndex,
    pub edge_indices: Vec<EdgeIndex>,
    pub speed: DualNodeGrowState,
    pub grown: Weight,
    pub is_virtual: bool,
    pub is_defect: bool,
    pub node_index: Option<NodeIndex>, // propagated_dual_node
    pub root_index: Option<NodeIndex>, // propagated_grandson_dual_node
}

impl Vertex {
    pub fn get_speed(&self) -> Weight {
        match self.speed {
            DualNodeGrowState::Stay => 0,
            DualNodeGrowState::Shrink => -1,
            DualNodeGrowState::Grow => 1,
        }
    }
}

impl DualPipelined for Vertex {
    fn execute_stage(&mut self, dual_module: &DualModuleRTL, instruction: &Instruction) {
        match instruction {
            Instruction::AddDefectVertex { vertex, node } => {
                if *vertex == self.vertex_index {
                    self.is_defect = true;
                    self.speed = DualNodeGrowState::Grow;
                    self.root_index = Some(*node);
                    self.node_index = Some(*node);
                }
            }
            Instruction::SetSpeed { node, speed } => {
                if Some(*node) == self.node_index {
                    self.speed = *speed;
                }
            }
            Instruction::Grow { length } => {
                self.grown += self.get_speed() * length;
                assert!(self.grown >= 0);
            }
            Instruction::SetBlossom { node, blossom } => {
                if Some(*node) == self.node_index || Some(*node) == self.root_index {
                    self.node_index = Some(*blossom);
                    self.speed = DualNodeGrowState::Grow;
                }
            }
            _ => {}
        }
    }

    fn update_stage(&mut self, dual_module: &DualModuleRTL, instruction: &Instruction) {
        // is there any growing peer trying to propagate to this node?
        let propagating_peer: Option<&Vertex> = {
            // find a peer node with positive growth and fully-grown edge
            self.edge_indices
                .iter()
                .map(|&edge_index| {
                    let edge = &dual_module.edges[edge_index];
                    let peer_index = edge.get_peer(self.vertex_index);
                    let peer = &dual_module.vertices[peer_index];
                    if edge.is_tight_from(peer_index) && peer.speed == DualNodeGrowState::Grow {
                        Some(peer)
                    } else {
                        None
                    }
                })
                .reduce(|a, b| a.or(b))
                .unwrap()
        };
        // is this node contributing to at least one
        if !self.is_defect && !self.is_virtual && self.grown == 0 {
            if let Some(peer) = propagating_peer {
                self.node_index = peer.node_index;
                self.root_index = peer.root_index;
                self.speed = peer.speed;
            } else {
                self.node_index = None;
                self.root_index = None;
                self.speed = DualNodeGrowState::Stay;
            }
        }
    }

    // generate a response
    fn write_stage(&self, dual_module: &DualModuleRTL, instruction: &Instruction) -> Option<Response> {
        // only detect when y_S = 0 and delta y_S = -1, whether there are two growing
        if self.speed != DualNodeGrowState::Shrink {
            return None;
        }
        None
    }
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub edge_index: EdgeIndex,
    pub weight: Weight,
    pub left_index: VertexIndex,
    pub right_index: VertexIndex,
    pub left_growth: Weight,
    pub right_growth: Weight,
}

impl Edge {
    pub fn is_tight(&self) -> bool {
        self.left_growth + self.right_growth >= self.weight
    }

    pub fn get_peer(&self, vertex: VertexIndex) -> VertexIndex {
        if vertex == self.left_index {
            self.right_index
        } else if vertex == self.right_index {
            self.left_index
        } else {
            panic!("vertex is not incident to the edge, cannot get peer")
        }
    }

    pub fn is_tight_from(&self, vertex: VertexIndex) -> bool {
        if vertex == self.left_index {
            self.left_growth == self.weight
        } else if vertex == self.right_index {
            self.right_growth == self.weight
        } else {
            panic!("invalid input: vertex is not incident to the edge")
        }
    }
}

impl DualPipelined for Edge {
    // compute the next register values
    fn execute_stage(&mut self, dual_module: &DualModuleRTL, instruction: &Instruction) {
        match instruction {
            Instruction::Grow { length } => {
                let left_vertex = &dual_module.vertices[self.left_index];
                let right_vertex = &dual_module.vertices[self.right_index];
                if left_vertex.node_index != right_vertex.node_index {
                    self.left_growth += left_vertex.get_speed() * length;
                    self.right_growth += right_vertex.get_speed() * length;
                    assert!(self.left_growth >= 0);
                    assert!(self.right_growth >= 0);
                    assert!(self.left_growth + self.right_growth <= self.weight);
                }
            }
            _ => {}
        }
    }

    fn update_stage(&mut self, dual_module: &DualModuleRTL, instruction: &Instruction) {}

    // generate a response
    fn write_stage(&self, dual_module: &DualModuleRTL, instruction: &Instruction) -> Option<Response> {
        None
    }
}

impl FusionVisualizer for DualModuleRTL {
    #[allow(clippy::unnecessary_cast)]
    fn snapshot(&self, abbrev: bool) -> serde_json::Value {
        let vertices: Vec<serde_json::Value> = self
            .vertices
            .iter()
            .map(|vertex| {
                let mut value = json!({
                    if abbrev { "v" } else { "is_virtual" }: i32::from(vertex.is_virtual),
                    if abbrev { "s" } else { "is_defect" }: i32::from(vertex.is_defect),
                });
                if let Some(node_index) = vertex.node_index.as_ref() {
                    value.as_object_mut().unwrap().insert(
                        (if abbrev { "p" } else { "propagated_dual_node" }).to_string(),
                        json!(node_index),
                    );
                }
                if let Some(root_index) = vertex.root_index.as_ref() {
                    value.as_object_mut().unwrap().insert(
                        (if abbrev { "pg" } else { "propagated_grandson_dual_node" }).to_string(),
                        json!(root_index),
                    );
                }
                value
            })
            .collect();
        let edges: Vec<serde_json::Value> = self
            .edges
            .iter()
            .map(|edge| {
                let mut value = json!({
                    if abbrev { "w" } else { "weight" }: edge.weight,
                    if abbrev { "l" } else { "left" }: edge.left_index,
                    if abbrev { "r" } else { "right" }: edge.right_index,
                    if abbrev { "lg" } else { "left_growth" }: edge.left_growth,
                    if abbrev { "rg" } else { "right_growth" }: edge.right_growth,
                });
                let left_vertex = &self.vertices[edge.left_index];
                let right_vertex = &self.vertices[edge.right_index];
                if let Some(node_index) = left_vertex.node_index.as_ref() {
                    value
                        .as_object_mut()
                        .unwrap()
                        .insert((if abbrev { "ld" } else { "left_dual_node" }).to_string(), json!(node_index));
                }
                if let Some(root_index) = left_vertex.root_index.as_ref() {
                    value.as_object_mut().unwrap().insert(
                        (if abbrev { "lgd" } else { "left_grandson_dual_node" }).to_string(),
                        json!(root_index),
                    );
                }
                if let Some(node_index) = right_vertex.node_index.as_ref() {
                    value
                        .as_object_mut()
                        .unwrap()
                        .insert((if abbrev { "rd" } else { "right_dual_node" }).to_string(), json!(node_index));
                }
                if let Some(root_index) = right_vertex.root_index.as_ref() {
                    value.as_object_mut().unwrap().insert(
                        (if abbrev { "rgd" } else { "right_grandson_dual_node" }).to_string(),
                        json!(root_index),
                    );
                }
                value
            })
            .collect();
        json!({
            "vertices": vertices,
            "edges": edges,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_blossom::example_codes::*;

    // to use visualization, we need the folder of fusion-blossom repo
    // e.g. export FUSION_DIR=/Users/wuyue/Documents/GitHub/fusion-blossom

    #[test]
    fn dual_module_rtl_basic_1() {
        // cargo test dual_module_rtl_basic_1 -- --nocapture
        let visualize_filename = "dual_module_rtl_basic_1.json".to_string();
        let half_weight = 500;
        let mut code = CodeCapacityPlanarCode::new(7, 0.1, half_weight);
        let mut visualizer = Visualizer::new(
            option_env!("FUSION_DIR").map(|dir| dir.to_owned() + "/visualize/data/" + visualize_filename.as_str()),
            code.get_positions(),
            true,
        )
        .unwrap();
        print_visualize_link(visualize_filename.clone());
        // create dual module
        let initializer = code.get_initializer();
        let mut dual_module = DualModuleRTL::new_empty(&initializer);
        // a simple syndrome
        code.vertices[19].is_defect = true;
        code.vertices[25].is_defect = true;
        let interface_ptr = DualModuleInterfacePtr::new_load(&code.get_syndrome(), &mut dual_module);
        visualizer
            .snapshot_combined("syndrome".to_string(), vec![&interface_ptr, &dual_module])
            .unwrap();
        // create dual nodes and grow them by half length
        let dual_node_19_ptr = interface_ptr.read_recursive().nodes[0].clone().unwrap();
        let dual_node_25_ptr = interface_ptr.read_recursive().nodes[1].clone().unwrap();
        dual_module.grow(half_weight);
        visualizer
            .snapshot_combined("grow to 0.5".to_string(), vec![&interface_ptr, &dual_module])
            .unwrap();
        dual_module.grow(half_weight);
        visualizer
            .snapshot_combined("grow to 1".to_string(), vec![&interface_ptr, &dual_module])
            .unwrap();
        dual_module.grow(half_weight);
        visualizer
            .snapshot_combined("grow to 1.5".to_string(), vec![&interface_ptr, &dual_module])
            .unwrap();
        // set to shrink
        dual_module.set_grow_state(&dual_node_19_ptr, DualNodeGrowState::Shrink);
        dual_module.set_grow_state(&dual_node_25_ptr, DualNodeGrowState::Shrink);
        dual_module.grow(half_weight);
        visualizer
            .snapshot_combined("shrink to 1".to_string(), vec![&interface_ptr, &dual_module])
            .unwrap();
        dual_module.grow(half_weight);
        visualizer
            .snapshot_combined("shrink to 0.5".to_string(), vec![&interface_ptr, &dual_module])
            .unwrap();
        dual_module.grow(half_weight);
        visualizer
            .snapshot_combined("shrink to 0".to_string(), vec![&interface_ptr, &dual_module])
            .unwrap();
    }

    #[test]
    fn dual_module_rtl_blossom_basics() {
        // cargo test dual_module_rtl_blossom_basics -- --nocapture
        let visualize_filename = "dual_module_rtl_blossom_basics.json".to_string();
        let half_weight = 500;
        let mut code = CodeCapacityPlanarCode::new(7, 0.1, half_weight);
        let mut visualizer = Visualizer::new(
            option_env!("FUSION_DIR").map(|dir| dir.to_owned() + "/visualize/data/" + visualize_filename.as_str()),
            code.get_positions(),
            true,
        )
        .unwrap();
        print_visualize_link(visualize_filename.clone());
        // create dual module
        let initializer = code.get_initializer();
        let mut dual_module = DualModuleRTL::new_empty(&initializer);
        // try to work on a simple syndrome
        code.vertices[19].is_defect = true;
        code.vertices[26].is_defect = true;
        code.vertices[35].is_defect = true;
        let interface_ptr = DualModuleInterfacePtr::new_load(&code.get_syndrome(), &mut dual_module);
        visualizer
            .snapshot_combined("syndrome".to_string(), vec![&interface_ptr, &dual_module])
            .unwrap();
        // create dual nodes and grow them by half length
        let dual_node_19_ptr = interface_ptr.read_recursive().nodes[0].clone().unwrap();
        let dual_node_26_ptr = interface_ptr.read_recursive().nodes[1].clone().unwrap();
        let dual_node_35_ptr = interface_ptr.read_recursive().nodes[2].clone().unwrap();
        interface_ptr.grow(2 * half_weight, &mut dual_module);
        assert_eq!(interface_ptr.sum_dual_variables(), 6 * half_weight);
        visualizer
            .snapshot_combined("before create blossom".to_string(), vec![&interface_ptr, &dual_module])
            .unwrap();
        let nodes_circle = vec![dual_node_19_ptr.clone(), dual_node_26_ptr.clone(), dual_node_35_ptr.clone()];
        interface_ptr.set_grow_state(&dual_node_26_ptr, DualNodeGrowState::Shrink, &mut dual_module);
        let dual_node_blossom = interface_ptr.create_blossom(nodes_circle, vec![], &mut dual_module);
        interface_ptr.grow(half_weight, &mut dual_module);
        assert_eq!(interface_ptr.sum_dual_variables(), 7 * half_weight);
        visualizer
            .snapshot_combined("blossom grow half weight".to_string(), vec![&interface_ptr, &dual_module])
            .unwrap();
        interface_ptr.grow(half_weight, &mut dual_module);
        assert_eq!(interface_ptr.sum_dual_variables(), 8 * half_weight);
        visualizer
            .snapshot_combined("blossom grow half weight".to_string(), vec![&interface_ptr, &dual_module])
            .unwrap();
        interface_ptr.grow(half_weight, &mut dual_module);
        assert_eq!(interface_ptr.sum_dual_variables(), 9 * half_weight);
        visualizer
            .snapshot_combined("blossom grow half weight".to_string(), vec![&interface_ptr, &dual_module])
            .unwrap();
        interface_ptr.set_grow_state(&dual_node_blossom, DualNodeGrowState::Shrink, &mut dual_module);
        interface_ptr.grow(half_weight, &mut dual_module);
        assert_eq!(interface_ptr.sum_dual_variables(), 8 * half_weight);
        visualizer
            .snapshot_combined("blossom shrink half weight".to_string(), vec![&interface_ptr, &dual_module])
            .unwrap();
        interface_ptr.grow(2 * half_weight, &mut dual_module);
        assert_eq!(interface_ptr.sum_dual_variables(), 6 * half_weight);
        visualizer
            .snapshot_combined("blossom shrink weight".to_string(), vec![&interface_ptr, &dual_module])
            .unwrap();
        interface_ptr.expand_blossom(dual_node_blossom, &mut dual_module);
        interface_ptr.set_grow_state(&dual_node_19_ptr, DualNodeGrowState::Shrink, &mut dual_module);
        interface_ptr.set_grow_state(&dual_node_26_ptr, DualNodeGrowState::Shrink, &mut dual_module);
        interface_ptr.set_grow_state(&dual_node_35_ptr, DualNodeGrowState::Shrink, &mut dual_module);
        interface_ptr.grow(half_weight, &mut dual_module);
        assert_eq!(interface_ptr.sum_dual_variables(), 3 * half_weight);
        visualizer
            .snapshot_combined(
                "individual shrink half weight".to_string(),
                vec![&interface_ptr, &dual_module],
            )
            .unwrap();
    }
}
