#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

#[ink::contract(env = pink_extension::PinkEnvironment)]
mod tokenomic {
    use alloc::{format, string::String};
    use chrono::{DateTime, NaiveDateTime, Utc};
    use fixed::types::U64F64;
    use fixed_macro::fixed;
    use pink_extension::http_get;
    use pink_subrpc::{get_storage, storage::storage_prefix};
    use scale::{Decode, Encode};
    use serde::Deserialize;
    use serde_json::from_slice;

    const COMPUTING_PERIOD: u64 = 24 * 60 * 60 * 1000; // 24 hours
    const ONE_MINUTE: u64 = 60 * 1000;
    const ONE_MINUTE_BUDGET: u64 = 500_000_000_000_000;
    const ONE_PERIOD_BUDGET: u64 = ONE_MINUTE_BUDGET * (COMPUTING_PERIOD / ONE_MINUTE);
    const NONCE_OFFSET: i64 = -19430; // based on contract first run date
    const HALVING_PERIOD: i64 = 180 * 24 * 60 * 60 * 1000; // 180 days
    const HALVING_START_INDEX: i64 = 1;
    const HALVING_RATIO: U64F64 = fixed!(0.75: U64F64);
    const HALVING_START_TIME: i64 = 1_685_923_200_000; // UNIX timestamp for 2023-06-05T00:00:00.000Z

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

    /// Defines the storage of your contract.
    /// All the fields will be encrypted and stored on-chain.
    /// In this stateless example, we just add a useless field for demo.
    #[ink(storage)]
    pub struct Tokenomic {
        // TODO: May be generate a new key pair here to send extrinsic
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
        pub period_block_count: u32,
        pub shares: U64F64,
    }

    fn timestamp_to_iso(timestamp: i64) -> String {
        let date_time = DateTime::<Utc>::from_utc(
            NaiveDateTime::from_timestamp_millis(timestamp).unwrap(),
            Utc,
        );

        // Mock JavaScript's .toISOString()
        date_time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    }

    fn fetch_nonce(endpoint: String) -> Result<u64> {
        // TODO: Use subrpc to get nonce
        let raw_storage = get_storage(
            endpoint.as_str(),
            &storage_prefix("Aura", "CurrentSlot")[..],
            None,
        )
        .or(Err(Error::HttpRequestFailed))
        .unwrap()
        .unwrap();

        let nonce = scale::Decode::decode(&mut raw_storage.as_slice())
            .or(Err(Error::InvalidStorage))
            .unwrap();

        Ok(nonce)
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
                period_block_count: 0,
                shares: fixed!(0: U64F64),
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

        pub fn send_extrinsic(&self, nonce: u64, _budget_per_block: U64F64) {
            if nonce > self.nonce {
                // TODO: Use subrpc to send extrinsic here
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
            self.fetch_block(format!("%7B%20blocksConnection(orderBy%3A%20height_ASC%20first%3A%201%20where%3A%20%7Btimestamp_gte%3A%20%22{}%22%7D%20)%20%7B%20edges%20%7B%20node%20%7B%20timestamp%20height%20%7D%20%7D%20%7D%20%7D", timestamp_to_iso(timestamp as i64)))
        }

        pub fn fetch_latest_block(&self) -> Result<Block> {
            self.fetch_block(String::from("%7B%20blocksConnection(orderBy%3A%20height_DESC%20first%3A%201)%20%7B%20edges%20%7B%20node%20%7B%20timestamp%20height%20%7D%20%7D%20%7D%20%7D"))
        }

        pub fn fetch_period_block_count(&mut self, end_time: u64, period: u64) -> Result<u32> {
            let start_time = end_time - period;
            let start_block = self.fetch_block_by_timestamp(start_time).unwrap();
            let end_block = self.fetch_block_by_timestamp(end_time).unwrap();
            let count = end_block.height - start_block.height;
            self.period_block_count = count;
            Ok(count)
        }

        pub fn set_budget_per_block(&self, total_shares: U64F64, halving: U64F64) -> U64F64 {
            self.shares / total_shares / U64F64::from_num(self.period_block_count)
                * U64F64::from_num(ONE_PERIOD_BUDGET)
                * halving
        }
    }

    impl Tokenomic {
        /// Constructor to initializes your contract
        #[ink(constructor)]
        pub fn new() -> Self {
            Self {}
        }

        #[ink(message)]
        pub fn balance(&self) -> Result<(u64, u64)> {
            let mut phala = Chain::new(
                String::from("Phala"),
                // String::from("https://phala.api.onfinality.io/public-ws"),
                String::from("https://pc-test-5.phala.network/phala/rpc"),
                // String::from("https://phala.explorer.subsquid.io/graphql"),
                String::from("http://54.39.243.230:4005/graphql"),
                String::from("http://54.39.243.230:4355/graphql"),
            );
            let mut khala = Chain::new(
                String::from("Khala"),
                // String::from("https://khala.api.onfinality.io/public-ws"),
                String::from("https://pc-test-3.phala.network/khala/rpc"),
                // String::from("https://khala.explorer.subsquid.io/graphql"),
                String::from("http://54.39.243.230:4003/graphql"),
                String::from("http://54.39.243.230:4353/graphql"),
            );

            let phala_latest_block = phala.fetch_latest_block().unwrap();
            let timestamp = DateTime::parse_from_rfc3339(&phala_latest_block.timestamp)
                .unwrap()
                .timestamp_millis() as u64;
            let halving_index =
                HALVING_START_INDEX + ((timestamp as i64 - HALVING_START_TIME) / HALVING_PERIOD);
            let halving = pow(HALVING_RATIO, halving_index as u32);
            let period_index = timestamp / COMPUTING_PERIOD;
            let period_end = period_index * COMPUTING_PERIOD;
            let nonce = u64::try_from(period_index as i64 + NONCE_OFFSET).unwrap();
            // if phala.nonce >= nonce && khala.nonce >= nonce {
            //     return Err(Error::InvalidNonce);
            // }

            let phala_block_count = phala
                .fetch_period_block_count(period_end, COMPUTING_PERIOD)
                .unwrap();
            let khala_block_count = khala
                .fetch_period_block_count(period_end, COMPUTING_PERIOD)
                .unwrap();

            let phala_shares = phala
                .fetch_shares_by_timestamp(timestamp - 10 * 60 * 1000)
                .unwrap();
            let khala_shares = khala
                .fetch_shares_by_timestamp(timestamp - 10 * 60 * 1000)
                .unwrap();
            let total_shares = phala_shares + khala_shares;
            let phala_budget_per_block = phala.set_budget_per_block(total_shares, halving);
            let khala_budget_per_block = khala.set_budget_per_block(total_shares, halving);

            println!("phala_latest_block: {:?}", phala_latest_block.timestamp);
            println!("nonce: {}", nonce);
            println!("halving: {}", halving);
            println!("phala_block_count: {}", phala_block_count);
            println!("khala_block_count: {}", khala_block_count);
            println!("phala_budget_per_block: {}", phala_budget_per_block);
            println!("khala_budget_per_block: {}", khala_budget_per_block);

            phala.send_extrinsic(nonce, phala_budget_per_block);
            khala.send_extrinsic(nonce, khala_budget_per_block);

            Ok((
                phala_budget_per_block.to_num(),
                khala_budget_per_block.to_num(),
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

            tokenomic.balance();
        }
    }
}
