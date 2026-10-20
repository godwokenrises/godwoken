# Godwoken Refund wallet

For a self-contained Docker image and public HTTPS setup, see
[Docker deployment](../docs/wallet-deployment.md).

Connect an Ethereum wallet to see its CKB mainnet OmniLock receiving address,
all live cells and recent transactions. Manual transfers and Godwoken V0/V1
refunds use the same wallet; no refund table, deployment or protocol selector is
required. Follow the address and transaction links to Nervos Explorer.

The service stores no private keys. The user's ETH wallet signs the exact CKB
transfer with `personal_sign`. The browser independently checks ownership,
recipient, amounts, change, dependencies, fee and signing digest. The API rebuilds
the transaction, checks confirmed live inputs and validates scripts before relay.

CKB and any standard sUDT can be transferred. The bundled token catalogue is only
for display labels and decimal precision, not an eligibility list. Unknown sUDTs
are shown in raw units. Cells requiring other type scripts or custom data remain
visible with Explorer links and require their asset-specific transfer tools.
CKB balances include capacity locked in these cells; the transferable column
shows capacity in supported cells. Large wallets transfer in batches of 64 cells.

```sh
# In claim-web/
npm ci
npm test
npm run build

# In exit-withdraw/; use the project's Rust 1.95 toolchain.
cargo +1.95.0 build --locked --bin gw-withdraw
./target/debug/gw-withdraw wallet-serve \
  --web-dir ./claim-web/dist \
  --listen 127.0.0.1:8787 \
  --public-origin http://127.0.0.1:8787 \
  --enable-broadcast
```

Open http://127.0.0.1:8787 in a browser with an Ethereum wallet extension.
Connecting reveals the receiving address without requiring a signature or ETH
gas. The default RPC and dependency configuration is CKB mainnet. Submission is
opt-in with `--enable-broadcast` and still requires the user's transaction signature.

The default fee is **1000 shannons/KB** (0.00001 CKB per KB), rounded up using the
actual serialized transaction size, including witnesses. There is no service fee.
Both browser and server enforce a 0.01 CKB absolute fee ceiling. The exact fee and
recipient amounts appear before signing. Token-cell capacity and change must
meet CKB consensus requirements; they are not additional fees. Default finality
is one committed block, configurable with `--confirmations`.

`/api/wallet/config`, `/api/wallet/account/<ETH>`, and
`/api/wallet/transactions/<ETH>?cursor=<cursor>` provide read-only wallet views.
`POST /api/wallet/prepare` takes a `BuildRequest`. `POST /api/wallet/submit` takes
the request, reviewed transaction hash, signing message and owner signature.
Only same-origin JSON submissions are accepted. Transaction status is available
at `/api/wallet/transaction/<hash>`. A lost submission response must be checked by
its already saved transaction hash before retrying.

The older `claim-serve` release-scoped API and operator control proposals are
retained for existing distribution integrations. The simplified wallet UI uses
`wallet-serve`; it does not depend on that API or change settlement permissions.
