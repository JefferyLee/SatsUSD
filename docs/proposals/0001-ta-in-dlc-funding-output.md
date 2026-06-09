# Taproot Assets in Taproot+MuSig2 DLC Funding Outputs
# 在 Taproot+MuSig2 DLC funding output 中承载 Taproot Assets 资产

**Status / 状态:** Draft for community review / 公开征求意见草案
**Author / 作者:** Jeffery Lee (SatUSD project)
**Target venues / 投递目标:**
- English: dlcspecs (GitHub), delvingbitcoin.org, lightning-dev mailing list
- 中文: BitcoinTalk 中文区, Delving Bitcoin Chinese, 各 BTC L2 团队 issue
**Date / 日期:** 2026-06-07

---

# Part I — English

## Abstract

We sketch a construction that lets a single Bitcoin UTXO act simultaneously as
(a) a Taproot Assets-committed output carrying a non-BTC asset (e.g. a
stablecoin issued under BIP-tap), and (b) a Discreet Log Contract (DLC)
funding output of the new Taproot + MuSig2 style. The construction relies on
the fact that the Taproot script tree of a key-path-spent DLC funding output
is conventionally empty *by choice*, not by protocol requirement, and that
BIP-tap places its asset commitment in a tap leaf whose role is inclusion
proof rather than spending path. We outline the funding transaction, CET
structure, PSBT field requirements, and the verifier-side recognition
problem. We ask the community to weigh in on three specific open questions
before this becomes a real spec.

## 1. Motivation

Taproot Assets (BIP-tap, formerly Taro) makes it practical to issue
USD-pegged stablecoins natively on Bitcoin L1. DLCs make it practical to
settle contingent payments against an oracle attestation without trusting
any custodian. The natural fusion — *redeem a TA-issued stablecoin for BTC
at an oracle-attested price, in a single non-custodial settlement* — has
been mentioned in passing in the literature (e.g. Conduition's *DLC Factory*
note, January 2025; DLC Markets's product roadmap) but, to our knowledge,
no public technical specification has been published.

This proposal is a first attempt at that specification, written from the
implementer's perspective. The motivation is concrete: the SatUSD project is
building a BTC-collateralized, TA-issued stablecoin on Bitcoin L1, and needs
a settlement primitive that does not introduce a new bridge, custodian, or
rollup.

## 2. Background recap

### 2.1 BIP-tap commitment structure

A Taproot Assets UTXO is a standard P2TR output whose Taproot tweak commits
to an asset state:

```
Q = P_internal + TaggedHash("TapTweak", P_internal ‖ MerkleRoot) · G
```

where `MerkleRoot` is the root of a tap script tree containing at least one
**asset commitment leaf**:

```
asset_commitment_leaf = TapLeaf(version, leaf_script)
leaf_script = encode(asset_version, asset_id, MS-SMT_root, …)
```

A TA verifier proves the asset's presence by exhibiting the leaf and a
BIP-341 control block; the leaf is *never required to be the script path
used for spending*. In typical TA usage the internal key is a NUMS point
that disables key-path spending, but **this is convention, not protocol**.

### 2.2 Taproot + MuSig2 DLC funding output

The newer DLC style (used in Lightning's taproot channel upgrade and in
several DLC implementations since 2023) replaces the legacy dlcspecs
P2WSH 2-of-2 funding output with:

```
P_internal  = MuSig2_KeyAgg(LP_pubkey, User_pubkey)
script_tree = ∅
Q           = P_internal + TaggedHash("TapTweak", P_internal ‖ ∅) · G
spending    = key-path only
```

Each CET corresponds to one oracle outcome and is unlocked by an adaptor
signature whose secret is precisely the oracle's anticipated signature for
that outcome.

## 3. The construction

The proposal is one observation followed by a construction.

**Observation.** The Taproot + MuSig2 DLC funding output uses key-path
spending and chooses an empty script tree *for efficiency*, not because the
DLC protocol requires it. BIP-341 places no constraint on the script tree
content provided key-path spending remains valid. Therefore, a single tap
leaf carrying a BIP-tap asset commitment can be placed in the script tree
of a DLC funding output without breaking either protocol's spending
semantics.

### 3.1 Funding output

```
internal key  P  = MuSig2_KeyAgg(LP_pubkey, User_pubkey)
script tree   T  = { ta_commit_leaf, refund_leaf }
                   where
                     ta_commit_leaf = BIP-tap asset commitment for $X SatUSD
                     refund_leaf    = <CSV timelock> 2-of-2 OP_CHECKMULTISIG
merkle root   M  = MerkleRoot(T)
tweak         t  = TaggedHash("TapTweak", P ‖ M)
output key    Q  = P + t · G
```

The funding transaction takes two inputs:

| Input | Source | Purpose |
|---|---|---|
| `in₀` | User's TA UTXO carrying $X of the stablecoin | The "USD side" |
| `in₁` | LP's BTC UTXO of value `Y ≥ ⌈$X / p_min⌉ + dust + fee_budget` | The "BTC side" |

It produces one funding output of value `Y` BTC and TA-committed for $X.

### 3.2 CET set

For each of N discretized oracle outcomes (a price bucket `p_i` at the
specified attestation event), the parties pre-sign an adaptor signature
binding to a CET with these outputs:

| Output | Recipient | Payload |
|---|---|---|
| `o_user` | User | BTC value = `⌊$X / p_i⌋` |
| `o_sink` | NUMS-derived burn address | TA-committed: asset_id = SatUSD, amount = $X |
| `o_lp` | LP | BTC remainder = `Y − ⌊$X / p_i⌋ − dust − fee` |

`o_sink` is a P2TR with internal key fixed to a NUMS point chosen per
BIP-tap convention; this constitutes a verifiable burn at the TA layer.

### 3.3 Settlement

When the oracle publishes its signature for outcome `p_actual`, *any*
broadcaster — LP, User, or a third-party relay — can:

1. Decrypt the matching adaptor signature using the oracle signature.
2. Combine with the MuSig2 partial signatures to obtain a complete Schnorr
   signature for `Q`.
3. Broadcast `CET_{p_actual}`.

The settlement is final the moment `CET_{p_actual}` is included in a block.
TA lineage is verified out-of-band by the receiving wallet (or by a
standalone TA verifier) by reading the asset transfer proof for `in₀` and
checking that `o_sink` preserves `asset_id` and `amount`.

### 3.4 Refund

If no settlement is broadcast within the validity window, the `refund_leaf`
script path is taken after its CSV timelock expires. The refund tx returns
TA to the user (carrying the asset commitment forward to a user-controlled
output) and BTC to the LP.

## 4. PSBT requirements

Constructing and coordinating the funding transaction needs a PSBT carrying
fields from three orthogonal specs simultaneously:

| Provenance | Fields |
|---|---|
| BIP-174 base | inputs, outputs, signatures |
| BIP-371 Taproot | `PSBT_OUT_TAP_INTERNAL_KEY`, `PSBT_OUT_TAP_TREE` |
| BIP-tap (PSBT extension, see PR #1489) | TA asset proof for each TA-committed input/output |
| dlcspecs (proposed Taproot extension) | per-CET adaptor signature, oracle announcement digest, refund descriptor |

We propose that the DLC-side and TA-side PSBT fields be defined as
*proprietary* (BIP-174 §`PSBT_*_PROPRIETARY`) until each upstream group
adopts a standard key prefix. SatUSD's reference implementation will use:

```
PSBT_OUT_PROPRIETARY  key prefix = "SatUSD/v1/"
  subkeys:
    "dlc_cet/<i>"          — i-th CET adaptor signature + oracle nonce binding
    "dlc_refund"           — CSV-locked refund descriptor
    "ta_proof_in/<vin>"    — incoming TA proof file
    "ta_proof_out/<vout>"  — outgoing TA proof file
```

This is a placeholder — we believe the right long-term home is
`PSBT_*_DLC_*` keys defined by dlcspecs and `PSBT_*_TAP_ASSET_*` keys
defined by BIP-tap, harmonized so the two ranges do not collide.

## 5. Verifier-side recognition

Today's `tapd` recognizes a TA-committed output by checking that the
witness reveals a tap leaf matching its expected asset commitment when the
output is spent via script path. A DLC funding output is spent via key
path; the asset commitment leaf is never revealed in-band.

Three implementation strategies are available, in increasing order of
ecosystem disruption:

1. **Out-of-band proof retrieval.** The asset transfer proof file for a
   TA-committed UTXO already travels out-of-band via TA universe servers.
   The verifier reads the proof directly, does not rely on `tapd`'s
   UTXO-recognition heuristic. This works today with no upstream changes.
2. **`tapd` extension** to recognize TA leaves in key-path-spent outputs by
   inspecting the BIP-371 `PSBT_OUT_TAP_TREE` field (when constructing) or
   the announced merkle root (when scanning). PR-able to upstream.
3. **Standardize via BIP-tap revision**: explicitly bless the "TA leaf
   alongside non-asset leaves, possibly key-path spent" case in the spec.

We will proceed with (1) for the SatUSD PoC and propose (2) in parallel.

## 6. Open questions for the community

We would value feedback specifically on these three points before this
hardens into a spec PR.

**Q1.** Does BIP-tap want to bless the "asset commitment in a key-path-spent
Taproot output" case explicitly, or treat it as an out-of-spec usage that
verifiers may opt into? We see no protocol-level obstruction but the
ecosystem assumption today is script-path spending of TA UTXOs.

**Q2.** What is the right cross-spec arrangement of PSBT keys? Naïvely
prefixing both TA and DLC fields as proprietary works for a single project
but does not compose if a wallet needs to handle both upstream-standard TA
PSBTs and DLC PSBTs concurrently with these hybrid ones.

**Q3.** MuSig2 nonce safety in long-lived pre-signing scenarios — for an
N-CET DLC with N in the low thousands, each adaptor signature consumes one
nonce per signer. Are existing deterministic-nonce constructions
(BIP-327 Annex) sufficient, or does this use case warrant a dedicated
nonce-derivation extension to prevent reuse across CET re-signings?

## 7. Reference and prior art

- *Discreet Log Contract Factories*, Conduition, January 2025 — mentions a
  "stablecoin-like" use case but does not specify TA integration.
- *Wrapless: trustless Bitcoin lending* (arXiv 2507.06064, July 2025) —
  uses DLCs for BTC-collateralized stablecoin loans; the stablecoin side is
  external, not TA.
- *OP-DLC 2*, Bitlayer Blog, 2025 — adds optimistic challenge to DLC oracle
  signatures; orthogonal to TA integration but composable with this
  proposal as a fallback dispute layer.
- *Lava Loans V2* — DLC-based BTC-collateralized stablecoin loans, stablecoin
  side is non-TA.
- DLC Markets has publicly stated intent to settle in USD via Taproot
  Assets; no technical document published as of the date of this draft.

## 8. Next steps

The SatUSD project intends to:

1. Build a PoC implementation of Section 3 against signet, single-oracle.
2. Publish reference vectors for the funding tx, a representative CET, and
   the refund tx.
3. Submit `tapd` PR for strategy (2) of Section 5.
4. Open a dlcspecs issue (or PR if scope warrants) proposing the
   Taproot+MuSig2 funding output as a peer to the existing P2WSH version,
   with an optional TA-aware variant.

Comments, corrections, and alternative constructions are very welcome.
The SatUSD repository will track this proposal at
`docs/proposals/0001-ta-in-dlc-funding-output.md`; mailing-list and
delvingbitcoin replies will be backlinked there.

---

# Part II — 中文

## 摘要

本文勾画一种构造，使**一笔比特币 UTXO** 同时承担两个角色：(a) 一个携带
BIP-tap（Taproot Assets，原名 Taro）资产承诺的 UTXO，承载非比特币
资产（例如稳定币）；(b) 一个采用 Taproot + MuSig2 新规范的 DLC funding
output。构造的关键洞察是：新版 Taproot+MuSig2 DLC funding output 的
script tree 之所以为空，是**设计上的选择**而非协议要求；而 BIP-tap 的资产
承诺位于 tap leaf 中，其作用是**包含性证明**而非花费路径。因此两者天然可以
共存于同一个 Taproot tweak 之下。本文给出 funding tx 结构、CET 结构、PSBT
字段需求、以及验证者侧的识别问题，并在最后提出三个需要社区共同决断的开放
问题。

## 1. 动机

Taproot Assets 让在比特币 L1 上原生发行美元稳定币变得现实可行；DLC 让基于
预言机签名的有条件支付得以在无托管方的前提下结算。两者的自然融合——
**「将 TA 发行的稳定币按预言机签名的市场价直接兑换成 BTC，一次非托管结算」**
——在文献中曾被零星提及（如 Conduition 2025 年 1 月的 *DLC Factory* 一文；
DLC Markets 的产品路线图），但**就我们所知，迄今没有任何公开的技术规范**。

本文是这个方向上的首个公开技术草案，从实施者视角出发。具体动机：SatUSD
项目正在比特币 L1 上构建一个 BTC 抵押、TA 发行的稳定币，需要一个**不引入新
桥、新托管方、不依赖任何 rollup** 的结算原语。

## 2. 前置事实回顾

### 2.1 BIP-tap 资产承诺结构

一个 Taproot Assets UTXO 就是一个标准 P2TR 输出，其 Taproot tweak 承诺了
某种资产状态：

```
Q = P_internal + TaggedHash("TapTweak", P_internal ‖ MerkleRoot) · G
```

其中 `MerkleRoot` 是 tap script 树的根，树中至少包含一片 **资产承诺叶子**：

```
asset_commitment_leaf = TapLeaf(version, leaf_script)
leaf_script = encode(asset_version, asset_id, MS-SMT_root, …)
```

TA 验证者通过展示该 leaf 加上 BIP-341 control block，即可证明资产存在性；
**该 leaf 永远不要求作为实际的花费路径**。在 TA 的常见用法中，internal key
被设为 NUMS 点以禁用 key-path 花费，但**这是惯例而非协议要求**。

### 2.2 Taproot + MuSig2 DLC funding output

新一代 DLC（Lightning Taproot channel 升级、Farcaster RFC 以及 2023 年起的
多个 DLC 实现都采用）取代了 dlcspecs 旧规范的 P2WSH 2-of-2 funding output：

```
P_internal  = MuSig2_KeyAgg(LP_pubkey, User_pubkey)
script_tree = ∅
Q           = P_internal + TaggedHash("TapTweak", P_internal ‖ ∅) · G
spending    = 仅 key-path
```

每一个 CET 对应一个 oracle outcome；CET 通过 adaptor signature 解锁，其
解密密钥正是 oracle 对该 outcome 的预期签名。

## 3. 构造

提案 = **一个关键观察 + 一套构造**。

**关键观察。** 新版 Taproot+MuSig2 DLC funding output 用 key-path 花费、
script tree 为空，是出于**效率考量**，而非 DLC 协议要求。BIP-341 对 script
tree 的内容没有任何限制，只要 key-path 花费仍然有效。**因此，将一片
BIP-tap 资产承诺叶子放入 DLC funding output 的 script tree，既不破坏 DLC
的花费语义，也不破坏 TA 的资产证明语义。**

### 3.1 Funding output

```
内部公钥  P  = MuSig2_KeyAgg(LP_pubkey, User_pubkey)
script 树 T  = { ta_commit_leaf, refund_leaf }
              其中
                ta_commit_leaf = $X SatUSD 的 BIP-tap 资产承诺
                refund_leaf    = <CSV 时间锁> 2-of-2 OP_CHECKMULTISIG
Merkle 根 M  = MerkleRoot(T)
tweak     t  = TaggedHash("TapTweak", P ‖ M)
输出公钥  Q  = P + t · G
```

Funding tx 接受两个输入：

| 输入 | 来源 | 用途 |
|---|---|---|
| `in₀` | 用户的 TA UTXO，承载 $X 稳定币 | "USD 侧" |
| `in₁` | LP 的 BTC UTXO，`Y ≥ ⌈$X / p_min⌉ + dust + fee` | "BTC 侧" |

产出一笔 funding output：BTC 值 `Y`，TA 层承载 $X SatUSD。

### 3.2 CET 集合

对 N 个离散化预言机 outcome（即指定 attestation 事件上的价格 bucket
`p_i`），双方预签一组 adaptor signature，每个签名绑定到下列结构的 CET：

| 输出 | 接收方 | 内容 |
|---|---|---|
| `o_user` | 用户 | BTC 值 = `⌊$X / p_i⌋` |
| `o_sink` | NUMS 销毁地址 | TA-committed：asset_id = SatUSD，amount = $X |
| `o_lp` | LP | BTC 余额 = `Y − ⌊$X / p_i⌋ − dust − fee` |

`o_sink` 是一个 P2TR，internal key 固定为 BIP-tap 约定的 NUMS 点，这构成
TA 层面的可验证销毁。

### 3.3 结算

当预言机公开对 outcome `p_actual` 的签名后，**任何 broadcaster**（LP、用
户、或第三方 relay）都可以：

1. 用 oracle 签名解密对应的 adaptor signature。
2. 与 MuSig2 partial signatures 合成，得到 `Q` 的完整 Schnorr 签名。
3. 广播 `CET_{p_actual}`。

`CET_{p_actual}` 一旦入块即为最终结算。TA lineage 由接收钱包（或独立 TA
验证器）在带外验证：读取 `in₀` 的 asset transfer proof，并核对 `o_sink`
保留了同一个 `asset_id` 和 `amount`。

### 3.4 退款

若在有效窗口内无任何 settlement 广播，CSV 锁时长到期后 `refund_leaf`
路径可被使用。Refund tx 将 TA 退回用户（资产承诺在用户控制的输出中延续），
将 BTC 退回 LP。

## 4. PSBT 字段需求

构造与协调 funding tx 需要一份同时承载三套规范字段的 PSBT：

| 来源 | 字段 |
|---|---|
| BIP-174 基础 | 输入、输出、签名 |
| BIP-371 Taproot | `PSBT_OUT_TAP_INTERNAL_KEY`, `PSBT_OUT_TAP_TREE` |
| BIP-tap (PSBT 扩展, PR #1489) | 每个 TA-committed 输入/输出的资产证明 |
| dlcspecs（拟议 Taproot 扩展）| 每个 CET 的 adaptor signature、预言机公告 digest、refund 描述符 |

我们建议 DLC 侧和 TA 侧的 PSBT 字段在各上游小组采纳标准 key 前缀前，先以
**proprietary**（BIP-174 §`PSBT_*_PROPRIETARY`）形式定义。SatUSD 参考实现
将采用：

```
PSBT_OUT_PROPRIETARY  key 前缀 = "SatUSD/v1/"
  subkeys:
    "dlc_cet/<i>"          — 第 i 个 CET 的 adaptor signature + 预言机 nonce 绑定
    "dlc_refund"           — CSV 锁定的 refund 描述符
    "ta_proof_in/<vin>"    — 入站 TA 证明文件
    "ta_proof_out/<vout>"  — 出站 TA 证明文件
```

这是一个**占位**——我们认为长期归宿应该是 dlcspecs 定义的
`PSBT_*_DLC_*` keys + BIP-tap 定义的 `PSBT_*_TAP_ASSET_*` keys，两者范围
协调避免冲突。

## 5. 验证者侧的识别问题

当前 `tapd` 识别 TA-committed 输出的方式是：当输出通过 script-path 花费时，
检查 witness 揭示的 tap leaf 是否与预期资产承诺匹配。**而 DLC funding
output 通过 key-path 花费，资产承诺叶子永远不在见证中显式揭示**。

按对生态的侵入度递增，三种实现策略：

1. **带外获取证明**。TA UTXO 的 asset transfer proof file 本来就通过 TA
   universe 服务器带外传递。验证者直接读取 proof，不依赖 tapd 的 UTXO 识别
   启发式。**今天即可工作，无需任何上游改动**。
2. **`tapd` 扩展**：通过 BIP-371 `PSBT_OUT_TAP_TREE` 字段（构造时）或公开
   merkle root（扫链时）识别 key-path 花费输出中的 TA 叶子。可向上游提 PR。
3. **修订 BIP-tap 规范**：显式认可「TA leaf 与非资产 leaf 并存、可能
   key-path 花费」这种用法。

SatUSD PoC 阶段先走 (1)，同时并行推进 (2)。

## 6. 留给社区的开放问题

希望在本提案硬化为正式 spec PR 之前，社区能就以下三点提供反馈：

**Q1.** BIP-tap 是否要明确认可「资产承诺位于 key-path 花费的 Taproot 输出」
这种用法？或者是否仅视为超出规范的可选用法，由验证者各自决定支持？我们看
不到协议层面的障碍，但生态今天的默认假设是 TA UTXO 必走 script-path 花费。

**Q2.** PSBT key 的跨规范排布如何安排最合理？将 TA 和 DLC 字段都简单加
proprietary 前缀对单个项目有效，但如果一个钱包要同时处理上游标准 TA PSBT、
DLC PSBT 和本文这种混合 PSBT，proprietary 方案无法组合。

**Q3.** MuSig2 nonce 在长生命周期预签场景下的安全性——一个 N-CET 的 DLC，
N 在千数量级，每个 adaptor signature 消耗每方一个 nonce。现有的确定性
nonce 构造（BIP-327 附录）是否充分，还是这个用例需要一份专门的 nonce 派生
扩展，防止跨 CET 重签时的 nonce 重用？

## 7. 引用与相关工作

- *Discreet Log Contract Factories*, Conduition, 2025-01 —— 提到「stablecoin
  类」用例但未规定 TA 集成
- *Wrapless: trustless Bitcoin lending* (arXiv 2507.06064, 2025-07) —— 用
  DLC 做 BTC 抵押稳定币贷款，稳定币侧在外部链
- *OP-DLC 2*, Bitlayer Blog, 2025 —— 给 DLC 预言机签名加乐观挑战；与本文
  正交且可与本提案的兜底争议层组合
- *Lava Loans V2* —— DLC 抵押 BTC 借出稳定币，稳定币侧非 TA
- **DLC Markets** 公开表态计划用 Taproot Assets 做 USD 端结算；截至本草案
  日期尚无任何公开技术文档

## 8. 下一步

SatUSD 项目计划：

1. 基于 signet + 单点 oracle 完成第 3 节构造的 PoC 实现
2. 发布 funding tx、代表性 CET、refund tx 的参考向量
3. 为第 5 节策略 (2) 向 `tapd` 提 PR
4. 在 dlcspecs 开 issue（视范围决定是否 PR），提议 Taproot+MuSig2 funding
   output 作为现有 P2WSH 版本的平行规范，并附 TA-aware 可选变种

欢迎评论、修正、和替代构造。SatUSD 代码仓库将持续追踪本提案于
`docs/proposals/0001-ta-in-dlc-funding-output.md`；邮件列表 / Delving
Bitcoin 上的回复将在此处反向链接。

---

*This document is a draft for community feedback, not a final specification.
The construction has been validated on paper; PoC implementation is
underway.*

*本文是公开征求意见草案，非最终规范。构造在纸面上已验证；PoC 实现正在
推进中。*
