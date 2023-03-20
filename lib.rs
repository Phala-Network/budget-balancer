#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

// pink_extension is short for Phala ink! extension
use pink_extension as pink;

#[pink::contract(env=PinkEnvironment)]
mod balance_tokenomic {
    use super::pink;
    use chrono::{DateTime, NaiveDateTime, Utc};
    use ink_prelude::{format, string::String};
    use pink::{http_get, PinkEnvironment};
    use pink_subrpc::{get_storage, storage::storage_prefix};
    use scale::{Decode, Encode};
    use serde::Deserialize;
    use serde_json::from_slice;

    const COMPUTING_PERIOD: i64 = 24 * 60 * 60 * 1000; // 24 hours
    const ONE_DAY: i64 = 24 * 60 * 60 * 1000;
    const ONE_DAY_BUDGET: i64 = 720_000;
    const ONE_PERIOD_BUDGET: f64 = ((COMPUTING_PERIOD / ONE_DAY) * ONE_DAY_BUDGET) as f64;
    const NONCE_OFFSET: i64 = -19430; // based on contract first run date
    const HALVING_PERIOD: i64 = 180 * 24 * 60 * 60 * 1000; // 180 days
    const HALVING_START_INDEX: i64 = 1;
    const HALVING_RATIO: f64 = 0.75;
    const HALVING_START_TIME: i64 = 1_685_923_200_000; // UNIX timestamp for 2023-06-05T00:00:00.000Z

    #[derive(Debug, PartialEq, Eq, Encode, Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub enum Error {
        HttpRequestFailed,
        InvalidResponseBody,
        BlockNotFound,
    }

    /// Type alias for the contract's result type.
    pub type Result<T> = core::result::Result<T, Error>;

    /// Defines the storage of your contract.
    /// All the fields will be encrypted and stored on-chain.
    /// In this stateless example, we just add a useless field for demo.
    #[ink(storage)]
    pub struct BalanceTokenomic {
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

    pub struct Block {
        timestamp: String,
        height: u32,
    }

    pub struct Chain {
        pub name: String,
        pub endpoint: String,
        pub archive_url: String,
        pub nonce: u64,
    }

    fn timestamp_to_iso(timestamp: i64) -> String {
        let date_time = DateTime::<Utc>::from_utc(
            NaiveDateTime::from_timestamp_millis(timestamp).unwrap(),
            Utc,
        );

        // Mock JavaScript's .toISOString()
        date_time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    }

    fn get_budget_per_block(shares: f64, total_shares: f64, block_count: u32, halving: f64) -> f64 {
        shares / total_shares * ONE_PERIOD_BUDGET * halving / block_count as f64
    }

    impl Chain {
        fn fetch_nonce(&mut self) -> Result<u64> {
            // TODO: Use subrpc to get nonce
            let raw_storage = get_storage(
                &self.endpoint,
                &storage_prefix("Aura", "CurrentSlot")[..],
                None,
            )
            .unwrap()
            .unwrap();

            // .log_err("Read storage [sub native balance] failed")
            // .map_err(|_| Error::FetchDataFailed)?
            let nonce = scale::Decode::decode(&mut raw_storage.as_slice()).unwrap();
            // .log_err("Decode storage failed")
            // .map_err(|_| Error::DecodeStorageFailed)?;

            self.nonce = nonce;
            println!("{} nonce: {}", self.name, nonce);
            Ok(nonce)
        }

        fn fetch_shares(&self) -> Result<f64> {
            let shares: f64 = 2.0;
            Ok(shares)
        }

        pub fn send_extrinsic(&self, nonce: u64, budget_per_block: f64) {
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

        pub fn fetch_block_by_timestamp(&self, timestamp: i64) -> Result<Block> {
            self.fetch_block(format!("%7B%20blocksConnection(orderBy%3A%20height_ASC%20first%3A%201%20where%3A%20%7Btimestamp_gte%3A%20%22{}%22%7D%20)%20%7B%20edges%20%7B%20node%20%7B%20timestamp%20height%20%7D%20%7D%20%7D%20%7D", timestamp_to_iso(timestamp)))
        }

        pub fn fetch_latest_block(&self) -> Result<Block> {
            self.fetch_block(String::from("%7B%20blocksConnection(orderBy%3A%20height_DESC%20first%3A%201)%20%7B%20edges%20%7B%20node%20%7B%20timestamp%20height%20%7D%20%7D%20%7D%20%7D"))
        }

        pub fn fetch_period_block_count(&self, end_time: i64, period: i64) -> Result<u32> {
            let start_time = end_time - period;
            let start_block = self.fetch_block_by_timestamp(start_time).unwrap();
            let end_block = self.fetch_block_by_timestamp(end_time).unwrap();
            Ok(end_block.height - start_block.height)
        }
    }

    impl BalanceTokenomic {
        /// Constructor to initializes your contract
        #[ink(constructor)]
        pub fn new() -> Self {
            Self {}
        }

        #[ink(message)]
        pub fn balance(&self) {
            let mut phala = Chain {
                name: String::from("Phala"),
                endpoint: String::from("https://khala.api.onfinality.io/public-ws"),
                archive_url: String::from("https://phala.explorer.subsquid.io/graphql"),
                nonce: u64::MAX,
            };
            let mut khala = Chain {
                name: String::from("Khala"),
                endpoint: String::from("https://khala.api.onfinality.io/public-ws"),
                archive_url: String::from("https://khala.explorer.subsquid.io/graphql"),
                nonce: u64::MAX,
            };
            // phala.fetch_nonce().unwrap();
            // khala.fetch_nonce().unwrap();

            let phala_latest_block = phala.fetch_latest_block().unwrap();
            let timestamp = DateTime::parse_from_rfc3339(&phala_latest_block.timestamp)
                .unwrap()
                .timestamp_millis();
            let halving_index =
                HALVING_START_INDEX + ((timestamp - HALVING_START_TIME) / HALVING_PERIOD);
            let halving = HALVING_RATIO.powi(halving_index as i32);
            let period_index = timestamp / COMPUTING_PERIOD;
            let period_end = period_index * COMPUTING_PERIOD;
            let nonce = u64::try_from(period_index + NONCE_OFFSET).unwrap();
            // if phala.nonce >= nonce || khala.nonce >= nonce {
            //     return;
            // }

            let phala_block_count = phala
                .fetch_period_block_count(period_end, COMPUTING_PERIOD)
                .unwrap();
            let khala_block_count = khala
                .fetch_period_block_count(period_end, COMPUTING_PERIOD)
                .unwrap();

            let phala_shares = phala.fetch_shares().unwrap();
            let khala_shares = khala.fetch_shares().unwrap();
            let total_shares = phala_shares + khala_shares;
            let phala_budget_per_block =
                get_budget_per_block(phala_shares, total_shares, phala_block_count, halving);
            let khala_budget_per_block =
                get_budget_per_block(khala_shares, total_shares, khala_block_count, halving);

            println!("phala_latest_block: {:?}", phala_latest_block.timestamp);
            println!("nonce: {}", nonce);
            println!("halving: {}", halving);
            println!("phala_block_count: {}", phala_block_count);
            println!("khala_block_count: {}", khala_block_count);
            println!("phala_budget_per_block: {}", phala_budget_per_block);
            println!("khala_budget_per_block: {}", khala_budget_per_block);

            phala.send_extrinsic(nonce, phala_budget_per_block);
            khala.send_extrinsic(nonce, khala_budget_per_block);
        }
    }

    /// Unit tests in Rust are normally defined within such a `#[cfg(test)]`
    /// module and test functions are marked with a `#[test]` attribute.
    /// The below code is technically just normal Rust code.
    #[cfg(test)]
    mod tests {
        /// Imports all the definitions from the outer scope so we can use them here.
        use super::*;
        /// Imports `ink_lang` so we can use `#[ink::test]`.
        use ink_lang as ink;

        /// We test a simple use case of our contract.
        #[ink::test]
        fn it_works() {
            // when your contract is really deployed, the Phala Worker will do the HTTP requests
            // mock is needed for local test
            pink_extension_runtime::mock_ext::mock_all_ext();

            let balance_tokenomic = BalanceTokenomic::new();

            balance_tokenomic.balance();
        }
    }
}
