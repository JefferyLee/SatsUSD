# SatUSD: A Bitcoin-Native Bridge to a Bitcoinizing Future
# SatUSD：通向比特币化未来的原生桥梁

## Why we exist / 为什么存在

Bitcoin is the most reliable money ever designed:
fixed supply, no issuer, permissionless, censorship-resistant,
custody-sovereign. As money, its only failure today is
**purchasing-power volatility** — its day-to-day USD price moves
enough that it cannot yet function as a unit of account in
ordinary commerce.

比特币是人类设计过的最可靠的货币：总量固定、无发行方、无许可、抗
审查、可自主托管。作为货币，它今天唯一的缺陷是**购买力波动率**——
以美元计的日内价格波动足以让它无法在日常商业中充当计价单位。

The US dollar — and the fiat system it represents — has the
opposite profile. Its short-term purchasing power is stable
enough to function as a unit of account. But it is issued by
political authority, inflated at the issuer's discretion, and
increasingly programmable into a surveillance and control
apparatus. Account freezes, transaction monitoring, sanctioned
addresses, programmable CBDC restrictions — these are not
hypothetical futures. They are the lived experience of millions
of people in 2026.

美元——以及它所代表的法币体系——则呈现相反的画像。其短期购买力足
够稳定，可以充当计价单位。但它由政治权力发行、由发行方自由决定通
胀、并日益被编程成监控与控制装置。账户冻结、交易监控、制裁地址、
可编程 CBDC 限制——这些不是假设的未来，而是 2026 年数百万人的真实
经历。

Both currencies fail. Bitcoin fails as a present-day unit of
account. The dollar fails as a long-term store of freedom. The
question this project answers is: **can a single instrument
inherit the strengths of both, while shedding the weaknesses
of either?**

两种货币都失败。比特币作为今日的计价单位失败，美元作为长期自由储
存失败。本项目要回答的问题是：**一种工具能否同时继承两者的长处，
而抹除任何一方的弱点？**

## What SatUSD is / SatUSD 是什么

SatUSD is a Bitcoin-collateralized, dollar-denominated
instrument issued natively on Bitcoin L1 via Taproot Assets.
A holder of N SatUSD always has a claim, enforced by the
protocol, on $N worth of bitcoin in the reserve, computed at
the current exchange rate.

SatUSD 是一种以比特币抵押、美元计价、通过 Taproot Assets 在比特币
L1 原生发行的工具。任何 N 单位 SatUSD 的持有者，**始终持有一项受协
议强制保护的请求权**——按当前汇率提取 $N 等值比特币。

This is not yet a new unit of account. It is **a bridge** —
not just a stablecoin, but the rail by which the world's
day-to-day commerce can begin migrating from dollar-denominated
to bitcoin-denominated without forcing users to absorb bitcoin's
present-day volatility. It speaks USD on the outside, and
bitcoin on the inside. Every SatUSD that circulates is backed
by bitcoin that never leaves Bitcoin L1; every redemption
settles in bitcoin. The user interacts with familiar
dollar-denominated quantities. The protocol, the reserve, and
the settlement are entirely bitcoin-native.

它现在还不是新的计价单位。它是**一座桥**——不只是一个稳定币，而是
让全世界的日常商业可以从「美元计价」开始迁移到「比特币计价」的轨
道，并且**不强迫用户在迁移过程中承担比特币当下的波动率**。对外它说
美元的话，对内它是比特币。每一枚流通的 SatUSD 都由从未离开 Bitcoin
L1 的比特币担保；每一次赎回都在比特币上结算。用户面对的是熟悉的美
元面值。协议、储备、结算全部都是比特币原生。

This is intentional scaffolding. **The dollar peg is the path,
not the destination.**

这是有意为之的脚手架。**美元锚定是路径，不是目标**。

## Why existing stablecoins fall short / 为什么现有稳定币都不够

Every stablecoin in 2026 makes a compromise that SatUSD refuses
to make:

2026 年所有现行稳定币都做了一个 SatUSD 拒绝做的妥协：

| Stablecoin | What it is / 它是什么 | The compromise / 妥协 |
|---|---|---|
| **USDT** (Tether) | Fiat-backed by Treasury bills + cash | Tether Ltd is a private company that can freeze any address; reserves are auditable only by their chosen auditor; the asset exists at the discretion of US Treasury / OFAC / a single CEO. **You trust a company, not a protocol.** |
| **USDC** (Circle) | Same model, US-regulated | Same trust pattern as USDT but with explicit US regulatory leash. Circle has frozen addresses for OFAC sanctions; this is a feature for institutional users, **a kill-switch for everyone else**. |
| **DAI** (MakerDAO) | Originally ETH-collateralized; now ~60% backed by USDC and Treasury bonds | Started as "decentralized stablecoin" but pragmatic governance imported the same fiat trust assumptions through the back door. **The collateral floor is USDC, so DAI inherits all of USDC's freeze and seizure risks**. |
| **FRAX** | Hybrid algorithmic + collateral | Complex peg-defense mechanism with multiple oracle dependencies; algorithmic component requires confidence loop that can break under stress. |
| **LUSD** (Liquity V1) | Pure ETH-collateralized | Truly decentralized peg mechanism, but lives on Ethereum and depends on ETH economics + Ethereum L1 oracle. **Bitcoiners must convert to ETH first.** |
| **Terra UST** | Pure algorithmic | Collapsed catastrophically in May 2022 (~$40B vaporized). **Algorithmic stability without external collateral is a proven failure mode.** |
| **ctUSD** (Citrea, 2026) | Fiat-backed via M0 + MoonPay | Lives on Citrea rollup. **Not BTC-collateralized**; it's USD-on-Bitcoin-rollup, not Bitcoin-as-money. Same trust pattern as USDC. |
| **BTD** (Alpen, testnet) | BTC-collateralized via Liquity V2 fork | Closest in spirit to SatUSD, but **lives on Alpen rollup, not Bitcoin L1**. Adds rollup security assumption. |

每一个都在以下至少一项上让步：
- **中心化发行方**（USDT, USDC, ctUSD）→ 可被政府胁迫，可冻结
- **fiat 储备**（USDT, USDC, DAI 间接, ctUSD）→ 全部 trust 仍在 fiat 系统里
- **错链**（LUSD, FRAX, BTD）→ 价值不锚定 Bitcoin L1
- **算法稳定**（UST, FRAX 部分）→ 已被市场证明不可靠

**SatUSD makes none of these compromises.** Reserve is bitcoin
only. Settlement is on Bitcoin L1. Issuance and redemption are
permissionless. The protocol has no kill switch — not for us,
not for any government, not for any committee. This combination
does not exist anywhere else in the market.

**SatUSD 不做以上任何妥协**。储备只用比特币。结算在 Bitcoin L1 上完
成。发行与赎回无许可。协议没有 kill switch——我们没有、任何政府没
有、任何委员会没有。**这个组合在今天的市场上不存在**。

## What SatUSD is not / SatUSD 不是什么

**SatUSD does not pay interest on holdings.** Paying yield on a
stablecoin requires deploying the reserve into yield-bearing
positions, which either compromises the redeem-anytime
guarantee, or routes the yield through fiat instruments
(Treasury bills) that re-import the very dependencies we are
trying to escape. We refuse this trade.

**SatUSD 不对持仓本身支付利息**。给稳定币付收益要求把储备投入生息
头寸——这要么破坏「随时可赎」的保证，要么通过法币工具（如美债）兜
一圈，把我们本来要逃离的依赖重新引回来。我们拒绝这笔交易。

This does not mean participation in the SatUSD economy is
unprofitable:

但这**不意味着参与 SatUSD 经济没有回报**：

- **Liquidity providers earn fees.** Providing bitcoin to the
  BTC/SatUSD redemption pools earns a portion of every redemption
  spread — a real return paid in real bitcoin, every block.
- **流动性提供者赚取手续费**：向 BTC/SatUSD 赎回池提供比特币流动性，
  每一次赎回的价差中的一部分归 LP——一笔以真实比特币支付的真实收益，
  每个区块都在结算。

- **Bitcoin appreciates as the economy bitcoinizes.** The deepest
  return comes not from any yield instrument but from holding
  bitcoin during the transition itself. If SatUSD succeeds at
  what it sets out to do, bitcoin's purchasing power will
  compound dramatically over the horizon of this project — far
  outstripping any Treasury yield available on a USDC-style
  product. **The reward for being early to a bitcoinizing world
  is bitcoin itself.**
- **随经济比特币化的过程，BTC 自身升值**。最深的回报不来自任何生息
  工具，而来自在过渡过程中本身持有比特币。如果 SatUSD 实现它要做的
  事，比特币的购买力将在本项目时间尺度内**急剧复利增长**——远远超
  过任何 USDC 类产品提供的国债收益。**比特币化世界对早期参与者的奖
  励，就是比特币本身**。

SatUSD is the rail for that transition. The token is a unit of
account. The reward is the asset behind it.

SatUSD 是那场过渡的轨道。代币是计价单位。回报是它身后的那个资产。

---

**SatUSD is not Treasury-backed.** We do not custody dollars,
US Treasury bills, or any fiat instrument. The reserve is
bitcoin only.

**SatUSD 不由美债担保**。我们不托管美元、美国国债或任何法币工具。储
备只有比特币。

**SatUSD is not a regulated stablecoin.** We do not implement
KYC/AML on issuance or redemption. The asset is permissionless,
just like the bitcoin behind it.

**SatUSD 不是受监管的稳定币**。我们不在发行与赎回流程上实施 KYC/AML。
这种资产是无许可的，就像它背后的比特币一样。

**SatUSD is not optimized for institutions.** Institutional
adoption requires regulatory wrappers, fiat on-ramps, and
audited custodians that contradict the asset's core properties.
We optimize for the individual user who has been denied
service by traditional banking, lives under capital controls,
or values monetary sovereignty as an end in itself.

**SatUSD 不为机构优化**。机构采用要求监管封装、法币入金、合规审计的
托管方——这些都与本资产的核心属性冲突。我们为这些个人用户优化：被
传统银行拒绝服务的人、生活在资本管制下的人、把货币主权本身视为目
的的人。

## The transition / 过渡路径

The vision is not stable forever. The vision is that SatUSD
serves as a transitional rail by which world commerce gradually
migrates from dollar-denominated to bitcoin-denominated,
without requiring users to absorb bitcoin volatility during
the transition.

愿景不是永远稳定。愿景是 SatUSD 充当一条过渡轨道，世界商业经由它
逐渐从美元计价迁移至比特币计价，而无需用户在迁移过程中承担比特币
波动率。

**Phase 0**: We exist. Small TVL, external oracle pinned to
CEX prices. Most users still think in dollars.

**阶段 0**：我们存在。TVL 较小，依赖 CEX 价格的外部 oracle。多数用户
仍然以美元思考。

**Phase 1**: Real volume. Internal market data begins to form.
The protocol still depends on external oracles but starts
cross-checking with internal trades.

**阶段 1**：真实交易量出现。内部市场数据开始形成。协议仍依赖外部
oracle，但开始与内部交易做交叉校验。

**Phase 2**: Internal market is canonical. SatUSD's own trade
history is the most authoritative BTC/USD price on Bitcoin L1.
External oracles become a sanity check, not a dependency.

**阶段 2**：内部市场成为权威。SatUSD 自己的交易历史成为比特币 L1 上
最权威的 BTC/USD 价格。外部 oracle 降级为完整性检查，不再是依赖。

**Phase 3**: BTC velocity reduces. As more commerce settles in
SatUSD-denominated channels backed by Bitcoin reserves, BTC's
USD-volatility shrinks. The peg becomes less and less
load-bearing because the user already lives mostly in the
SatUSD economy.

**阶段 3**：BTC 波动率下降。随着越来越多商业活动在 SatUSD 渠道（由
比特币储备担保）里结算，比特币的美元波动率收窄。锚定变得越来越不
承重，因为用户已经主要生活在 SatUSD 经济里。

**Phase 4**: The peg is vestigial. BTC has become a sufficient
unit of account directly. SatUSD continues to exist as a
fixed-purchasing-power instrument, but its "USD" suffix is now
historical — a reminder of the legacy unit it once bridged
from. New denominations native to bitcoin may emerge alongside.

**阶段 4**：锚定退化为历史痕迹。比特币本身已足够充当计价单位。
SatUSD 继续作为「**固定购买力**」工具存在，但其「USD」后缀已是历史
——一个提醒，告诉世人它曾经从哪个 legacy 单位过渡而来。新的比特币
原生计价单位可能并行出现。

We estimate Phase 0→1 takes 1–3 years; Phase 1→2 takes 3–7
years; Phase 2→3 takes a decade; Phase 4 is generational. We
are explicitly building for a longer horizon than most crypto
projects. We assume we will be wrong about the specifics; we
are committed to being correct about the direction.

我们估计阶段 0→1 需要 1–3 年；阶段 1→2 需要 3–7 年；阶段 2→3 需要十
年；阶段 4 是世代级。我们显式建造的是比绝大多数加密项目更长的时间
尺度。我们预期会在细节上犯错，但承诺方向上对。

## Why self-referencing is necessary / 为什么必须 self-referencing

A stablecoin that permanently depends on an external price
oracle has not escaped the system it claims to escape. If
SatUSD's reserve calculus, redemption rate, and liquidation
trigger forever depend on what Coinbase or Binance reports,
then the legacy financial system retains a veto over SatUSD's
operation. That veto is a single point of attack — political,
regulatory, technical — for adversaries who would prefer the
project not exist.

任何永久依赖外部价格 oracle 的稳定币，都没能逃离它声称要逃离的体
系。如果 SatUSD 的储备核算、赎回汇率、清算触发永远依赖于 Coinbase
或 Binance 报告的价格，那么传统金融体系就保留了对 SatUSD 运行的否
决权。这种否决权是单一攻击点——政治的、监管的、技术的——任何不希
望本项目存在的对手都可以利用它。

Self-referencing — deriving the canonical SatUSD price from
SatUSD's own on-chain economic activity, secured only by
Bitcoin's consensus — is therefore not a technical
optimization. **It is the only end state consistent with the
project's mission.** We cannot succeed at being a freedom
instrument while remaining cryptographically dependent on the
institutions we are providing escape from.

self-referencing——让 SatUSD 的权威价格从其自身的链上经济活动中派
生，仅由比特币共识担保——因此**不是技术优化**。**它是与本项目使命
兼容的唯一终态**。我们不可能在密码学上依赖那些「我们正在为他们的
潜在受害者提供逃生通道」的机构，同时还成功扮演自由工具。

This is a long path. We will spend years dependent on external
oracles. But every architectural choice should be evaluated by
this criterion: **does it move us closer to or further from a
state where the external dependency could be removed?**

这是一条长路。我们将花费多年时间依赖外部 oracle。但每一个架构决策
都应该用一个标准来评估：**它让我们更接近还是更远离「外部依赖可以
被拿掉」的那个状态？**

## A standing invitation / 一封持续有效的邀请

This is not a wager. This is a bet on a future that is already
arriving.

这不是一场赌局。这是对一个**已经在到来的未来**的下注。

Bitcoin's hash rate compounds. Its supply schedule is set in
stone for the next century. The dollar's purchasing power has
halved in the last fifteen years and will halve again. Central
bank digital currencies are being prototyped in two dozen
jurisdictions. Account freezes for political reasons are
becoming routine in liberal democracies, not just authoritarian
ones. The world is sorting itself into people who can opt out
of monetary politics and people who cannot.

比特币的算力在复利增长，其供给计划在未来一个世纪都已刻在石头上。
美元的购买力在过去十五年里减半，未来还会再次减半。央行数字货币正
在二十多个司法辖区被原型化。出于政治原因的账户冻结，在自由民主国
家正变得和威权国家一样常规。世界正在分化为「可以选择退出货币政治
的人」和「不能的人」。

SatUSD exists to make the first category larger.

SatUSD 存在的目的，是让前一类人变多。

We do not seek venture capital. We do not seek regulatory
approval. We do not seek institutional endorsement. We seek
the attention of:

我们不寻求 venture capital。我们不寻求监管批准。我们不寻求机构背
书。我们寻求以下人群的关注：

- **Bitcoin developers** who recognize that the next decade of
  this technology is not just about price-go-up but about
  building the financial infrastructure that allows people to
  live in it.
- **比特币开发者**，那些理解这门技术下一个十年的关键不是币价上涨、
  而是构建能让人真正生活在其中的金融基础设施的人。

- **Cryptographers** willing to engage with the unsolved
  problems of Bitcoin-L1 oracle design, threshold signing, and
  trust-minimized bridges.
- **密码学家**，愿意参与解决比特币 L1 oracle 设计、阈值签名、可信最
  小化桥这些尚未解决问题的人。

- **Users** who have personally felt the cost of fiat coercion
  and are willing to use early, imperfect alternatives so the
  next generation has better ones.
- **用户**，亲身感受过法币胁迫成本、愿意使用早期不完美替代方案的人
  ——你的使用让下一代有更好的选择。

- **Critics** who will tell us bluntly where the design is
  wrong. Hostile criticism well-articulated is more valuable
  to this project than friendly endorsement.
- **批评者**，会直接告诉我们设计哪里错了的人。**良好表达的敌意批
  评比友好背书对本项目更有价值**。

This project is an attempt. The attempt may fail. If it does,
we will have documented the failure in the open, and the
next generation will build on what we tried. If it succeeds,
the world will be measurably freer in our lifetimes — not
through revolution, not through politics, but through the
quiet substitution of better money for worse.

本项目是一次尝试。这次尝试可能失败。**如果失败，我们将在公开场合
记录失败，下一代会在我们尝试过的基础上继续**。如果成功，这个世界在
我们有生之年将变得**可以测量地更自由**——不是通过革命、不是通过政
治，而是**通过更好的货币悄无声息地替代糟糕的货币**。

Either way, what we build will be open, what we learn will be
shared, and what we believe will be written down. This
document is the first installment.

无论结果如何，**我们构建的东西将开放、我们学到的将共享、我们相信的
将被白纸黑字写下**。本文是第一份。

---

*This document defines the project's intent. Technical
documentation defines its implementation. Where the two appear
to conflict, this document is the higher authority — any
implementation choice that contradicts the mission must either
be revised or justified as a deliberate, temporary compromise
toward the mission.*

*本文定义项目意图。技术文档定义实现方式。当两者出现冲突时，本文具
更高权威——任何与使命矛盾的实现选择，必须被修订，或被显式标记为
朝向使命的、有意的、暂时性妥协。*

*Authorship and version control of this document follow the
SatUSD repository's standard contribution process. The vision
articulated here is intended to outlive any individual
contributor, including its original drafters.*

*本文的署名与版本控制遵循 SatUSD 仓库的标准贡献流程。这里阐述的愿景
意在超越任何单一贡献者——包括它最初的起草者——而长期存在。*
