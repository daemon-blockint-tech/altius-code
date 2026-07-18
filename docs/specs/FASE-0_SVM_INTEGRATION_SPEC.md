# Spesifikasi Teknis — Fase 0–3 Bulan: Integrasi SVM + Guardrail Simulasi Wajib

Turunan dari [Strategi Moat](../MOAT_STRATEGY.md) Langkah 1 (moat vertikal) dan
Langkah 2 (moat kepercayaan), difokuskan pada ekosistem **Solana/SVM**:
program Rust native, **Anchor**, dan **Pinocchio**.

## 1. Tujuan & Lingkup

**Tujuan:** dalam 3 bulan, Altius Code mampu menjalankan siklus penuh
pengembangan program Solana — deteksi proyek → build → test → analisis
keamanan → deploy — dengan jaminan bahwa **tidak ada transaksi on-chain yang
dieksekusi tanpa melewati simulasi dan kebijakan guardrail**.

**Dalam lingkup (in-scope):**
- Deteksi & dukungan toolchain: Rust native (`solana-program`), Anchor,
  Pinocchio, `cargo build-sbf`, Solana CLI (Agave), `solana-test-validator`.
- Test harness: unit test cepat (LiteSVM / Mollusk), integration test
  (`solana-program-test` / bankrun), `anchor test`.
- Guardrail transaksi (TxGuard): simulasi wajib, kebijakan cluster,
  konfirmasi manusia, audit log.
- Isolasi signer: agent tidak pernah membaca material kunci privat.

**Di luar lingkup (Fase berikutnya):** EVM/chain lain, marketplace skills,
memori lintas sesi, benchmark publik, otomasi mainnet tanpa manusia.

## 2. Arsitektur

```
┌────────────────────────── Altius Agent Core ──────────────────────────┐
│                                                                       │
│  ┌───────────────┐   ┌───────────────┐   ┌─────────────────────────┐  │
│  │ SVM Project    │   │ Tool Adapters │   │ TxGuard                 │  │
│  │ Detector       │──▶│ (anchor/cargo/ │──▶│ policy → simulate →    │  │
│  │                │   │  solana-cli)  │   │ diff → approve → log   │  │
│  └───────────────┘   └───────────────┘   └───────────┬─────────────┘  │
│                                                      │                │
└──────────────────────────────────────────────────────┼────────────────┘
                                                       ▼
                                            ┌─────────────────────┐
                                            │ Signer Service      │
                                            │ (proses terpisah;   │
                                            │  keypair/HW wallet) │
                                            └─────────────────────┘
```

Empat komponen baru, semuanya crate Rust di workspace Altius:

| Crate | Tanggung jawab |
|---|---|
| `altius-svm-detect` | Deteksi jenis proyek & toolchain yang terpasang |
| `altius-svm-tools` | Adapter perintah build/test/deploy per framework |
| `altius-txguard` | Pipeline guardrail untuk semua transaksi on-chain |
| `altius-signer` | Proses signer terisolasi (IPC), abstraksi keypair/HW wallet |

## 3. `altius-svm-detect` — Deteksi Proyek

Berjalan saat sesi dibuka pada direktori kerja. Aturan deteksi (berurutan):

1. **Anchor**: ada `Anchor.toml` → baca `[programs.*]`, `[provider]`
   (cluster & wallet path — wallet path dicatat tapi TIDAK dibaca isinya).
2. **Pinocchio**: `Cargo.toml` memuat dependency `pinocchio` →
   proyek Pinocchio (build via `cargo build-sbf`, tanpa IDL).
3. **Rust native**: dependency `solana-program`/`solana-sdk` +
   `crate-type = ["cdylib", "lib"]` → program native.
4. Selain itu → bukan proyek SVM; fitur SVM tidak aktif.

Hasil deteksi berupa `SvmProject`:

```rust
pub struct SvmProject {
    pub framework: Framework,        // Anchor | Pinocchio | Native
    pub programs: Vec<ProgramInfo>,  // nama, path, program_id (jika ada)
    pub toolchain: Toolchain,        // versi solana-cli, anchor-cli, rustc
    pub default_cluster: Cluster,    // dari Anchor.toml / config; default: Localnet
}
```

Toolchain yang hilang dilaporkan ke pengguna beserta perintah pemasangan
(mis. `agave-install`, `avm install`), tidak dipasang diam-diam.

## 4. `altius-svm-tools` — Adapter Build/Test/Deploy

Satu trait untuk semua framework agar agent memakai antarmuka seragam:

```rust
pub trait SvmToolchain {
    fn build(&self) -> Result<BuildArtifacts>;   // .so + IDL (Anchor)
    fn unit_test(&self) -> Result<TestReport>;    // LiteSVM / Mollusk
    fn integration_test(&self) -> Result<TestReport>;
    fn lint(&self) -> Result<LintReport>;         // clippy + lint SVM khusus
    fn deploy(&self, cluster: Cluster) -> Result<TxRequest>; // TIDAK eksekusi
}
```

Pemetaan per framework:

| Operasi | Anchor | Pinocchio / Native |
|---|---|---|
| Build | `anchor build` | `cargo build-sbf` |
| Unit test | Mollusk / LiteSVM | Mollusk / LiteSVM |
| Integration | `anchor test` (localnet) | `solana-program-test` |
| Deploy | `anchor deploy` → dicegat | `solana program deploy` → dicegat |

**Penting:** `deploy()` tidak pernah mengeksekusi transaksi. Ia hanya
menghasilkan `TxRequest` (transaksi belum ditandatangani + metadata) yang
wajib melewati TxGuard (§6). Adapter memblokir jalur pintas: perintah shell
mentah yang cocok dengan pola `solana program deploy|write-buffer`,
`solana transfer`, `anchor deploy|upgrade|migrate` dialihkan ke TxGuard.

**Lint SVM khusus** (di atas clippy + `cargo-audit`), minimal 6 aturan v1:
missing signer check, missing owner check, arbitrary CPI, akun writable tanpa
validasi, integer overflow pada perhitungan lamports, `close` account tanpa
zeroing. Untuk Anchor sebagian sudah dijaga macro-nya — lint menyesuaikan
per framework.

## 5. Test Harness

- **Unit (cepat, default):** LiteSVM/Mollusk di dalam `cargo test` — tanpa
  validator, sub-detik, dipakai agent untuk iterasi.
- **Integration:** `solana-test-validator` dikelola oleh Altius sebagai
  managed process (start/stop/reset, port acak, log ditangkap). Mendukung
  `--clone <pubkey> --url mainnet-beta` untuk menghadirkan akun/program
  mainnet ke localnet (dasar simulasi fork di §6).
- Agent membaca `TestReport` terstruktur (bukan parsing stdout bebas):
  nama test, status, compute units terpakai, log program.

## 6. `altius-txguard` — Guardrail Simulasi Wajib

**Invarian utama: tidak ada `TxRequest` yang mencapai Signer Service tanpa
melewati kelima tahap berikut secara berurutan dan lulus semuanya.**

```
TxRequest → [1 Policy] → [2 Simulate] → [3 Diff] → [4 Approve] → [5 Log] → Signer
```

### Tahap 1 — Policy check (statis)

Kebijakan dibaca dari `altius.toml` proyek, dengan default aman bila absen:

```toml
[svm.policy]
allowed_clusters   = ["localnet", "devnet"]  # default; mainnet & testnet opt-in
mainnet            = "require-approval"       # forbid | require-approval (tidak ada "auto")
max_lamports_out   = 100_000_000              # batas transfer keluar per-tx
deny_instructions  = ["SetAuthority", "Upgrade", "CloseAccount"]  # perlu approval eksplisit
```

Aturan yang tidak bisa dimatikan lewat konfigurasi (hard rules):
- Nilai `mainnet = "auto"` tidak dikenal — approval manusia untuk
  mainnet-beta tidak bisa dinonaktifkan.
- Perubahan upgrade authority, penutupan program, dan transfer aset di
  mainnet selalu `require-approval`.

### Tahap 2 — Simulasi wajib

- **Localnet/devnet:** `simulateTransaction` (sigVerify off, replaceRecentBlockhash)
  terhadap cluster target.
- **Mainnet:** simulasi dua lapis — (a) `simulateTransaction` ke RPC mainnet,
  dan (b) replay di localnet fork (`solana-test-validator --clone` semua akun
  yang disentuh transaksi). Keduanya harus lulus.
- Simulasi gagal (error program, compute budget terlampaui, akun hilang)
  → transaksi DITOLAK; agent menerima log lengkap untuk diperbaiki, bukan
  opsi "lanjutkan saja".

### Tahap 3 — Diff report

Dari hasil simulasi, TxGuard menyusun ringkasan yang dapat dibaca manusia:

- Δ lamports per akun (siapa membayar apa, termasuk fee & rent).
- Akun yang dibuat/ditutup/di-realloc; perubahan owner & authority.
- Program yang di-CPI beserta kedalamannya.
- Compute units terpakai vs limit.

### Tahap 4 — Approval

- Aksi non-ireversibel di localnet/devnet: auto-approve (dicatat).
- Aksi ireversibel ATAU cluster mainnet: prompt konfirmasi eksplisit ke
  pengguna menampilkan diff report Tahap 3. Di mode headless, tanpa kanal
  approval interaktif → transaksi ditolak (fail-closed), bukan ditunda.

### Tahap 5 — Audit log

Append-only JSONL di `.altius/txlog/`: `TxRequest`, hasil simulasi, diff,
keputusan approval (siapa/kapan), signature transaksi final. Setiap entri
memuat hash entri sebelumnya (tamper-evident). Perintah `altius tx replay`
dapat menjalankan ulang simulasi entri mana pun.

## 7. `altius-signer` — Isolasi Kunci

- Proses terpisah dari agent; komunikasi via IPC (unix socket) dengan API
  sempit: `pubkey()`, `sign(message) -> signature`. Tidak ada API ekspor kunci.
- Backend v1: file keypair (path dikonfigurasi pengguna, dibaca hanya oleh
  proses signer), dukungan Ledger via `solana-remote-wallet`.
- Guardrail sisi agent: tool `Read`/`Grep`/shell agent menolak path yang
  cocok pola keypair (`*.json` berisi array 64 byte, `id.json` di
  `~/.config/solana/`, path wallet dari `Anchor.toml`), dan output perintah
  di-scan untuk pola kunci privat sebelum masuk konteks model.

## 8. Milestone & Kriteria Penerimaan

| Bulan | Deliverable | Kriteria penerimaan |
|---|---|---|
| 0–1 | `altius-svm-detect` + `altius-svm-tools` (build & unit test) | Deteksi benar pada ≥20 repo publik sampel (Anchor/Pinocchio/native); agent bisa build & menjalankan unit test ketiga framework end-to-end |
| 1–2 | Test harness terkelola + lint SVM + `altius-signer` v1 | `solana-test-validator` dikelola penuh (start/clone/reset); 6 aturan lint aktif dengan <10% false positive pada korpus sampel; kunci privat tidak pernah muncul di konteks model (diverifikasi test red-team otomatis) |
| 2–3 | `altius-txguard` lengkap + deploy devnet/mainnet | Kelima tahap pipeline aktif; uji coba: 100 deploy devnet + 5 deploy mainnet terpandu tanpa insiden; mutation test membuktikan tidak ada jalur kode yang mencapai signer tanpa melewati TxGuard |

## 9. Metrik Fase

- **Keamanan:** 0 transaksi terkirim tanpa simulasi (invarian, diuji CI);
  0 insiden dana; 100% aksi ireversibel melalui approval manusia.
- **Kecepatan:** unit test loop < 5 detik; siklus build+test+lint Anchor
  standar < 90 detik.
- **Cakupan:** ketiga framework lulus e2e suite yang sama.

## 10. Risiko & Mitigasi

| Risiko | Mitigasi |
|---|---|
| Simulasi mainnet tidak identik dengan eksekusi nyata (state berubah antara simulasi & kirim) | Simulasi ulang sesaat sebelum submit + batas umur blockhash; dokumentasikan jendela risiko ke pengguna |
| Perubahan cepat toolchain Solana (Agave, versi Anchor) | Matriks versi yang didukung + CI terhadap versi stable & sebelumnya |
| Agent menemukan jalur pintas shell yang tidak terdeteksi pola intersepsi | Defense-in-depth: signer terpisah tetap satu-satunya pemegang kunci — tanpa tanda tangan, tidak ada transaksi |
| False positive lint mengganggu alur | Lint SVM sebagai warning (bukan blocker) kecuali kategori signer/owner check |
