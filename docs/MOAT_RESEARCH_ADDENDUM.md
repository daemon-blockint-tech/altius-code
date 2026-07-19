# Addendum Riset Moat — Prior Art Non-Kompetitor Langsung

Lanjutan dari [Strategi Moat](MOAT_STRATEGY.md). Sembilan repo berikut bukan
kompetitor "AI coding agent generalis" langsung seperti enam repo di dokumen
utama, melainkan proyek yang relevan sebagai *prior art* — pola desain,
standar, atau kesenjangan pasar yang layak diketahui saat membangun vertikal
web3 Altius Code. Data diambil dari GitHub per 18 Juli 2026.

## 1. Ringkasan Temuan

| Repo | Kategori | Temuan kunci |
|---|---|---|
| langchain-ai/langsmith-sdk | Observability agent LLM | Tracing (`@traceable`), dataset evaluasi, feedback loop — pola untuk mengaudit *perilaku agent sendiri*, bukan transaksi on-chain |
| langchain-ai/deepagentsjs | Framework agent (JS) | Planning eksplisit (`write_todos`), filesystem virtual sebagai memori kerja, subagent dengan context terisolasi |
| solana-foundation/solana-improvement-documents | Standar protokol (SIMD) | Proses formal perubahan protokol Solana — sumber kanonis untuk melacak perubahan simulasi/fee/loader |
| solana-foundation/pay | CLI pembayaran agent | Membungkus `curl`/`claude`/`codex` untuk menangani HTTP 402 (x402/MPP) otomatis dengan otorisasi wallet biometrik |
| solana-foundation/create-solana-dapp | Scaffolding dApp | Konvensi proyek Anchor + frontend (Next.js/Vue), template via `--template` |
| solana-foundation/x402 | Standar protokol pembayaran | HTTP 402 lintas chain/fiat; integrasi 1 baris kode di server, 1 fungsi di client |
| solana-foundation/explorer | Explorer blockchain | Dekode transaksi per-protokol untuk tampilan yang bisa dibaca manusia |
| solana-foundation/solana-developer-platform | Platform enterprise | Wallet, token issuance, payments, compliance — masih pre-mainnet |
| i-am-bee/acp | Protokol komunikasi antar-agent | **Bukan** Agent Client Protocol (Zed) — ini "Agent Communication Protocol" (IBM/BeeAI), beda proyek meski nama sama |
| rust-langgraph (crates.io) | Orkestrasi agent (Rust) | Port awal LangGraph ke Rust — v0.1.1, satu maintainer, belum matang |

## 2. Implikasi Konkret untuk Altius Code

### a. Disambiguasi penting: "ACP" ada dua

`i-am-bee/acp` (Agent Communication Protocol, IBM/BeeAI) **bukan** protokol
yang dimaksud di README Altius Code. Protokol yang relevan untuk Altius Code
adalah **Agent Client Protocol** (agentclientprotocol.com, dipakai Zed) —
untuk komunikasi *editor ↔ agent*, bukan *agent ↔ agent*. Nama yang identik
ini berisiko membingungkan dokumentasi ke depan; setiap referensi "ACP" di
materi Altius Code sebaiknya secara eksplisit tautkan ke
agentclientprotocol.com agar tidak tertukar dengan proyek IBM tersebut.

### b. Peluang moat baru: agent yang bisa *membayar*, bukan cuma men-deploy

Temuan paling signifikan adalah `solana-foundation/pay` + `x402`: sudah ada
pola nyata "agent CLI membayar API berbayar secara otonom via stablecoin
Solana, dengan otorisasi wallet lokal, tanpa mengekspos private key ke
agent." Ini persis filosofi `altius-txguard` yang sudah dibangun (guardrail
+ signer terisolasi) — hanya beda target: bukan deploy program, tapi
transaksi pembayaran x402/MPP.

**Langkah lanjutan yang disarankan (masuk Langkah 5 — moat ekosistem, di
strategi utama):** perluas `TxKind` di `altius-txguard` dengan varian
`Payment` yang lewat pipeline lima-tahap yang sama, lalu tambahkan adapter
x402 di `altius-svm-tools` (atau crate baru `altius-x402`). Efeknya: Altius
Code bisa jadi agent coding pertama yang *juga* bisa membayar API/compute
berbayar secara aman dalam alur kerja yang sama dengan deploy — kombinasi
yang belum ada di kompetitor manapun di dokumen moat utama.

### c. Peluang perbaikan `DiffReport`: dekode instruksi per-protokol

`solana-foundation/explorer` mendekode instruksi per-jenis protokol (Token,
System, protokol DeFi dikenal) untuk tampilan yang mudah dibaca — bukan
sekadar delta lamports mentah. `DiffReport` (`crates/altius-txguard/src/diff.rs`)
saat ini hanya melaporkan delta lamports/owner dari `SimulationOutcome`.
**Peningkatan konkret:** tambahkan pengenal program ID terkenal (System,
Token, Token-2022, BPF Loader Upgradeable — yang sudah kita pakai sendiri di
`deploy_plan.rs`) agar diff bisa menampilkan "Transfer 0.5 SOL" alih-alih
hanya angka lamports mentah. Ini memperkuat Langkah 2 (moat kepercayaan)
karena manusia lebih mudah memverifikasi apa yang sebenarnya terjadi
sebelum approve.

### d. Peluang moat orkestrasi agent: Rust belum punya "LangGraph"

`rust-langgraph` masih v0.1.1, satu maintainer, dokumentasi tipis — sinyal
kuat bahwa ekosistem Rust **belum** punya kerangka orkestrasi agent
(planning + subagent + state graph) sematang LangGraph/`deepagentsjs` di
JS/Python. Karena Altius Code sendiri dibangun di Rust, ini kesempatan:
membangun lapisan orkestrasi task-management sendiri (bukan bergantung pada
crate pihak ketiga yang belum matang) sekaligus berpotensi jadi kontribusi
open-source yang menaikkan visibilitas Altius Code di komunitas Rust.

`deepagentsjs` memberi cetak biru konkret pola yang layak ditiru secara
konseptual (bukan meng-copy kode JS-nya): tool `write_todos` untuk
memecah pekerjaan, filesystem virtual sebagai memori kerja lintas langkah,
dan subagent dengan context terisolasi untuk tugas panjang. Ini langsung
menopang fitur "long-running task management" yang sudah ada di
positioning Altius Code, dan berhubungan erat dengan Langkah 4 (moat
memori) di strategi utama.

### e. Observability internal: pola dari LangSmith, tapi untuk audit agent bukan hanya transaksi

`altius-txguard` sudah punya audit log tamper-evident untuk *transaksi
on-chain*. LangSmith menunjukkan pola serupa untuk *keputusan agent secara
umum*: tracing tiap langkah reasoning/tool-call, dataset evaluasi, feedback
scoring. Untuk mendukung Langkah 3 (benchmark & evaluasi) di strategi
utama, Altius Code bisa mengadopsi pola "traceable run" ini pada level agent
(bukan hanya transaksi), sehingga trace tersebut sekaligus menjadi bahan
mentah untuk benchmark publik yang direncanakan.

### f. Yang dicatat tapi tidak ditindaklanjuti sekarang

- `create-solana-dapp`: konvensi scaffolding (Anchor + Next.js/pnpm) relevan
  untuk fitur "buat proyek baru" di masa depan, tapi bukan prioritas Fase 0.
- `solana-developer-platform`: masih pre-mainnet, fokus enterprise
  (compliance, wallet-as-a-service) — tumpang tindih sebagian dengan
  `PolicyConfig` kita; dipantau, belum perlu ditiru.
- `solana-improvement-documents`: tidak ada SIMD spesifik terkait simulasi/
  fee yang berhasil diidentifikasi dari halaman repo saja — perlu penelusuran
  langsung ke direktori `/proposals` bila dibutuhkan detail teknis, di luar
  cakupan sesi riset ini.

## 3. Prioritas Tindak Lanjut

| Urutan | Item | Alasan |
|---|---|---|
| 1 | Disambiguasi "ACP" di seluruh dokumentasi Altius Code | Risiko kebingungan/miskredit ke proyek lain, biaya perbaikan murah |
| 2 | Perkaya `DiffReport` dengan pengenal program ID terkenal | Perbaikan kecil, dampak langsung ke kepercayaan pengguna (Langkah 2) |
| 3 | Rancang `TxKind::Payment` + adapter x402/MPP | Perluasan moat nyata, belum ada di kompetitor manapun |
| 4 | Jelajahi pola planning/subagent ala `deepagentsjs` untuk runtime agent Altius sendiri | Menopang Langkah 4, sekaligus mengisi kekosongan ekosistem Rust |

## 4. Competitive landscape refresh (19 Jul 2026)

Sumber primer / produk publik (bukan spekulasi pemasaran). Fokus: mekanika Claude Code
yang Altius adaptasi, scanner keamanan, simulasi transaksi, dan agent web3 vertikal.

### 4.1 Matriks kompetitor / adjacent

| Produk | Kategori | Overlap vs Altius | Diferensiasi Altius |
|---|---|---|---|
| [Claude Code Remote Control](https://code.claude.com/docs/en/remote-control) | Remote coding agent (research preview) | Thin client → host session; permission gates tetap; MCP/skills lokal | Altius: self-hosted BeeACP/PWA + bearer token + SQLite RunStore; **bukan** subscription OAuth Anthropic; domain web3 |
| [Cyfrin Aderyn](https://github.com/cyfrin/aderyn) | Static analyzer Solidity (Rust), SARIF/MD/JSON, CI Action | `altius scan --format sarif` di CI | Aderyn = detector EVM dalam; Altius = fleet Solana-first + agent route `/scan` + TxGuard path (scanner chain lain di `altius-scanners` bersifat sekunder, lihat §0 strategi utama — bukan prioritas investasi) |
| Trail of Bits Slither / Mythril | Static / symbolic EVM | Pattern scanners, CI gate | Altius mengorkestrasi native scanners + agent, bukan mengganti Slither |
| [Tenderly Simulations](https://docs.tenderly.co/simulations/overview) | Dev-grade tx simulation API (100+ EVM nets) | Preview outcome sebelum broadcast | Altius TxGuard: sim → HITL → isolated signer; Solana-first + policy fail-closed |
| Blowfish (wallet simulation / fraud) | End-user / wallet risk engine (Solana+EVM) | Human-readable pre-sign preview | Altius target **developer agent** workflow, bukan wallet extension |
| Pocket Universe | Browser extension + insurance tier | Pre-sign phishing catch | Adjacent UX; bukan coding fleet |
| [SmartContract-VulnHunter](https://github.com/MaridWSH/SmartContract-VulnHunter) | Multi-scanner CLI + LLM triage + SARIF | Orchestrates Slither/Aderyn/Trident/sec3 | Altius: Rust fleet + BeeACP remote + TxGuard; VulnHunter = scanner orchestra |
| Lamport / forge-solana-sdk / Luna Agent | Solana codegen / desktop audit agents | Anchor generate/build/audit loops | Altius: production guardrails + remote PWA + plugin packs; jangan race codegen UX saja |
| Solana Agent Kit / GOAT / Rig | On-chain action toolkits | Agent tools for transfer/swap/DeFi | Toolkit ≠ full fleet; Altius harus **membungkus** aksi lewat TxGuard, bukan expose raw tools |

### 4.2 Temuan Claude Remote Control (primer)

Dari docs resmi Anthropic (research preview, diperbarui 2026):

- Session tetap **lokal**; browser/phone hanya viewport (filesystem/MCP/config di mesin host).
- Auth: **claude.ai OAuth saja** — API keys tidak didukung; Team/Enterprise off-by-default sampai Owner enable.
- Permission gates tetap aktif saat remote; sandbox opsional; session URL harus diperlakukan sebagai secret.
- Fitur mature: reconnect, multi-device sync, worktree spawn, capacity limits.

**Implikasi produksi Altius:** P0 remote (token + SSE + durable store + awaiting HITL) sudah arah yang benar. Gap vs Claude: Trusted Devices / org admin toggle, outbound-only relay (bukan open bind tanpa auth), QR/session naming, reconnect resilience. **Jangan** meniru lock-in OAuth lab; pertahankan self-hosted + model-agnostic (Langkah 6).

### 4.3 Implikasi production-readiness

| Prioritas | Aksi | Alasan kompetitif |
|---|---|---|
| P0 | Bearer wajib di non-localhost; dokumentasikan no-auth hanya offline demo | Claude memperlakukan remote URL sebagai credential; Altius harus setara |
| P0 | Human-readable `DiffReport` (program ID dikenal) sebelum approve | Tenderly/Blowfish menang di preview readability (Langkah 2) |
| P1 | SARIF CI fail-on High/Critical + artifact upload (sudah ada job `scan`) | Paritas Aderyn CI / VulnHunter SARIF |
| P1 | Perdalam detector Solana (lint SVM + shell-out opsional ke tool audit Solana) | Kedalaman vertikal Solana adalah moat; lihat §0 strategi utama — **cross-chain static analysis sengaja tidak dikejar** meski itu gap pasar 2026 |
| P2 | `TxKind::Payment` / x402 lewat TxGuard | Differentiator vs Agent Kit/GOAT yang expose transfer mentah |
| P2 | Plugin pack marketplace web3 (bukan general) | Langkah 5; v0 install-by-path cukup sampai retention ada |
| Hindari | Race channel messaging / desktop IDE / frontier model lock-in | Sudah di §5 strategi utama; Claude/OpenClaw menang di sana |

### 4.4 Sumber

- https://code.claude.com/docs/en/remote-control
- https://github.com/cyfrin/aderyn (v0.6.8+, SARIF, GitHub Action, ~784★ / 19 Jul 2026)
- https://docs.tenderly.co/simulations/overview
- https://docs.tenderly.co/api-reference/simulations/simulate-transaction
- https://github.com/brave/brave-browser/wiki/Transaction-Simulation (Blowfish as Brave Wallet backend)
- https://github.com/MaridWSH/SmartContract-VulnHunter
- https://www.alchemy.com/blog/how-to-build-solana-ai-agents-in-2026
- https://github.com/manavnotop/lamport
- https://github.com/Prestes16/luna-agent

## 5. Studi banding SDK: opencode (19 Jul 2026)

Sumber: dokumen SDK resmi opencode (`https://opencode.ai/docs/sdk/`, salinan
diunggah) + dokumen Server (`https://opencode.ai/docs/server/`). opencode
dilacak sebagai kompetitor generalis kunci di §1 [Strategi Moat](MOAT_STRATEGY.md).
Perbandingan ini menilai *permukaan SDK/API remote*-nya terhadap permukaan
remote Altius (BeeACP HTTP di `crates/altius-protocol/src/beeacp/` +
`crates/altius-cli/src/serve_command.rs`).

### 5.1 Ringkasan permukaan SDK opencode

- **Bentuk klien:** paket npm `@opencode-ai/sdk`. Dua entrypoint —
  `createOpencode()` men-*spawn* server + klien sekaligus; `createOpencodeClient({ baseUrl })`
  menyambung ke server yang sudah jalan. Opsi klien: `baseUrl`, `fetch`
  kustom, `parseAs`, `responseStyle`, `throwOnError`.
- **Tipe & generasi kode:** semua tipe (`Session`, `Message`, `Part`, `Agent`,
  dst.) **di-generate dari spec OpenAPI 3.1 server** dan dikomit sebagai
  `types.gen.ts`. Server headless (`opencode serve`) mengekspos spec di
  endpoint `/doc`; SDK diturunkan darinya, sehingga klien bahasa lain bisa
  dibangkitkan. Nav dokumen menandakan setidaknya SDK JS/TS + Go.
- **Model API — *session-centric*, bukan *run-centric*:** unit utamanya
  `Session` yang tahan lama, berisi `Message`/`Part`, dengan `children`
  (subsession), `fork`, `revert`/`unrevert`, `share`/`unshare`, `summarize`,
  `todo` per-session, dan riwayat pesan yang bisa di-*list*/di-*get*.
  `session.prompt` mendukung `noReply` (injeksi konteks) dan
  **structured output** (`format: json_schema`, dengan `retryCount`).
- **Auth:** HTTP Basic (`OPENCODE_SERVER_PASSWORD`, user default `opencode`)
  di sisi server; plus OAuth *provider* (`/provider/{id}/oauth/...`) dan
  `PUT /auth/:id` untuk kredensial provider model. CORS via flag `--cors`.
- **Eventing/streaming:** **satu bus SSE global** (`GET /event`, frame pertama
  `server.connected` lalu event bus bertipe dengan `event.type` +
  `event.properties`). Bukan per-run. Ada `POST /session/:id/prompt_async`
  (balas `204`, hasil diamati lewat event) dan endpoint izin eksplisit
  `POST /session/:id/permissions/:permissionID` untuk merespons *permission
  request* — jadi tool-call/izin adalah event bertipe di kabel.
- **Lain-lain di kabel:** daftar `tool` bertipe skema JSON (`/experimental/tool`),
  status/penambahan MCP (`/mcp`), daftar `agent`, status LSP/formatter, dan
  jembatan TUI (`/tui/*`). **Versioning:** `global.health()` mengembalikan
  `version`; SDK diberi versi di npm; spec `/doc` jadi kontrak sumber tunggal.

### 5.2 Gap vs permukaan remote Altius (BeeACP)

| Aspek | opencode | Altius BeeACP (sekarang) | Gap / catatan |
|---|---|---|---|
| Spec mesin | OpenAPI 3.1 di `/doc`, jadi sumber generasi | Tidak ada spec; hanya tabel prosa di docs | **Gap nyata** — tak ada kontrak mesin |
| Klien resmi | Paket npm typed, tipe di-generate | PWA vanilla JS `fetch` tanpa tipe (`assets/pwa/app.js`) | **Gap** — klien tak typed, rawan drift |
| Cakupan bahasa | JS/TS (+ Go), generatable dari OpenAPI | — | Ikut dari gap spec |
| Unit utama | `Session` tahan lama + riwayat pesan | `Run` sekali-jalan (`created→in-progress→awaiting→…`) | Beda paradigma; **sengaja** lebih ramping |
| Riwayat pesan | `list/get message`, `part`, `fork`, `revert`, `todo` | Hanya `input`/`output` per run; tak ada list message | Gap fitur — *scope generalis*, lihat §5.3 |
| Event model | 1 bus SSE bertipe, granular (part/tool/permission) | Per-run `GET /runs/{id}/events`, polling 500 ms, snapshot Run utuh saat berubah | **Gap** — event kasar, tak bertipe granular |
| Izin/tool di kabel | `permissions/:permissionID` + event izin bertipe | HITL = status `awaiting`; resume = injeksi pesan generik | **Gap paling relevan web3** — approval tak berstruktur |
| Structured output | `json_schema` + retry | Tidak ada | Scope generalis; abaikan untuk kini |
| Auth | HTTP Basic + OAuth provider | Bearer token (+ `?token=` untuk SSE) | **Setara/lebih baik** untuk vertikal; pertahankan |
| Discovery | mDNS opsional | A2A agent-card + ANP stub | Beda jalur; memadai |

### 5.3 Rekomendasi respons Altius (terkecil, sesuai strategi moat)

Prinsip: **jangan mengkloning SDK generalis** (session/fork/todo/tui/
structured-output = medan yang sengaja dihindari, lihat §5 strategi utama).
Ambil hanya mekanika yang menaikkan kepercayaan + integrasi tanpa memperluas
permukaan generalis.

**Status (Juli 2026):** item 1–4 di bawah **selesai** di repo (`/openapi.json`,
`beeacp-client.js`, field `approval` pada run/SSE, §2.1 `FLEET_ARCHITECTURE.md`).

1. **Terbitkan spec OpenAPI 3.1 untuk permukaan BeeACP** (`/runs`,
   `/runs/{id}`, `/runs/{id}/events`, `/cancel`, `/resume`) + kartu A2A.
   Bangkitkan dari kode via `utoipa` (turunan dari struct `Run`/`Message`/
   `CreateRunRequest`/`ResumeRunRequest` yang sudah `Serialize`/`Deserialize`),
   sajikan di endpoint `/doc` atau `/openapi.json`, dan komit artefaknya.
   *Leverage tertinggi:* satu artefak menutup gap "spec" **dan** "klien typed"
   sekaligus, dengan biaya kecil karena tipe wire sudah ada.
2. **Generate klien TS tipis untuk PWA** dari spec itu
   (`openapi-typescript` + `openapi-fetch`) untuk menggantikan `fetch`
   tanpa tipe di `assets/pwa/app.js` — tetap *thin client* zero-build, tapi
   typed dan tahan-drift. Publikasi ke npm bisa ditunda; cukup vendor dulu.
3. **Naikkan HITL `awaiting` jadi event kabel bertipe** — ini adaptasi
   web3-relevan dari pola `permission` opencode, *tanpa* mengadopsi model
   session. Sertakan payload approval terstruktur (mis. ringkasan
   `DiffReport`/preview TxGuard: "Transfer 0.5 SOL", program ID dikenal) di
   frame SSE `event: run` saat status `awaiting`, plus bentuk balasan
   resume/izin bertipe. Ini menyatukan §4.3 (P0 `DiffReport` readable) dengan
   permukaan remote, dan justru menjadi pembeda vertikal: "setujui pratinjau
   transaksi", bukan sekadar "lanjutkan sesi".
4. **Dokumentasikan protokol wire** (status lifecycle, auth Bearer, semantik
   SSE) di docs sebagai pelengkap spec, sejalan gaya §4.

**Sengaja TIDAK ditiru:** session/message-history/fork/revert/todo,
`prompt_async`, jembatan TUI, dan structured-output `json_schema` — semuanya
memperlebar ke arah agent generalis dan bertentangan dengan §5 strategi utama.

### 5.4 Sumber

- https://opencode.ai/docs/sdk/ (salinan unggahan `sdk-0.md`, terakhir diperbarui 17 Jul 2026)
- https://opencode.ai/docs/server/ (OpenAPI 3.1 di `/doc`, HTTP Basic auth, bus SSE `/event`)
- Referensi tipe: `packages/sdk/js/src/gen/types.gen.ts` (repo opencode)
- Bandingkan: `crates/altius-protocol/src/beeacp/{routes.rs,model.rs}`, `crates/altius-cli/src/serve_command.rs`, `crates/altius-cli/assets/pwa/app.js`
