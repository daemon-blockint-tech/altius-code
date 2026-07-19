# Strategi Moat Altius Code

Dokumen ini menyusun langkah-langkah membangun *moat* (parit kompetitif) Altius
Code terhadap enam kompetitor utama di ruang AI coding agent. Data kompetitor
diambil dari halaman GitHub masing-masing per 18 Juli 2026.

## 0. Keputusan Fokus: Solana Only (19 Juli 2026)

**Moat vertikal Altius Code fokus penuh ke Solana/SVM — bukan multi-chain.**
Ini keputusan eksplisit, bukan default yang belum dipikirkan:

- Setiap penyebutan "blockchain/web3" generik di dokumen ini dan turunannya
  (`MOAT_RESEARCH_ADDENDUM.md`) berarti **Solana**, kecuali disebut lain secara
  eksplisit sebagai catatan riset prior-art.
- Toolchain yang jadi prioritas kelas satu: **Anchor, Pinocchio, dan Rust
  native (`cargo build-sbf`)** — tiga framework yang sudah didukung nyata di
  `altius-svm-detect`/`altius-svm-tools`. Foundry/Hardhat (EVM), Move
  (Aptos/Sui), dan CosmWasm **bukan** target — disebutkan di draf awal
  dokumen ini sebagai contoh pola vertikal, bukan roadmap yang disetujui.
- **Alasan:** kedalaman mengalahkan lebar (lihat §1 & Langkah 1 di bawah).
  Repo saat ini sudah punya `altius-scanners` dengan heuristik untuk EVM,
  Cairo, Cosmos, Algorand, dan TON selain SVM — itu tetap boleh ada sebagai
  kapabilitas sekunder/eksperimental, **tapi tidak lagi diinvestasikan** dan
  tidak masuk hitungan metrik moat (§4). Semua penambahan fitur, benchmark,
  dan dokumentasi baru berikutnya mengasumsikan Solana sebagai satu-satunya
  chain utama sampai keputusan ini direvisi secara eksplisit.

## 1. Peta Kompetitor

| Kompetitor | Posisi | Lisensi | Popularitas | Kekuatan utama |
|---|---|---|---|---|
| [claude-code](https://github.com/anthropics/claude-code) (Anthropic) | Terminal + IDE + GitHub agent, proprietary | Komersial | ±138k ⭐ | Kualitas model frontier, ekosistem plugin, distribusi multi-kanal |
| [grok-build](https://github.com/xai-org/grok-build) (xAI) | TUI coding agent berbasis Rust — posisi paling mirip Altius | Apache 2.0 | ±17,5k ⭐ | Reliabilitas production-grade, TUI cepat, MCP/plugin/skills |
| [opencode](https://github.com/anomalyco/opencode) | Coding agent open-source, CLI + desktop | MIT | ±187k ⭐ | Model-agnostic, komunitas besar (963 kontributor), mode build/plan |
| [hermes-agent](https://github.com/NousResearch/hermes-agent) (Nous) | Agent self-improving multi-platform | MIT | ±217k ⭐ | Learning loop (membuat & memperbaiki skill sendiri), memori prosedural, deploy fleksibel |
| [openclaw](https://github.com/openclaw/openclaw) | Asisten AI personal self-hosted, multi-channel | MIT | ±383k ⭐ | Local-first, kedaulatan data, sandboxing, 10+ kanal pesan |
| [arc-agi-crystalline](https://github.com/synchopate/arc-agi-crystalline) | Riset: memori kognitif 5-tier untuk ARC-AGI-3 | — | 11 ⭐ | Teknik "crystallized memory": belajar dari *kenapa* percobaan gagal (+70% completion) |

**Kesimpulan peta:** kategori "AI coding agent generalis di terminal" sudah
sangat padat dan dimenangkan oleh pemain dengan distribusi raksasa
(claude-code) atau komunitas open-source masif (opencode, hermes, openclaw).
Bersaing frontal di sana berarti bersaing tanpa moat. Celah yang belum
diambil siapa pun: **tidak ada satu pun kompetitor yang fokus pada
pengembangan blockchain/web3** — padahal itu justru pembeda yang sudah ada di
positioning Altius Code.

## 2. Langkah Moat (Berurutan Berdasarkan Prioritas)

### Langkah 1 — Moat Vertikal: jadi agent #1 untuk pengembangan Solana

Jangan bersaing sebagai "coding agent yang juga bisa Solana", tapi sebagai
"agent Solana yang kebetulan juga coding agent lengkap".

- Dukungan kelas satu untuk toolchain Solana: **Anchor, Pinocchio, dan Rust
  native** — deteksi otomatis proyek dan alur kerja yang sesuai (compile →
  test → lint → simulasi → deploy). Chain lain (EVM, Move, Cosmos, dst.)
  sengaja tidak diprioritaskan (lihat §0).
- Alur kerja *security-first*: lint keamanan SVM bawaan (missing
  signer/owner check, arbitrary CPI, dsb. — sudah ada di
  `altius-svm-tools`) sebagai langkah wajib sebelum deploy, bukan opsi
  tambahan.
- Pemahaman on-chain: baca state program, decode transaksi & instruksi
  Solana per-protokol, simulasi RPC nyata sebelum eksekusi mainnet.

*Kenapa ini moat:* kompetitor generalis besar tidak akan memprioritaskan
vertikal ini (pasar mereka horizontal), dan pendatang kecil butuh bertahun
untuk menyamai kedalaman domain. Vertikal yang dalam mengalahkan generalis
yang lebar.

### Langkah 2 — Moat Kepercayaan: guardrail transaksi yang tidak bisa ditawar

Di web3, kesalahan agent bersifat ireversibel dan berbiaya uang nyata. Yang
paling dipercaya, menang.

- Simulasi wajib (fork/dry-run) sebelum transaksi mainnet apa pun.
- Konfirmasi manusia eksplisit untuk aksi ireversibel: deploy mainnet,
  transfer aset, upgrade kontrak, perubahan ownership.
- Manajemen kunci yang tidak pernah mengekspos private key ke model
  (signer terpisah, dukungan hardware wallet / KMS).
- Audit trail lengkap: setiap aksi on-chain tercatat dan bisa direplay.

*Kenapa ini moat:* reputasi keamanan terakumulasi lambat dan hancur sekali
insiden. Menjadi agent yang "belum pernah menghilangkan dana pengguna" adalah
aset yang tidak bisa disalin dengan fork kode.

### Langkah 3 — Moat Data & Evaluasi: benchmark dan korpus Solana

- Bangun korpus terkurasi: program Solana teraudit, pola kerentanan (SWC
  registry yang relevan, laporan audit publik ekosistem Solana), idiom per
  protokol (Anchor, Pinocchio).
- Rilis benchmark publik untuk tugas agent di program Solana (perbaiki
  kerentanan, tulis test invariant, deploy ke devnet/mainnet) — jadikan
  Altius standar pengukurannya, seperti arc-agi-crystalline membuktikan diri
  lewat benchmark ARC.

*Kenapa ini moat:* siapa yang memiliki benchmark memiliki definisi "bagus"
di kategorinya; data terkurasi tidak ikut ter-fork bersama kode.

### Langkah 4 — Moat Memori: belajar dari kegagalan lintas sesi

Adopsi pelajaran arc-agi-crystalline dan hermes-agent: memori yang menyimpan
*kenapa sebuah percobaan gagal*, bukan sekadar riwayat chat.

- Memori prosedural per-proyek: pola error build, konvensi repo, keputusan
  arsitektur yang sudah diambil.
- Memori domain lintas-proyek: pola gas optimization yang berhasil, jebakan
  per-chain (nonce, reorg, gas estimation) yang pernah ditemui.
- Skill yang dikurasi dari pengalaman (learning loop ala Hermes), dapat
  dibagikan antar pengguna sebagai paket.

*Kenapa ini moat:* nilai produk naik seiring pemakaian (switching cost);
agent pesaing mulai dari nol di setiap proyek.

### Langkah 5 — Moat Ekosistem: skills & MCP marketplace khusus Solana

- Skills per-protokol Solana (Jupiter, Marinade, dsb.) yang bisa dibuat
  komunitas — meniru mekanika plugin claude-code/grok-build tapi dengan
  fokus vertikal Solana, bukan lintas-chain.
- MCP server siap pakai untuk RPC node Solana, indexer on-chain, Solana
  Explorer, dan price oracle Solana (Pyth, dsb.).
- Insentif kontributor lewat norma web3 (bounty, grant dari foundation
  chain) — jalur pendanaan komunitas yang tidak dimiliki kompetitor umum.

*Kenapa ini moat:* efek jaringan dua sisi (pembuat skill ↔ pengguna) sulit
di-bootstrap ulang oleh pendatang.

### Langkah 6 — Moat Keterbukaan: interoperable, bukan lock-in

Belajar dari kemenangan opencode/hermes/openclaw atas produk tertutup:

- Tetap open-source (Apache 2.0 sudah tepat), model-agnostic (Claude, GPT,
  Grok, model lokal), dan dukung penuh ACP + MCP.
- Mode headless untuk CI: "audit otomatis di setiap PR kontrak" adalah wedge
  distribusi ke tim, bukan hanya individu.
- Self-hosted penuh untuk tim yang menuntut kedaulatan data (pelajaran
  openclaw) — penting bagi protokol yang kodenya sensitif pra-audit.

*Kenapa ini moat:* menghilangkan alasan utama pengguna menolak (vendor
lock-in) sekaligus menjadikan Altius pilihan default yang aman secara
politik di komunitas open-source/web3.

## 3. Urutan Eksekusi

| Fase | Fokus | Hasil yang membuktikan moat |
|---|---|---|
| 0–3 bulan | Langkah 1 & 2 (vertikal + guardrail) | Alur Foundry/Hardhat end-to-end dengan simulasi wajib; nol insiden dana |
| 3–6 bulan | Langkah 3 & 6 (benchmark + headless CI) | Benchmark web3-agent publik v1; audit-in-CI dipakai ≥10 protokol |
| 6–12 bulan | Langkah 4 & 5 (memori + marketplace) | Memori lintas sesi aktif; ≥50 skill komunitas di marketplace |

## 4. Metrik Moat

- **Kedalaman vertikal:** % tugas Solana di benchmark yang diselesaikan vs kompetitor generalis (bukan rata-rata lintas chain — lihat §0).
- **Kepercayaan:** jumlah transaksi mainnet dieksekusi tanpa insiden; jumlah temuan audit yang dicegah.
- **Retensi:** switching cost terukur — berapa banyak konteks/memori/skill per pengguna aktif.
- **Ekosistem:** jumlah skill pihak ketiga dan MCP server aktif per bulan.

## 5. Apa yang Sengaja TIDAK Dilakukan

- Tidak berlomba jumlah kanal pesan (WhatsApp/Telegram dst.) melawan
  openclaw/hermes — bukan medan yang relevan untuk developer tool.
- Tidak berlomba kualitas model frontier melawan Anthropic/xAI — Altius
  netral model, biarkan lab bersaing dan Altius memakai yang terbaik.
- Tidak membangun IDE/desktop app sendiri sebelum vertikal menang — ACP
  sudah memberi jalur masuk ke editor dengan biaya kecil.
