const {create, signCertificate, types} = require('@phala/sdk')
const {WsProvider, ApiPromise} = require('@polkadot/api')
const fs = require('fs')
const {ContractPromise} = require('@polkadot/api-contract')
const {Keyring} = require('@polkadot/api')
const {waitReady} = require('@polkadot/wasm-crypto')

const main = async () => {
  await waitReady()
  const keyring = new Keyring({type: 'sr25519'})
  const alice = keyring.addFromUri('//Alice')
  const provider = new WsProvider('wss://phala-rpc.dwellir.com')
  const pruntimeURL = 'https://phat-cluster-de.phala.network/pruntime-01'
  const api = await ApiPromise.create({provider, types})
  const certificate = await signCertificate({api, pair: alice})
  const contractId =
    '0xaaa789d2b1232cb53e2f3e964d7bf924069becc4979451f9c67935521a7f500e'
  const metadata = fs.readFileSync(
    '../target/ink/tokenomic_contract.json',
    'utf-8'
  )

  const decoratedApi = (await create({api, baseURL: pruntimeURL, contractId}))
    .api

  const contract = new ContractPromise(
    decoratedApi,
    JSON.parse(metadata),
    contractId
  )

  try {
    const res1 = await contract.query.getExecutorAccount(certificate, {})
    const res2 = await contract.query.balance(certificate, {})

    console.log(res1.output.toJSON())
    console.log(res2.output.toJSON())
  } catch (err) {
    console.error(err)
  }
}

main()
