#![feature(int_roundings)]
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

#[ink::contract(env=pink_extension::PinkEnvironment)]
mod tokenomic_contract {
    use alloc::{format, string::String, vec::Vec};
    use chrono::{DateTime, NaiveDateTime, Utc};
    use fixed::types::U64F64;
    use fixed_macro::fixed;
    use pink_extension::{
        chain_extension::{signing, SigType},
        http_get,
    };
    use pink_json::from_slice;
    use pink_subrpc::{
        create_transaction, get_storage, send_transaction, storage::storage_prefix, ExtraParam,
    };
    use scale::{Decode, Encode};
    use serde::Deserialize;

    const PERIOD: u64 = 24 * 60 * 60 * 1000; // 24 hours
    const ONE_MINUTE: u64 = 60 * 1000;
    const ONE_MINUTE_BUDGET: u64 = 500;
    const ONE_PERIOD_BUDGET: u64 = ONE_MINUTE_BUDGET * (PERIOD / ONE_MINUTE);
    const NONCE_OFFSET: i64 = -19500; // based on contract first run date
    const HALVING_PERIOD: i64 = 180 * 24 * 60 * 60 * 1000; // 180 days
    const HALVING_START_INDEX: i64 = 1;
    const HALVING_RATIO: U64F64 = fixed!(0.75: U64F64);
    const HALVING_START_TIME: i64 = 1_686_528_000_000; // UNIX timestamp for 2023-06-12T00:00:00.000Z

    fn pow(x: U64F64, n: u32) -> U64F64 {
        let mut i = n;
        let mut x_pow2 = x;
        let mut z = fixed!(1: U64F64);
        while i > 0 {
            if i & 1 == 1 {
                z *= x_pow2;
            }
            x_pow2 *= x_pow2;
            i >>= 1;
        }
        z
    }

    #[derive(Debug, PartialEq, Eq, Encode, Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub enum Error {
        InvalidStorage,
        HttpRequestFailed,
        InvalidResponseBody,
        BlockNotFound,
        SharesNotFound,
        InvalidNonce,
    }

    /// Type alias for the contract's result type.
    pub type Result<T, E = Error> = core::result::Result<T, E>;

    #[ink(storage)]
    pub struct Tokenomic {
        pub executor_account: [u8; 32],
        executor_private_key: [u8; 32],
    }

    #[derive(Deserialize, Encode, Clone, Debug, PartialEq)]
    struct GraphQLResponse<T> {
        data: T,
    }

    #[derive(Deserialize, Encode, Clone, Debug, PartialEq)]
    pub struct BlockNode {
        timestamp: String,
        height: u32,
    }

    #[derive(Deserialize, Encode, Clone, Debug, PartialEq)]
    struct ConnectionEdge<T> {
        node: T,
    }

    #[derive(Deserialize, Encode, Clone, Debug, PartialEq)]
    struct BlocksConnection {
        edges: Vec<ConnectionEdge<BlockNode>>,
    }

    #[derive(Deserialize, Encode, Clone, Debug, PartialEq)]
    #[serde(rename_all = "camelCase")]
    struct BlocksData {
        blocks_connection: BlocksConnection,
    }

    #[derive(Deserialize, Encode, Clone, Debug, PartialEq)]
    pub struct SnapshotNode {
        shares: String,
    }

    #[derive(Deserialize, Encode, Clone, Debug, PartialEq)]
    struct SnapshotsConnection {
        edges: Vec<ConnectionEdge<SnapshotNode>>,
    }

    #[derive(Deserialize, Encode, Clone, Debug, PartialEq)]
    #[serde(rename_all = "camelCase")]
    struct SnapshotData {
        worker_shares_snapshots_connection: SnapshotsConnection,
    }

    pub struct Block {
        timestamp: String,
        height: u32,
    }

    pub struct Chain {
        pub name: String,
        pub endpoint: String,
        pub archive_url: String,
        pub squid_url: String,
        pub nonce: u64,
        pub period_last_block: u32,
        pub period_block_count: u32,
        pub shares: U64F64,
        pub budget_per_block: U64F64,
    }

    fn timestamp_to_iso(timestamp: i64) -> String {
        let date_time = DateTime::<Utc>::from_utc(
            NaiveDateTime::from_timestamp_millis(timestamp).unwrap(),
            Utc,
        );

        date_time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }

    fn fetch_nonce(endpoint: String) -> Result<u64> {
        let raw_storage_result = get_storage(
            endpoint.as_str(),
            &storage_prefix("PhalaComputation", "BudgetUpdateNonce"),
            None,
        )
        .or(Err(Error::HttpRequestFailed))?;

        if let Some(raw_storage) = raw_storage_result {
            let nonce = scale::Decode::decode(&mut raw_storage.as_slice())
                .or(Err(Error::InvalidStorage))?;
            Ok(nonce)
        } else {
            Ok(0)
        }
    }

    impl Chain {
        pub fn new(name: String, endpoint: String, archive_url: String, squid_url: String) -> Self {
            let endpoint_clone = endpoint.clone();
            Self {
                name,
                endpoint,
                archive_url,
                squid_url,
                nonce: fetch_nonce(endpoint_clone).unwrap(),
                period_last_block: 0,
                period_block_count: 0,
                shares: fixed!(0: U64F64),
                budget_per_block: fixed!(0: U64F64),
            }
        }

        fn fetch_shares_by_timestamp(&mut self, timestamp: u64) -> Result<U64F64> {
            let resp = http_get!(format!("{}?query=%7B%20workerSharesSnapshotsConnection(orderBy%3A%20updatedTime_ASC%2C%20first%3A%201%2C%20where%3A%20%7BupdatedTime_gte%3A%20%22{}%22%7D)%20%7B%20edges%20%7B%20node%20%7B%20shares%20%7D%20%7D%20%7D%20%7D%0A", self.squid_url, timestamp_to_iso(timestamp as i64)));
            if resp.status_code != 200 {
                return Err(Error::HttpRequestFailed);
            }
            let result: GraphQLResponse<SnapshotData> = from_slice(&resp.body).unwrap();

            match result.data.worker_shares_snapshots_connection.edges.len() {
                0 => return Err(Error::SharesNotFound),
                _ => Ok({
                    let node = result.data.worker_shares_snapshots_connection.edges[0]
                        .node
                        .clone();

                    let shares = U64F64::from_str(&node.shares).unwrap();
                    self.shares = shares;
                    shares
                }),
            }
        }

        pub fn send_extrinsic(&self, signer: [u8; 32], nonce: u64) {
            if nonce > self.nonce {
                let signed_tx = create_transaction(
                    &signer,
                    &self.name,
                    &self.endpoint,
                    0x57_u8,
                    0x07_u8,
                    (
                        nonce,
                        self.period_last_block,
                        self.budget_per_block.to_bits(),
                    ),
                    ExtraParam::default(),
                )
                .unwrap();
                send_transaction(&self.endpoint, &signed_tx).unwrap_or_default();
            }
        }

        fn fetch_block(&self, query: String) -> Result<Block> {
            let resp = http_get!(format!("{}?query={}", self.archive_url, query));

            if resp.status_code != 200 {
                return Err(Error::HttpRequestFailed);
            }

            let result: GraphQLResponse<BlocksData> = from_slice(&resp.body)
                .or(Err(Error::InvalidResponseBody))
                .unwrap();

            match result.data.blocks_connection.edges.len() {
                0 => return Err(Error::BlockNotFound),
                _ => Ok({
                    let node = result.data.blocks_connection.edges[0].node.clone();
                    Block {
                        timestamp: String::from(node.timestamp),
                        height: node.height,
                    }
                }),
            }
        }

        pub fn fetch_block_by_timestamp(&self, timestamp: u64) -> Result<Block> {
            self.fetch_block(format!("%7B%20blocksConnection(orderBy%3A%20timestamp_ASC%20first%3A%201%20where%3A%20%7Btimestamp_gte%3A%20%22{}%22%7D%20)%20%7B%20edges%20%7B%20node%20%7B%20timestamp%20height%20%7D%20%7D%20%7D%20%7D", timestamp_to_iso(timestamp as i64)))
        }

        pub fn fetch_latest_block(&self) -> Result<Block> {
            self.fetch_block(String::from("%7B%20blocksConnection(orderBy%3A%20timestamp_DESC%20first%3A%201)%20%7B%20edges%20%7B%20node%20%7B%20timestamp%20height%20%7D%20%7D%20%7D%20%7D"))
        }

        pub fn fetch_period_block_count(&mut self, end_time: u64, period: u64) -> Result<u32> {
            let start_time = end_time - period;
            let start_block = self.fetch_block_by_timestamp(start_time).unwrap();
            let end_block = self.fetch_block_by_timestamp(end_time).unwrap();
            self.period_last_block = end_block.height;
            let count = end_block.height - start_block.height;
            self.period_block_count = count;
            Ok(count)
        }

        pub fn set_budget_per_block(&mut self, total_shares: U64F64, halving: U64F64) {
            self.budget_per_block =
                self.shares / total_shares / U64F64::from_num(self.period_block_count)
                    * U64F64::from_num(ONE_PERIOD_BUDGET)
                    * halving
        }
    }

    impl Tokenomic {
        /// Constructor to initializes your contract
        #[ink(constructor)]
        pub fn new() -> Self {
            let private_key =
                pink_web3::keys::pink::KeyPair::derive_keypair(b"executor").private_key();
            let account32: [u8; 32] = signing::get_public_key(&private_key, SigType::Sr25519)
                .try_into()
                .unwrap();

            Self {
                executor_account: account32,
                executor_private_key: private_key,
            }
        }

        #[ink(message)]
        pub fn get_executor_account(&self) -> String {
            hex::encode(self.executor_account)
        }

        #[ink(message)]
        pub fn balance(&self) -> Result<(u128, u128)> {
            let mut phala = Chain::new(
                String::from("Phala"),
                String::from("https://phala.api.onfinality.io/public-ws"),
                String::from("https://phala.explorer.subsquid.io/graphql"),
                String::from("https://squid.subsquid.io/phala-computation-lite/graphql"),
            );

            let mut khala = Chain::new(
                String::from("Khala"),
                String::from("https://khala.api.onfinality.io/public-ws"),
                String::from("https://khala.explorer.subsquid.io/graphql"),
                String::from("https://squid.subsquid.io/khala-computation-lite/graphql"),
            );

            let phala_latest_block = phala.fetch_latest_block().unwrap();
            let timestamp = DateTime::parse_from_rfc3339(&phala_latest_block.timestamp)
                .unwrap()
                .timestamp_millis() as u64;
            let halving_index = HALVING_START_INDEX
                + (timestamp as i64 - HALVING_START_TIME).div_ceil(HALVING_PERIOD);
            let halving = pow(HALVING_RATIO, halving_index as u32);
            let period_index = timestamp / PERIOD;
            let period_end = period_index * PERIOD;
            let nonce = u64::try_from(period_index as i64 + NONCE_OFFSET).unwrap();
            if phala.nonce >= nonce && khala.nonce >= nonce {
                return Err(Error::InvalidNonce);
            }

            phala.fetch_period_block_count(period_end, PERIOD).unwrap();
            khala.fetch_period_block_count(period_end, PERIOD).unwrap();

            let phala_shares = phala.fetch_shares_by_timestamp(period_end).unwrap();
            let khala_shares = khala.fetch_shares_by_timestamp(period_end).unwrap();
            let total_shares = phala_shares + khala_shares;
            phala.set_budget_per_block(total_shares, halving);
            khala.set_budget_per_block(total_shares, halving);

            khala.send_extrinsic(self.executor_private_key, nonce);
            phala.send_extrinsic(self.executor_private_key, nonce);

            Ok((
                phala.budget_per_block.to_bits(),
                khala.budget_per_block.to_bits(),
            ))
        }
    }

    /// Unit tests in Rust are normally defined within such a `#[cfg(test)]`
    /// module and test functions are marked with a `#[test]` attribute.
    /// The below code is technically just normal Rust code.
    #[cfg(test)]
    mod tests {
        /// Imports all the definitions from the outer scope so we can use them here.
        use super::*;
        use ink;

        /// We test a simple use case of our contract.
        #[ink::test]
        fn it_works() {
            // when your contract is really deployed, the Phala Worker will do the HTTP requests
            // mock is needed for local test
            pink_extension_runtime::mock_ext::mock_all_ext();

            let tokenomic = Tokenomic::new();

            let account = tokenomic.get_executor_account();
            println!("executor account: {}", account);
            let result = tokenomic.balance().unwrap();
            println!("phala budget: {}", result.0);
            println!("khala budget: {}", result.1);
        }
    }
}
