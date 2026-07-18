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
