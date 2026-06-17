# 研究报告：赎回信任模型的竞品地图、Vitalik 期权提案、与 BitVM/前沿技术评估

- **日期**: 2026-06-17
- **状态**: research（非规范性；用于战略决策，不改变 spec）
- **作者**: Jeffery（方向）+ AI 研究 agent ×5（一手资料调研）
- **缘起**: 在 L1 上"期内 + 任意时刻 + 当前价 + 单边 + free-option=0"五者被证明不可兼得（见 [design journal](2026-06-design-journal.md) 的不可能性定理）之后，评估：(a) SatUSD 的定位是否被竞品占据；(b) Vitalik 的期权型稳定币提案与 SatUSD 的关系；(c) BitVM 2/3 及类似"无软分叉"技术能否替代缺失的操作码（CSFS）来消灭 free-option。
- **术语约定**: 文中英文术语首次出现即解释；标注【确认】= 一手论文/项目文档证实，【推断】= 工程判断，【未证实】= 资料未覆盖。

---

## 0. 总判断（TL;DR）

1. **Vitalik 2026-06-01 的"期权型、无清算合成资产"提案，在经济结构上与 SatUSD 同构**——P=持有人(稳定份)、N=LP(杠杆多头份)、P+N≡1 BTC、到期慢速预言机结算、无清算。SatUSD 可诚实宣称是"该期权积木的比特币原生实现"，但**不是**其完整稳定币（Vitalik 的完整版还需一层自动滚动换仓的 DAO，SatUSD 那层尚在路线图）。
2. **以太坊"拆分→稳定+杠杆"赛道已被 f(x) Protocol 占据**（且 f(x) V2 反而重新引入了清算）；"期权型"新线（Vitalik 启发）全是测试网早期、卡在 rebalance 滑点。**搬去以太坊 = 丢掉比特币原生护城河、进拥挤场。不推荐。**
3. **比特币"无发行人 + 无清算 + 自托管 + BTC 背书 + 单边期权/到期赎回"这个精确格子——没有任何活产品五条全中**。最接近的 10101（已死、机制不同）和 conduition DLC-factory（仅论文）。**格子真空着**；但需求侧未被验证（10101 死于卖不动），且"自托管+无清算"工程难（Lava 退回托管、Yala 被盗）——难度即护城河。
4. **BitVM 2/3 能在原理上模拟缺失的 CSFS，但整套套用会引入签名者委员会 + 挑战者 + operator 垫资 + 数天至数周窗口，直接背叛 no-issuer/单边/不可冻结**——这正是项目早先把 BitVM 架构（ADR-0018/ADR-007，已归档）简化为现 DLC 模型的原因。值得拿走的只是**最小原语**（bit commitment + 单步欺诈证明）。
5. **今天可搭、且信任代价远小于"发行人"的 free-option 缓解路**至少有两条：(a) **timelock 加密让授权按窗口过期**（代价：信任 drand 门限 + 预言机按窗口操作）；(b) **预言机 adaptor 短时效签名**（代价：信任预言机 liveness）。纯硬密码学的理想仍要等 CSFS 或经审计的见证加密(WE)。

---

## 1. Vitalik 的期权型提案 = SatUSD 的经济模型

**一手来源**：Vitalik 本人，ethresear.ch，《Building index-tracking assets on top of options instead of debt》，2026-06-01（CoinDesk/The Block/The Defiant 多源确认署名本人）。

**机制（公式原文确认）**：
- 1 ETH 随时拆成一对 (P, N)，随时合回。参数：行权价 S、到期日 M。
- 到期预言机读价 x：**P 拿 `min(1, S/x)` ETH，N 拿 `max(0, 1−S/x)` ETH**。
- **P + N ≡ 1 ETH → 结构上不可能资不抵债 → 不存在清算（liquidation：抵押跌破阈值被强制平仓）。**
- 数值直觉（S=1500）：价 >1500 时 **P 永远恰值 $1500（完美合成美元）**、N 吃 1500 以上全部上涨（杠杆多头）；价 <1500 时 P 退化为裸 ETH、N 归零。
- **期权分解**：P ≈ 备兑看涨（covered call：持标的 + 卖出行权价 S 的看涨期权）；N ≈ 买入该看涨期权。
- **核心论证**：因无强制清算，系统只需**到期读一次预言机**，故可用**慢速预言机（slow oracle，容忍延迟、可走争议/人工复核）**，免疫实时喂价被闪电贷操纵。

**与 SatUSD 逐点对照（经济结构同构，已确认）**：持有人=P，LP=N，两份之和=1 BTC，DLC（保密对数合约：预言机对结果签一次名解锁对应预签交易）天生即"到期单次、慢速预言机"范式。

**唯一 gap（诚实）**：Vitalik 完整愿景需在单期积木上叠一层**自动滚动换仓的 DAO（rebalance wrapper）**才得长期稳定币；SatUSD 实现的是单期积木，滚动续期=已 PARK 的 DLC channels。**可宣称口径**："SatUSD 是 Vitalik 期权积木的比特币原生实现"；**不可宣称**："实现了 Vitalik 的完整稳定币"。

---

## 2. 以太坊分级/期权稳定币赛道

| 项目 | 机制 | 无清算? | 信任模型 | 状态/TVL |
|---|---|---|---|---|
| **f(x) Protocol** | stETH/WBTC 拆成 fxUSD(稳)+xPOSITION(杠杆) | V1 是；**V2 重新引入清算 + 坏账社会化** | 实时预言机 + 可升级合约 + veFXN 治理 + 强依赖 Aave/Morpho/Curve | 活，~$9000 万（赛道最大） |
| **Tranchess** | QUEEN=BISHOP(稳)+ROOK(2x)，内部借贷 | **真无清算**（rebalance 重置净值） | 30 分钟 TWAP（慢速预言机）+ veCHESS | 活但萎缩，~$560 万（两年 −97%） |
| **Pendle** | PT(本金)+YT(收益)——拆**收益**非**波动** | 无（到期机制） | 自定义 AMM | ~$11.9 亿（最大，但不同类） |
| **BarnBridge SMART Alpha** | 价格波动分级 jETH/mETH/sETH | n/a | Chainlink 实时 + DAO | **死于 SEC（未注册证券，2023）** |
| **Vitalik 期权线**（Split/Cleave/Sir Trading/Gnosis CTF） | P+N≡1 慢速预言机 | 是 | 各异 | **全是测试网/早期，无规模 TVL** |

**判断**：债务型拆分（f(x) 那种）已占满；期权型线 2026-06 才被定义、白空间大但卡在 **rebalance 滑点**（Vitalik 自承年化可漏 2%+）。**搬到以太坊丢掉的是**：无发行人、无包装抵押（wstETH/WBTC/桥全有发行人）、无可升级管理员、无 DeFi 乐高传染——即 SatUSD 的全部信任优势。

---

## 3. 比特币"无发行人无清算美元"竞品扫描（最关键）

筛选标准（与门，五条全中才算直接竞品）：**无发行人 + 无清算 + 自托管 + BTC 背书 + 持有人可单边的期权/到期赎回**。

| 项目 | 无发行人? | 无清算? | 自托管? | 败在哪 |
|---|---|---|---|---|
| **Ducat (UNIT)** | 半 | 否(135%清算) | 是 | CDP；**也喊"L1自托管"→最易混淆** |
| bitSmiley / Satoshi / Yala / Avalon / BOB | 多为DAO | 否(全清算) | 多为否 | CDP 阵营 |
| Hermetica (USDh) / Stablesats | 是 | 变相强平 | **否(交易所对手方)** | delta-neutral；占用"合成美元"一词 |
| USDB / Citrea ctUSD / Tether-on-RGB | **是(持牌)** | 否 | 否 | 法币背书 |
| **10101**（历史） | **是** | 否→有清算 | **是** | 永续+清算+需对手方，**2024/9 死** |
| **conduition DLC-factory**（研究） | **是** | **是** | **是** | **机制几乎与 SatUSD 一模一样，但仅论文** |

**裁决**：五条全中的活产品 **零个**。最接近的 10101 已死且机制不同；机制双胞胎 conduition 仅是 scriptless 研究文章——**这反而背书了 SatUSD 路线的正确性，同时格子仍空。**

**清醒警告**：(1) **需求侧未验证**（10101 死于增长不足，是市场风险非技术风险）；(2) **"自托管+无清算"工程/安全都难**（Lava 2025 底退回托管、Yala 被盗脱锚至 $0.20）——反过来，真做扎实即稀缺壁垒。

**对外区隔话术**：对 Ducat 强调"**无清算**"；对 Hermetica/Stablesats 强调"**BTC 背书、预言机结算，非 delta-neutral**"；对 USDB 强调"**无发行人、无法币储备**"。

---

## 4. BitVM 2/3 及类似技术 —— 能否替代缺失的操作码？

**项目自身历史（已确认）**：SatUSD 早先有一整套 BitVM 架构——`docs/archive/decisions/ADR-0018`（BitVM2 fallback + 覆盖 backend 抽象 + 顾问门 G5/G6）、`ADR-007`（BitVM3 争议架构 + lineage/lock-binding 争议子电路）。这套"乐观储备 + 签名委员会 + 争议电路"在 v5.2 时代被接纳，**后被归档，项目转向现在更简洁的 DLC 单边赎回模型（spec 07）**。本报告等于重新评估"那条已被放弃的路是否因 free-option 死胡同而值得回头"。

**要解决的事**：脚本读不到预言机实时价、不能强制"按当前价赎回"→ 预签授权不过期 → free-option。需要 **CSFS（CHECKSIGFROMSTACK，让脚本验证预言机对栈上消息的签名）** 或契约（covenant，约束花费交易的形状）。

**关键术语（第一性原理）**：
- **bit commitment（比特承诺）**：用两个 hash 把"一个比特"钉上链，揭示哪个原像就只能得对应 0/1；**同时揭示两个 = equivocation（等价/自相矛盾）→ 泄密被罚没**。重复 256 次 = **Lamport 一次性签名**。让链下算出的值穿过多笔交易保持一致、且不能反悔的全部魔法。
- **乐观验证 / fraud proof**：默认 operator 算对，仅在被质疑时把出错的那一步搬上链跑一次来罚没。
- **委员会模拟契约**：n 人对整个交易图预签后删钥 → 这些钱只能按预签那几笔花。代价 = 信任"真删了钥"（**1-of-n 诚实**）+ 只能约束 setup 时枚举的花法。
- **混淆电路（garbled circuit）**：原为多方安全计算隐藏输入；BitVM3 用其"privacy-free"变体把欺诈证明做便宜。
- **见证加密（Witness Encryption, WE）**：把消息加密到一个 NP 陈述，**任何能提供该陈述见证的人都能解密、无需密钥**。"密钥"被替换成"对一道数学题的解答"。

**BitVM2**（已用于 Citrea/Bitlayer 桥）：
- 原理上**能**模拟 CSFS（"按最新价"是链下可判定断言，乐观欺诈证明可仲裁）。
- 代价：**n-of-n 签名委员会（=联邦/发行人）+ 一直在线挑战者/watchtower + operator 垫资**；最坏一次欺诈证明 **~14.9M sat ≈ $16,000**（2025-06 币价，已确认）；挑战窗 **数天至数周**（实现参数）；是**一个大共享桥**的设计，非逐笔小额。
- **逐条撞 SatUSD 目标**：委员会=背叛无发行人；挑战窗+盯链=背叛单边（**与否掉 DLC channel 同理**）；operator 可拒=可软冻结。**整套套用=横向换个更糟妥协。**

**BitVM3**（eprint 2026/933，peer-reviewed）：混淆电路把欺诈证明降 ~1000x（Assert ~$9、Disprove ~$0.20），但**仍桥形态、仍委员会、链下 41GB/3.4 小时 setup、仍走向生产**。语义与 SatUSD 正交。

**真正值得拿走的最小原语**（契合 ADR 的"两层 backend、仅缺操作码时乐观兜底"）：
> 用 **bit commitment + 单步欺诈证明** 给现 `redeem_tx` 加一个**"陈旧价可被罚没"的乐观闸**：持有人承诺所用签报高度；**LP 本就每块推新签报，让 LP 兼任挑战者**——持有人用陈旧价，LP 亮出更新签报罚没。无 SNARK、无委员会、无 operator 垫资。
> **诚实代价**：仍引入（可很短的）挑战窗 + LP 盯链——**与 PARK 掉 DLC channel 是同种伤、更轻**，须同杆秤量。

**前沿三项**：
- **见证加密 / PIPEs v2（eprint 2026/186）**：**语义正中靶心**（"只有提供按最新价的证明才能算出赎回签名"，链上不可区分、无软分叉、号称无委员会），**但地基可疑**——硬度假设 AADP 未经审计、作者自陈非最终、属历史反复被攻破的 iO/多线性族；成本 $100-200/次；"见证 setup 时确定"可能把你推回原问题。**未来正解，现在只跟踪/testnet，不可承重。**
- **ColliderScript（eprint 2024/1802）**：每次花费 ~$5000 万，纯理论。排除。
- **时间锁加密（drand/tlock，eprint 2023/189）**：让秘密**只在未来某时刻自动对所有人可解**。解决的是 SatUSD 的**次要**信任假设（预言机到期价不能提前泄露）；今天可用，代价=信任 drand 门限（t-of-22 合谋才出事，比"信任单一预言机"严格更弱）。**创造性用法**：预言机只在当前窗口 timelock 公布"解锁本窗口授权的秘密"，过期窗口秘密永不公布 → **旧授权自然失效**。无软分叉、无 BitVM 角色、保持单边；代价=drand 门限 + 预言机按窗口操作。

---

## 5. 候选路径与残余风险

**今天可搭的 free-option 缓解（按现实度排序）**：
1. **timelock-授权过期**（§4 末）—— 最现实的"今天能在 signet 搭"的 PoC；信任收窄到 drand 门限 + 预言机按窗。
2. **预言机 adaptor 短时效签名** —— 只用主网已有 Schnorr adaptor；代价=信任预言机 liveness（可拒签卡你）。
3. **最小乐观罚没闸**（借 BitVM 两把螺丝）—— 引入短挑战窗 + LP 盯链，须与已 PARK 的 DLC channel 同标准评判。
4. **CSFS / 经审计的 WE / 契约软分叉** —— 干净终局，但要等。

**残余风险（不甜化）**：
- **需求侧未验证**（10101 死于卖不动）——最该先回答的问号，是市场风险非技术风险。
- **工程难度高**（Lava 退托管、Yala 被盗）——但难度即护城河。
- **今天每条 free-option 解都付一点信任**（drand 门限 / 预言机 liveness / 短挑战窗）——纯硬密码学理想等 CSFS/WE。

**平台判断**：去以太坊 = 丢护城河进拥挤场，不推荐；Vitalik 的热度是给比特币叙事背书，不是叫搬家。

---

## 6. 参考资料（全部一手）

**Vitalik 提案**
- 原帖：https://ethresear.ch/t/building-index-tracking-assets-on-top-of-options-instead-of-debt/25036
- CoinDesk：https://www.coindesk.com/tech/2026/06/01/ethereum-s-vitalik-buterin-is-rethinking-how-defi-handles-market-crashes
- The Block：https://www.theblock.co/post/403311/vitalik-buterin-proposes-options-based-synthetic-assets-to-avoid-liquidations-and-reduce-reliance-on-real-time-oracles

**以太坊赛道**
- f(x) 2.0 拆解：https://mixbytes.io/blog/modern-stablecoins-how-they-re-made-f-x-protocol-2-0
- f(x) 综述：https://oakresearch.io/en/reports/protocols/fx-protocol-fxn-comprehensive-overview
- f(x) TVL：https://defillama.com/protocol/fx-protocol
- Tranchess 白皮书：https://docs.tranchess.com/whitepaper ；TVL：https://defillama.com/protocol/tranchess
- Pendle TVL：https://defillama.com/protocol/pendle
- Bankless（点名 Split/Cleave/Sir Trading/Gnosis）：https://www.bankless.com/read/a-new-kind-of-stablecoin-is-brewing-on-ethereum
- BarnBridge SMART Alpha：https://barnbridge.com/smart-alpha/ ；SEC 处罚：https://www.sec.gov/newsroom/press-releases/2023-258

**比特币竞品**
- Ducat 文档（清算 135%）：https://docs.ducatprotocol.com/liquidations/basic-mechanics ；philosophy：https://docs.ducatprotocol.com/unit/philosophy
- 10101 关停 + 合成稳定币原理：https://10101.finance/blog/10101-is-shutting-down/ ；https://10101.finance/blog/synthetic-stable/
- conduition DLC-factory（机制最像）：https://conduition.io/scriptless/dlc-factory/
- USDB（Decrypt）：https://decrypt.co/326741/bitcoin-gets-native-dollar-backed-stablecoin-usdb
- bitSmiley（MT Capital）：https://medium.com/@MTCapital_US/mt-capital-bitsmiley-pioneer-of-native-stablecoin-protocols-for-bitcoin-b287dba97d93
- Satoshi Protocol（Decrypt）：https://decrypt.co/225670/satoshi-protocol-first-cdp-on-bitcoin-layer2-500k-oshi-airdrop-with-binance-wallet-and-bevm
- Yala 脱锚：https://bravenewcoin.com/insights/bitcoin-backed-stablecoin-yu-crashes-80-after-7-7-million-protocol-attack
- Avalon USDa：https://www.gate.com/learn/articles/what-is-usda-and-avalon-labs/7111
- Lava 退回托管：https://bitcoinmagazine.com/business/lava-abandons-self-custody-amidst-fund-raise-sparking-controversy ；V2 DLC 原理：https://bitcoinmagazine.com/technical/lava-loans-protocol-v2-dlc-based-bitcoin-collateralized-loans
- Hermetica USDh：https://docs.hermetica.fi/resources/faqs/usdh-and-susdh
- Stablesats：https://stablesats.com/
- Citrea ctUSD（2026/1 主网）：https://crypto.news/bitcoin-zk-rollup-citrea-launches-mainnet-2026/
- BOB 清算引擎：https://www.coindesk.com/tech/2025/10/28/bob-unveils-bitcoin-vault-liquidation-engine-to-power-btc-backed-stablecoin-lending
- Tether USD₮ on RGB：https://tether.io/news/tether-to-launch-usdt-on-rgb-expanding-native-bitcoin-stablecoin-support/
- Liquidium（DLC 自托管借贷）：https://liquidium.wtf/

**BitVM / 前沿技术**
- BitVM2（IACR 2025/1158）：https://eprint.iacr.org/2025/1158 ；站点：https://bitvm.org/bitvm2.html
- Alpen《State of SNARK verification with BitVM2》：https://www.alpenlabs.io/blog/state-of-snark-verification-with-bitvm2
- bit commitment / Lamport：https://bitcoinmagazine.com/technical/script-state-from-lamport-signatures ；https://www.rootstocklabs.com/blog/exploring-lamport-and-winternitz-signatures-for-stateful-bitcoin-scripts/
- Citrea/Clementine 桥白皮书：https://citrea.xyz/clementine_whitepaper.pdf
- Bitlayer《BitVM Bridge Becomes Practical》：https://blog.bitlayer.org/BitVM_Bridge_Becomes_Practical/
- BitVM3（IACR 2026/933）：https://eprint.iacr.org/2026/933 ；白皮书：https://bitvm.org/bitvm3.pdf ；Blockworks（$16k vs $5/$0.20）：https://blockworks.co/news/bitvm3-promises-cheaper-bitcoin-bridges
- Bitcoin PIPEs v2 / 见证加密（IACR 2026/186）：https://eprint.iacr.org/2026/186 ；Delving 讨论：https://delvingbitcoin.org/t/bitcoin-pipes-v2/2249
- ColliderScript（IACR 2024/1802）：https://eprint.iacr.org/2024/1802 ；解读：https://bitcoinmagazine.com/technical/colliderscript-a-50m-bitcoin-covenant-with-no-new-opcodes
- BitVMX：https://arxiv.org/pdf/2405.06842 ；ESSPI（签名即输入）：https://arxiv.org/pdf/2503.02772
- 时间锁加密：https://docs.drand.love/docs/timelock-encryption/ ；tlock 论文：https://eprint.iacr.org/2023/189.pdf ；库：https://github.com/drand/tlock
- CSFS（BIP-348）：https://github.com/bitcoin/bips/blob/master/bip-0348.md

---

## 7. 方法论与可信度

本报告由 5 个并行 AI 研究 agent 各读一手资料后综合：①Vitalik 提案精读 ②以太坊赛道 ③比特币竞品深扫 ④BitVM2 第一性原理 ⑤BitVM3+前沿技术。各 agent 均标注【确认/推断/未证实】。**需独立复核的承重项**：conduition 与 SatUSD 的精确重合度；Ducat 当前确切模型与状态；各早期项目 TVL；Vitalik 原帖第 2 页讨论（一个 agent 未能逐字抓取）。BitVM 成本/窗口数字为 2026 年中快照，应在实际决策前 re-check。
