#![feature(int_roundings)]
#![cfg_attr(not(feature = "std"), no_std, no_main)]
extern crate alloc;

#[ink::contract(env=pink_extension::PinkEnvironment)]
mod budget_balancer {
    use alloc::{format, string::String, vec, vec::Vec};
    use chrono::{DateTime, SecondsFormat};
    use fixed::types::U64F64;
    use fixed_macro::fixed;
    use pink_extension::{
        chain_extension::{signing, SigType},
        http_get, http_post,
    };
    use pink_json::{from_slice, to_vec};
    use pink_subrpc::{
        create_transaction, get_storage, send_transaction, storage::storage_prefix, ExtraParam,
    };
    use scale::{Decode, Encode};
    use serde::{Deserialize, Serialize};

    const PERIOD: u64 = 24 * 60 * 60; // 24 hours
    const ONE_MINUTE: u64 = 60;
    const ONE_MINUTE_BUDGET: u64 = 500;
    const ONE_PERIOD_BUDGET: u64 = ONE_MINUTE_BUDGET * (PERIOD / ONE_MINUTE);
    const NONCE_OFFSET: i64 = -19500; // based on contract first run date
    const HALVING_PERIOD: i64 = 180 * 24 * 60 * 60; // 180 days
    const HALVING_START_INDEX: i64 = 1;
    const HALVING_RATIO: U64F64 = fixed!(0.75: U64F64);
    const HALVING_START_TIME: i64 = 1_686_528_000; // UNIX timestamp for 2023-06-12T00:00:00Z
    const SUBSCAN_API_KEY: &str = "get from https://pro.subscan.io/api_service";

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
        RpcRequestFailed,
        FetchTimestampFailed,
        InvalidTimestamp,
        FetchBlockFailed,
        FetchSharesFailed,
        SharesNotFound,
        InvalidNonce,
        CreateExtrinsicFailed,
        SendExtrinsicFailed,
    }

    /// Type alias for the contract's result type.
    pub type Result<T, E = Error> = core::result::Result<T, E>;

    #[ink(storage)]
    pub struct BudgetBalancer {
        pub executor_account: Vec<u8>,
        executor_private_key: [u8; 32],
    }

    #[derive(Deserialize, Encode, Clone, Debug, PartialEq)]
    struct GraphQLResponse<T> {
        data: T,
    }

    #[derive(Deserialize, Encode, Clone, Debug, PartialEq)]
    struct SubscanResponse<T> {
        code: u8,
        message: String,
        generated_at: u32,
        data: T,
    }

    #[derive(Serialize, Deserialize, Encode, Clone, Debug, PartialEq)]
    struct BlockDetailParams {
        block_timestamp: u64,
        only_head: bool,
    }

    #[derive(Deserialize, Encode, Clone, Debug, PartialEq)]
    pub struct BlockDetail {
        block_num: u32,
        block_timestamp: u64,
    }

    #[derive(Deserialize, Encode, Clone, Debug, PartialEq)]
    struct ConnectionEdge<T> {
        node: T,
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

    #[derive(Clone, Debug, PartialEq)]
    pub struct Chain {
        pub name: String,
        pub endpoint: String,
        pub subscan_url: String,
        pub squid_url: String,
        pub nonce: u64,
        pub period_last_block: u32,
        pub period_block_count: u32,
        pub shares: U64F64,
        pub budget_per_block: U64F64,
    }

    fn timestamp_to_iso(timestamp: i64) -> Result<String, Error> {
        let date_time = DateTime::from_timestamp(timestamp, 0).ok_or(Error::InvalidTimestamp)?;

        Ok(date_time.to_rfc3339_opts(SecondsFormat::Millis, true))
    }

    fn fetch_nonce(endpoint: String) -> Result<u64, Error> {
        let raw_storage_result = get_storage(
            endpoint.as_str(),
            &storage_prefix("PhalaComputation", "BudgetUpdateNonce"),
            None,
        )
        .map_err(|_| Error::RpcRequestFailed)?;

        if let Some(raw_storage) = raw_storage_result {
            let nonce = scale::Decode::decode(&mut raw_storage.as_slice())
                .map_err(|_| Error::InvalidStorage)?;
            Ok(nonce)
        } else {
            Ok(0)
        }
    }

    impl Chain {
        pub fn new(name: String, endpoint: String, subscan_url: String, squid_url: String) -> Self {
            let endpoint_clone = endpoint.clone();
            Self {
                name,
                endpoint,
                subscan_url,
                squid_url,
                nonce: fetch_nonce(endpoint_clone).unwrap(),
                period_last_block: 0,
                period_block_count: 0,
                shares: fixed!(0: U64F64),
                budget_per_block: fixed!(0: U64F64),
            }
        }

        fn fetch_shares_by_timestamp(&mut self, timestamp: u64) -> Result<U64F64, Error> {
            let resp = http_get!(format!("{}?query=%7B%20workerSharesSnapshotsConnection(orderBy%3A%20updatedTime_ASC%2C%20first%3A%201%2C%20where%3A%20%7BupdatedTime_gte%3A%20%22{}%22%7D)%20%7B%20edges%20%7B%20node%20%7B%20shares%20%7D%20%7D%20%7D%20%7D%0A", self.squid_url, timestamp_to_iso(timestamp as i64)?));
            if resp.status_code != 200 {
                return Err(Error::FetchSharesFailed);
            }
            let result: GraphQLResponse<SnapshotData> =
                from_slice(&resp.body).map_err(|_| Error::FetchSharesFailed)?;

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

        pub fn send_extrinsic(&self, signer: [u8; 32], nonce: u64) -> Result<Vec<u8>, Error> {
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
                .map_err(|_| Error::CreateExtrinsicFailed)?;
                Ok(send_transaction(&self.endpoint, &signed_tx)
                    .map_err(|_| Error::SendExtrinsicFailed)?)
            } else {
                Err(Error::InvalidNonce)
            }
        }

        pub fn fetch_block_by_timestamp(&self, block_timestamp: u64) -> Result<BlockDetail, Error> {
            let headers = vec![
                ("X-API-Key".into(), SUBSCAN_API_KEY.into()),
                ("Content-Type".into(), "application/json".into()),
            ];
            let resp = http_post!(
                format!("{}{}", &self.subscan_url, "/api/scan/block"),
                to_vec(&BlockDetailParams {
                    block_timestamp,
                    only_head: true
                })
                .unwrap(),
                headers
            );
            if resp.status_code != 200 {
                return Err(Error::FetchBlockFailed);
            }
            let result: SubscanResponse<BlockDetail> =
                from_slice(&resp.body).map_err(|_| Error::FetchBlockFailed)?;
            Ok(result.data)
        }

        pub fn fetch_current_timestamp(&self) -> Result<u64, Error> {
            let headers = vec![("X-API-Key".into(), SUBSCAN_API_KEY.into())];
            let resp = http_get!(format!("{}{}", &self.subscan_url, "/api/now"), headers);
            if resp.status_code != 200 {
                return Err(Error::FetchTimestampFailed);
            }
            let result: SubscanResponse<u64> =
                from_slice(&resp.body).map_err(|_| Error::FetchTimestampFailed)?;
            Ok(result.data)
        }

        pub fn fetch_period_block_count(
            &mut self,
            end_time: u64,
            period: u64,
        ) -> Result<u32, Error> {
            let start_time = end_time - period;
            let start_block = self.fetch_block_by_timestamp(start_time)?;
            let end_block = self.fetch_block_by_timestamp(end_time)?;
            self.period_last_block = end_block.block_num;
            let count = end_block.block_num - start_block.block_num;
            self.period_block_count = count;
            Ok(count)
        }

        pub fn set_budget_per_block(&mut self, total_shares: U64F64, halving: U64F64) {
            let total_budget = U64F64::from_num(ONE_PERIOD_BUDGET) * halving;
            let max_budget = total_budget / U64F64::from_num(PERIOD / 12 / 3);
            self.budget_per_block =
                (self.shares / total_shares / U64F64::from_num(self.period_block_count)
                    * total_budget)
                    .min(max_budget);
        }
    }

    impl BudgetBalancer {
        #[ink(constructor)]
        pub fn new() -> Self {
            let executor_private_key =
                pink_web3::keys::pink::KeyPair::derive_keypair(b"executor").private_key();
            let executor_account = signing::get_public_key(&executor_private_key, SigType::Sr25519);

            Self {
                executor_account,
                executor_private_key,
            }
        }

        #[ink(message)]
        pub fn get_executor_account(&self) -> String {
            hex::encode(&self.executor_account)
        }

        #[ink(message)]
        pub fn update_budget(&self) -> Result<(u128, u128), Error> {
            let mut phala = Chain::new(
                "Phala".into(),
                "https://phala.api.onfinality.io/public-ws".into(),
                "https://phala.api.subscan.io".into(),
                "https://squid.subsquid.io/phala-computation-lite/graphql".into(),
            );

            let mut khala = Chain::new(
                "Khala".into(),
                "https://khala.api.onfinality.io/public-ws".into(),
                "https://khala.api.subscan.io".into(),
                "https://squid.subsquid.io/khala-computation-lite/graphql".into(),
            );

            let timestamp = phala.fetch_current_timestamp()?;
            let halving_index = HALVING_START_INDEX
                + (timestamp as i64 - HALVING_START_TIME).div_ceil(HALVING_PERIOD);
            let halving = pow(HALVING_RATIO, halving_index as u32);
            let period_index = timestamp / PERIOD;
            let period_end = period_index * PERIOD;
            let nonce = u64::try_from(period_index as i64 + NONCE_OFFSET).unwrap();
            if phala.nonce >= nonce && khala.nonce >= nonce {
                return Err(Error::InvalidNonce);
            }

            phala.fetch_period_block_count(period_end, PERIOD)?;
            khala.fetch_period_block_count(period_end, PERIOD)?;

            let phala_shares = phala.fetch_shares_by_timestamp(period_end)?;
            let khala_shares = khala.fetch_shares_by_timestamp(period_end)?;
            let total_shares = phala_shares + khala_shares;
            phala.set_budget_per_block(total_shares, halving);
            khala.set_budget_per_block(total_shares, halving);

            // Make  extrinsics not block each other
            phala
                .send_extrinsic(self.executor_private_key, nonce)
                .unwrap_or_default();
            khala
                .send_extrinsic(self.executor_private_key, nonce)
                .unwrap_or_default();

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

            let budget_balancer = BudgetBalancer::new();

            let account = budget_balancer.get_executor_account();
            println!("executor account: {}", account);
            let result = budget_balancer.update_budget().unwrap();
            println!("phala budget: {}", result.0);
            println!("khala budget: {}", result.1);
        }
    }
}
