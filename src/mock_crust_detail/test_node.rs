// Copyright 2018 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::poll;
use chunk_store::Error as ChunkStoreError;
use config_handler::Config;
use hex;
use itertools::Itertools;
use personas::data_manager::DataId;
use rand::{self, Rng};
use routing::mock_crust::{self, Endpoint, Network, ServiceHandle};
use routing::Config as RoutingConfig;
use routing::DevConfig as RoutingDevConfig;
use routing::{BootstrapConfig, RoutingTable, XorName, Xorable};
use std::env;
use std::fs;
use std::path::PathBuf;
use vault::Vault;

/// Test node for mock network
pub struct TestNode {
    /// A handle of this node's mock Crust service
    pub handle: ServiceHandle,
    vault: Vault,
    chunk_store_root: PathBuf,
}

impl TestNode {
    /// create a test node for mock network
    pub fn new(
        network: &Network,
        bootstrap_config: Option<BootstrapConfig>,
        config: Option<Config>,
        first_node: bool,
        use_cache: bool,
    ) -> Self {
        let handle = network.new_service_handle(bootstrap_config, None);
        let routing_config = RoutingConfig {
            dev: Some(RoutingDevConfig {
                min_section_size: Some(network.min_section_size()),
                ..RoutingDevConfig::default()
            }),
        };
        let temp_root = env::temp_dir();

        // Note: using non-deterministic rng here to prevent multiple threads to
        // set the same chunk store root which would cause crash when running tests
        // in parallel.
        let chunk_store_root = temp_root.join(hex::encode(
            rand::thread_rng().gen_iter().take(8).collect::<Vec<u8>>(),
        ));
        let vault_config = match config {
            Some(config) => Config {
                chunk_store_root: Some(format!("{}", chunk_store_root.display())),
                ..config
            },
            None => Config {
                wallet_address: None,
                max_capacity: None,
                chunk_store_root: Some(format!("{}", chunk_store_root.display())),
                invite_key: None,
                dev: None,
            },
        };
        let vault = mock_crust::make_current(&handle, || {
            unwrap!(Vault::new_with_configs(
                first_node,
                use_cache,
                vault_config,
                routing_config,
            ))
        });

        TestNode {
            handle,
            vault,
            chunk_store_root,
        }
    }
    /// Empty the event queue for this node on the mock network
    pub fn poll(&mut self) -> usize {
        let mut result = 0;

        while self.vault.poll() {
            result += 1;
        }

        result
    }

    /// empty this client event loop
    pub fn poll_once(&mut self) -> bool {
        self.vault.poll()
    }

    /// Return endpoint for this node
    pub fn endpoint(&self) -> Endpoint {
        self.handle.endpoint()
    }

    /// Return IDs of all data stored on mock network
    pub fn get_stored_ids_and_versions(&self) -> Result<Vec<(DataId, u64)>, ChunkStoreError> {
        self.vault.get_stored_ids_and_versions()
    }

    /// return the number of mutations performed by the given client
    pub fn get_maid_manager_mutation_count(&self, client_name: &XorName) -> Option<u64> {
        self.vault.get_maid_manager_mutation_count(client_name)
    }

    /// name of vault.
    pub fn name(&self) -> XorName {
        self.vault.name()
    }

    /// returns the vault's routing_table.
    pub fn routing_table(&self) -> &RoutingTable<XorName> {
        self.vault.routing_table()
    }

    /// Set whether `DataManager` group refreshes should be delayed or not on this vault.
    /// Any un-handled delayed group refreshes in the cache will be handled and purged.
    pub fn delay_group_refreshes(&mut self, delayed: bool) {
        self.vault.delay_group_refreshes(delayed)
    }
}

impl Drop for TestNode {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.chunk_store_root);
    }
}

/// Create nodes for mock network
pub fn create_nodes(
    network: &Network,
    size: usize,
    config: Option<Config>,
    use_cache: bool,
) -> Vec<TestNode> {
    let mut nodes = Vec::new();

    // Create the seed node.
    nodes.push(TestNode::new(
        network,
        None,
        config.clone(),
        true,
        use_cache,
    ));
    while nodes[0].poll() > 0 {}

    let bootstrap_config = BootstrapConfig::with_contacts(&[nodes[0].endpoint()]);

    // Create other nodes using the seed node endpoint as bootstrap contact.
    for _ in 1..size {
        // (2nd to Nth node clone the config objects.)
        nodes.push(TestNode::new(
            network,
            Some(bootstrap_config.clone()),
            config.clone(),
            false,
            use_cache,
        ));
        let _ = poll::nodes(&mut nodes);
    }
    drop(config);

    nodes
}

/// Add node to the mock network
pub fn add_node(
    network: &Network,
    nodes: &mut Vec<TestNode>,
    index: usize,
    use_cache: bool,
) {
    let config = BootstrapConfig::with_contacts(&[nodes[index].endpoint()]);
    nodes.push(TestNode::new(network, Some(config), None, false, use_cache));
}

/// Add node to the mock network with specified config
pub fn add_node_with_config(
    network: &Network,
    nodes: &mut Vec<TestNode>,
    config: Config,
    index: usize,
    use_cache: bool,
) {
    let bootstrap_config = BootstrapConfig::with_contacts(&[nodes[index].endpoint()]);
    nodes.push(TestNode::new(
        network,
        Some(bootstrap_config),
        Some(config),
        false,
        use_cache,
    ));
}

/// remove this node from the mock network
pub fn drop_node(nodes: &mut Vec<TestNode>, index: usize) {
    let node = nodes.remove(index);
    trace!("Removing node: {:?}", node.name());
    drop(node);
}

/// Process all events
fn _poll_all(nodes: &mut [TestNode]) {
    while nodes.iter_mut().any(|node| node.poll() > 0) {}
}

/// Get `count` closest nodes to the given name.
pub fn closest_to<'a, 'b>(
    nodes: &'a [TestNode],
    name: &'b XorName,
    count: usize,
) -> Vec<&'a TestNode> {
    let mut sorted = nodes
        .iter()
        .sorted_by(|left, right| name.cmp_distance(&left.name(), &right.name()));
    sorted.truncate(count);
    sorted
}
