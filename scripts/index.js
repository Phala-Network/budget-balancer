const {create, signCertificate, types} = require('@phala/sdk')
const {WsProvider, ApiPromise} = require('@polkadot/api')
const {ContractPromise} = require('@polkadot/api-contract')
const {Keyring} = require('@polkadot/api')
const {waitReady} = require('@polkadot/wasm-crypto')

const metadata =
  '{"source":{"hash":"0x3215226038f525e8a60612f2e1e5678f96088c95363e084809110895fd71d89d","language":"ink! 4.2.0","compiler":"rustc 1.69.0","build_info":{"build_mode":"Release","cargo_contract_version":"2.1.0","rust_toolchain":"stable-aarch64-apple-darwin","wasm_opt_settings":{"keep_debug_symbols":false,"optimization_passes":"Z"}}},"contract":{"name":"tokenomic_contract","version":"0.1.0","authors":["kingsley"]},"spec":{"constructors":[{"args":[],"default":false,"docs":["Constructor to initializes your contract"],"label":"new","payable":false,"returnType":{"displayName":["ink_primitives","ConstructorResult"],"type":1},"selector":"0x9bae9d5e"}],"docs":[],"environment":{"accountId":{"displayName":["AccountId"],"type":11},"balance":{"displayName":["Balance"],"type":9},"blockNumber":{"displayName":["BlockNumber"],"type":15},"chainExtension":{"displayName":["ChainExtension"],"type":16},"hash":{"displayName":["Hash"],"type":13},"maxEventTopics":4,"timestamp":{"displayName":["Timestamp"],"type":14}},"events":[],"lang_error":{"displayName":["ink","LangError"],"type":3},"messages":[{"args":[],"default":false,"docs":[],"label":"get_executor_account","mutates":false,"payable":false,"returnType":{"displayName":["ink","MessageResult"],"type":4},"selector":"0xa0d3b7a4"},{"args":[],"default":false,"docs":[],"label":"balance","mutates":false,"payable":false,"returnType":{"displayName":["ink","MessageResult"],"type":6},"selector":"0xb48c1684"}]},"storage":{"root":{"layout":{"struct":{"fields":[{"layout":{"array":{"layout":{"leaf":{"key":"0x00000000","ty":0}},"len":32,"offset":"0x00000000"}},"name":"executor_account"},{"layout":{"array":{"layout":{"leaf":{"key":"0x00000000","ty":0}},"len":32,"offset":"0x00000000"}},"name":"executor_private_key"}],"name":"Tokenomic"}},"root_key":"0x00000000"}},"types":[{"id":0,"type":{"def":{"primitive":"u8"}}},{"id":1,"type":{"def":{"variant":{"variants":[{"fields":[{"type":2}],"index":0,"name":"Ok"},{"fields":[{"type":3}],"index":1,"name":"Err"}]}},"params":[{"name":"T","type":2},{"name":"E","type":3}],"path":["Result"]}},{"id":2,"type":{"def":{"tuple":[]}}},{"id":3,"type":{"def":{"variant":{"variants":[{"index":1,"name":"CouldNotReadInput"}]}},"path":["ink_primitives","LangError"]}},{"id":4,"type":{"def":{"variant":{"variants":[{"fields":[{"type":5}],"index":0,"name":"Ok"},{"fields":[{"type":3}],"index":1,"name":"Err"}]}},"params":[{"name":"T","type":5},{"name":"E","type":3}],"path":["Result"]}},{"id":5,"type":{"def":{"primitive":"str"}}},{"id":6,"type":{"def":{"variant":{"variants":[{"fields":[{"type":7}],"index":0,"name":"Ok"},{"fields":[{"type":3}],"index":1,"name":"Err"}]}},"params":[{"name":"T","type":7},{"name":"E","type":3}],"path":["Result"]}},{"id":7,"type":{"def":{"variant":{"variants":[{"fields":[{"type":8}],"index":0,"name":"Ok"},{"fields":[{"type":10}],"index":1,"name":"Err"}]}},"params":[{"name":"T","type":8},{"name":"E","type":10}],"path":["Result"]}},{"id":8,"type":{"def":{"tuple":[9,9]}}},{"id":9,"type":{"def":{"primitive":"u128"}}},{"id":10,"type":{"def":{"variant":{"variants":[{"index":0,"name":"InvalidStorage"},{"index":1,"name":"HttpRequestFailed"},{"index":2,"name":"InvalidResponseBody"},{"index":3,"name":"BlockNotFound"},{"index":4,"name":"SharesNotFound"},{"index":5,"name":"InvalidNonce"}]}},"path":["tokenomic_contract","tokenomic_contract","Error"]}},{"id":11,"type":{"def":{"composite":{"fields":[{"type":12,"typeName":"[u8; 32]"}]}},"path":["ink_primitives","types","AccountId"]}},{"id":12,"type":{"def":{"array":{"len":32,"type":0}}}},{"id":13,"type":{"def":{"composite":{"fields":[{"type":12,"typeName":"[u8; 32]"}]}},"path":["ink_primitives","types","Hash"]}},{"id":14,"type":{"def":{"primitive":"u64"}}},{"id":15,"type":{"def":{"primitive":"u32"}}},{"id":16,"type":{"def":{"variant":{}},"path":["pink_extension","chain_extension","PinkExt"]}}],"version":"4"}'

const main = async () => {
  if (new Date().getMinutes() <= 1) {
    return
  }
  await waitReady()
  const keyring = new Keyring({type: 'sr25519'})
  const alice = keyring.addFromUri('//Alice')
  const provider = new WsProvider('wss://api.phala.network/ws')
  const pruntimeURL = 'https://phat-cluster-de.phala.network/pruntime-01'
  const api = await ApiPromise.create({provider, types})
  const certificate = await signCertificate({api, pair: alice})
  const contractId =
    '0x593bc7272e4273ba981a7686e5b75ee3e5687da3ecce69a714171446178f1a13'

  const decoratedApi = (await create({api, baseURL: pruntimeURL, contractId}))
    .api

  const contract = new ContractPromise(
    decoratedApi,
    JSON.parse(metadata),
    contractId
  )

  try {
    // const res1 = await contract.query.getExecutorAccount(certificate, {})
    const res2 = await contract.query.balance(certificate, {})

    // console.log(res1.output.toJSON())
    console.log(res2.output.toJSON())
  } catch (err) {
    console.error(err)
  }
}

main()

setInterval(() => {
  main()
}, 1000 * 60 * 5)
