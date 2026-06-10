# SatUSD: A Bitcoin-Native Bridge to a Bitcoinizing Future
# SatUSD：通向比特币化未来的原生桥梁

*Version 2 — the founding document, rewritten from the seed vision.
All other documents in this repository derive from, and are
subordinate to, this one.*

*第 2 版——自种子愿景重写的奠基文档。本仓库中所有其他文档均派生于
本文，并从属于本文。*

## Two monies, two failures / 两种货币，两种失败

Bitcoin is the most reliable money ever designed: fixed supply,
no discretionary issuer, permissionless, censorship-resistant,
custody-sovereign. As money, its principal failure today is
**purchasing-power volatility**: its day-to-day price moves too
much for anyone to quote a salary, a contract, or a cup of
coffee in it. It cannot yet serve as a unit of account — and so
it cannot yet do the one thing a free market needs most from
its money.

比特币是人类设计过的最可靠的货币：总量固定、无任意增发的发行方、
无许可、抗审查、可自主托管。作为货币，它今天最主要的失败是**购买
力波动率**：日常价格波动太大，没有人能用它报工资、签合同、标一杯
咖啡的价。它还不能充当计价单位——因而还不能履行自由市场对货币最
核心的那一项要求。

The US dollar — and the fiat system it represents — has the
opposite profile. Its short-term purchasing power is stable
enough to quote prices in. But it is issued by political
authority, inflated at the issuer's discretion, and increasingly
programmable into a surveillance and control apparatus. Account
freezes, transaction monitoring, sanctioned addresses,
programmable CBDC restrictions — these are not hypothetical
futures. They are the lived experience of millions of people
in 2026.

美元——以及它所代表的法币体系——呈现相反的画像。其短期购买力足够
稳定，可以用来标价。但它由政治权力发行、由发行方自由决定通胀、并
日益被编程成监控与控制装置。账户冻结、交易监控、制裁地址、可编程
CBDC 限制——这些不是假设的未来，而是 2026 年数百万人的真实经历。

Both monies fail. Bitcoin fails as a present-day unit of
account. The dollar fails as a long-term store of freedom. The
question this project answers is: **can a single instrument
inherit the strengths of both, while shedding the weaknesses of
either?**

两种货币都失败。比特币作为今日的计价单位失败，美元作为长期自由储
存失败。本项目要回答的问题是：**一种工具能否同时继承两者的长处，
而抹除任何一方的弱点？**

## Money is an information system / 货币是一套信息系统

Money's deepest function is neither storage nor payment. It is
**information**: prices denominated in a common unit are the
distributed signals by which billions of strangers coordinate
production, consumption, and exchange — without any of them
needing to know the whole. This is the price-signal function,
and it is the hardest function of money to migrate, because it
lives not in any ledger but in the habits of every mind that
quotes a price.

货币最深层的功能既不是储值也不是支付，而是**信息**：以共同单位标
出的价格，是亿万陌生人协调生产、消费与交换的分布式信号——无需任
何人通晓全局。这就是价格信号功能。它也是货币诸功能中最难迁移的一
项，因为它不存在于任何账本里，而存在于每一个用它报价的头脑的习惯
里。

Be honest about the present: fiat performs this function today.
That is the real reason it survives. Bitcoin does not — nothing
is quoted in it, so it generates no signals. But fiat's signal
source is owned. It is inflated at will, surveilled at scale,
and — with programmable central-bank money — being rebuilt into
an instrument of control. A free market running on captured
signals is free in name only.

对现状要诚实：今天执行这个功能的是法币——这是它仍然活着的真正原
因。比特币没有在执行——没有东西以它标价，它就不产生信号。但法币
的信号源是**有主人的**：随意通胀、规模化监控，并且——随着可编程
央行货币的到来——正在被改造成控制工具。运行在被劫持信号上的自由
市场，只在名义上自由。

**The mission of this project, stated precisely, is to migrate
the price-signal function of money from fiat to bitcoin.** Not
to migrate balances — that has been possible for fifteen years —
but to migrate the function: the unit people quote in, the
signals markets coordinate by.

**精确地陈述本项目的使命：把货币的价格信号功能从法币迁移到比特币
之上。**不是迁移余额——那十五年前就可以做到——而是迁移功能本身：
人们用以报价的单位，市场赖以协调的信号。

## What SatUSD is / SatUSD 是什么

SatUSD is a bitcoin-collateralized, dollar-denominated
instrument issued natively on Bitcoin L1 via Taproot Assets. A
holder of N SatUSD holds a claim on $N worth of bitcoin in the
reserve — a claim the protocol is built to enforce without
trusted intermediaries, and one that is as strong as the
reserve's over-collateralization, which the mechanism is
designed to maintain.

SatUSD 是一种以比特币抵押、美元计价、通过 Taproot Assets 在比特币
L1 原生发行的工具。持有 N 单位 SatUSD，即持有对储备中 $N 等值比特
币的请求权——协议的全部构造都为了让这项请求权**无需可信中介即可
执行**；其强度等于储备的超额抵押程度，而机制设计的目标正是维持这
种超额。

It is **a bridge** — not just a stablecoin. The dollar's real
fortress is not the central bank; it is the habit of billions of
minds quoting prices in dollars. That network effect cannot be
taken by frontal assault. SatUSD does not assault it. It hollows
it out: the user keeps the dollar habit — familiar denominations,
stable quotes — while the substance beneath becomes bitcoin.
Reserve: bitcoin, never leaving Bitcoin L1. Settlement: bitcoin.
Custody: the holder's own keys. **Substance migrates first. The
habit migrates last — and by then it is migrating across a
bridge that already exists.**

它是**一座桥**——不只是一个稳定币。美元真正的堡垒不是中央银行，
而是数十亿头脑用美元报价的习惯。这种网络效应无法被正面攻克。
SatUSD 不去攻打它，而是把它掏空：用户保留美元习惯——熟悉的面值、
稳定的报价——而其下的实质换成比特币。储备：比特币，从不离开
Bitcoin L1。结算：比特币。托管：持有者自己的密钥。**实质最先迁
移，习惯最后迁移——而到那时，习惯是在一座已经存在的桥上完成迁移
的。**

This is intentional scaffolding. **The dollar peg is the path,
not the destination.**

这是有意为之的脚手架。**美元锚定是路径，不是目标。**

## What SatUSD is not / SatUSD 不是什么

**SatUSD does not pay interest on holdings.** Paying yield on a
stablecoin requires deploying the reserve into yield-bearing
positions, which either compromises the redeem-anytime guarantee
or routes the yield through fiat instruments that re-import the
dependencies we are escaping. We refuse this trade.

**SatUSD 不对持仓本身支付利息。**给稳定币付收益要求把储备投入生息
头寸——要么破坏「随时可赎」的保证，要么借道法币工具把我们正要逃
离的依赖重新引进来。我们拒绝这笔交易。

This does not mean participation is unprofitable:

但这不意味着参与没有回报：

- **Liquidity providers earn fees.** Providing bitcoin to the
  BTC/SatUSD redemption rails earns a share of every redemption
  spread — a real return, paid in real bitcoin, at every
  settlement.
- **流动性提供者赚取手续费。**向 BTC/SatUSD 赎回通道提供比特币流动
  性，每一次赎回的价差都有一份归 LP——以真实比特币支付的真实回报，
  随每笔结算到账。

- **Bitcoin appreciates as the economy bitcoinizes.** The deepest
  return comes not from any yield instrument but from holding
  bitcoin through the transition itself. If SatUSD succeeds,
  bitcoin's purchasing power compounds over the horizon of this
  project. **The reward for being early to a bitcoinizing world
  is bitcoin itself.**
- **随经济比特币化，比特币自身升值。**最深的回报不来自任何生息工
  具，而来自在过渡过程中持有比特币本身。如果 SatUSD 成功，比特币
  的购买力将在本项目的时间尺度上复利增长。**比特币化世界给早期参
  与者的奖励，就是比特币本身。**

**SatUSD holds no fiat reserve.** No dollars, no Treasury bills,
no fiat instrument of any kind. The reserve is bitcoin only.

**SatUSD 不持有任何法币储备。**没有美元、没有美债、没有任何形式的
法币工具。储备只有比特币。

**SatUSD has no permission layer.** No KYC, no AML gate, no
freeze function, no admin key on the issuance and redemption
paths. The asset is as permissionless as the bitcoin behind it.
Where today's implementation still contains transitional
controls, they are scaffolding — enumerated, justified, and
scheduled for demolition in the technical documents.

**SatUSD 没有许可层。**发行与赎回路径上没有 KYC、没有 AML 闸门、没
有冻结功能、没有管理员密钥。这种资产和它背后的比特币一样无许可。
凡今日实现中仍存的过渡性控制，皆为脚手架——在技术文档中逐项列
明、说明理由、并排定拆除。

**SatUSD is not optimized for institutions.** Institutional
adoption demands regulatory wrappers and audited custodians that
contradict the asset's core properties. We optimize for the
individual: the person denied banking service, the person under
capital controls, the person who treats monetary sovereignty as
an end in itself.

**SatUSD 不为机构优化。**机构采用要求监管封装与合规托管，与本资产
的核心属性冲突。我们为个人优化：被银行拒之门外的人、身处资本管制
之下的人、把货币主权本身当作目的的人。

## Why existing stablecoins fall short / 为什么现有稳定币都不够

Every stablecoin on the market makes at least one of four
compromises that SatUSD refuses:

市面上每一种稳定币，都至少做了 SatUSD 拒绝的四种妥协之一：

**1. A centralized issuer** — USDT (Tether), USDC (Circle),
ctUSD (M0/MoonPay on Citrea). A company that can freeze any
address is a kill switch wearing a brand. You trust a firm, not
a protocol.

**1. 中心化发行方**——USDT（Tether）、USDC（Circle）、ctUSD
（Citrea 上的 M0/MoonPay）。一家能冻结任意地址的公司，就是一个挂着
品牌的 kill switch。你信任的是公司，不是协议。

**2. A fiat reserve** — USDT, USDC, ctUSD; DAI/USDS (Sky,
formerly MakerDAO) indirectly through its USDC and Treasury
backing; even FRAX, the flagship of algorithmic design, pivoted
to full Treasury collateralization in 2025. The trust never
left the fiat system — it was laundered through a token.

**2. 法币储备**——USDT、USDC、ctUSD；DAI/USDS（Sky，前 MakerDAO）
通过其 USDC 与美债背书间接如此；连算法设计的旗手 FRAX 也在 2025 年
转向了全额美债抵押。信任从未离开法币体系——只是经由代币洗了一道。

**3. The wrong chain** — LUSD/BOLD (Liquity) live on Ethereum;
BTD (Alpen) lives on a rollup. Value that does not settle on
Bitcoin L1 inherits someone else's security assumptions.

**3. 错误的链**——LUSD/BOLD（Liquity）在以太坊上；BTD（Alpen）在
rollup 上。不在 Bitcoin L1 上结算的价值，继承的是别人的安全假设。

**4. Algorithmic stability** — Terra's UST erased roughly $40B
in May 2022. Stability conjured from a system's own token is a
proven failure mode, not a design choice.

**4. 算法稳定**——Terra 的 UST 在 2022 年 5 月蒸发约 $400 亿。从系
统自身代币中变出来的稳定，是已被证明的失败模式，不是设计选项。

**SatUSD makes none of these compromises.** Bitcoin-only
reserve. Bitcoin L1 settlement. Permissionless issuance and
redemption. No kill switch by design — not for us, not for any
government, not for any committee. This combination exists
nowhere else.

**SatUSD 不做其中任何一种妥协。**纯比特币储备。Bitcoin L1 结算。无
许可的发行与赎回。设计上没有 kill switch——我们没有、任何政府没
有、任何委员会没有。这个组合在市场上不存在第二份。

## The transition / 过渡路径

First, the thesis that makes the path coherent: **volatility is
a property of denomination, not of bitcoin.** Today bitcoin's
price is set in dollar-denominated, speculation-dominated
markets — so "bitcoin is volatile." In a world where goods and
labor are quoted in sats, the measuring stick has switched
sides, and what fluctuates is the dollar. The phases below are
the path between those two worlds. Their boundaries are
recognized in hindsight by metrics — volume, internal-external
price coherence, oracle market share — not declared by anyone.

先陈述让整条路径自洽的论题：**波动率是计价单位的属性，不是比特币
的属性。**今天比特币的价格在以美元计价、由投机主导的市场里形成，
所以「比特币波动大」。而在货物与劳动以 sat 标价的世界里，尺子换到
了另一边，波动的是美元。以下阶段就是这两个世界之间的路。阶段边界
由指标在事后辨认——交易量、内外价格一致性、oracle 市场份额——而
不由任何人宣布。

**Phase 0 — We exist.** Small volume, prices pinned to external
sources. Most users still think in dollars.

**阶段 0——我们存在。**交易量小，价格锚定外部来源。多数用户仍以美
元思考。

**Phase 1 — Real volume.** The internal market begins generating
its own data. External oracles remain the reference; internal
trades begin to cross-check them.

**阶段 1——真实交易量。**内部市场开始产生自己的数据。外部 oracle
仍是基准；内部交易开始与之交叉校验。

**Phase 2 — The internal market becomes canonical.** SatUSD's
own trade history is the most authoritative BTC/USD price on
Bitcoin L1. For the first time, the bitcoin economy generates
its own price signal. External sources demote to sanity checks.

**阶段 2——内部市场成为权威。**SatUSD 自己的交易历史成为 Bitcoin
L1 上最权威的 BTC/USD 价格。比特币经济第一次生成了自己的价格信
号。外部来源降级为完整性检查。

**Phase 3 — Denomination begins to flip.** Commerce settles in
SatUSD channels backed by bitcoin reserves; transactional demand
for bitcoin grows continuous and two-sided; measured volatility
shrinks because the speculative share of flow shrinks. Prices
start appearing in sats alongside dollars.

**阶段 3——计价开始翻转。**商业在比特币储备担保的 SatUSD 渠道中结
算；比特币的交易性需求变得连续且双向；测得的波动率收窄，因为投机
流量的占比在缩小。价格开始以 sat 与美元并列出现。

**Phase 4 — The bridge retires.** When bitcoin is a sufficient
unit of account, SatUSD's work is done. Holders redeem into the
sat-denominated world; issuance wanes; the instrument winds down
by attrition, the same way it grew — no decree required. **A
bridge succeeds when traffic no longer needs it.** We state this
in the founding document so that no one — including us — can
later pretend this project was meant to live forever and collect
rent.

**阶段 4——桥退役。**当比特币足以充当计价单位，SatUSD 的工作就完
成了。持有者赎回、进入以 sat 计价的世界；发行量衰减；这个工具以它
生长的同样方式——自然消长、无需法令——逐渐谢幕。**一座桥的成功，
是车流不再需要它。**我们把这一条写进奠基文档，以使任何人——包括
我们自己——都无法在日后假装这个项目本该永远活着、永远收租。

We estimate Phase 0→1 in years, Phase 1→2 in more years, Phase
3 in a decade, Phase 4 generational. The engineering can be
fast; adoption runs on the world's clock, not ours. We assume we
will be wrong about specifics, and we are committed to being
right about direction.

我们估计阶段 0→1 以年计，1→2 以更多年计，阶段 3 以十年计，阶段 4
是世代级。工程可以很快；而采用走的是世界的钟，不是我们的钟。我们
预期在细节上犯错，并承诺在方向上正确。

## Self-referencing: why it is necessary — and why this is not Terra
## Self-referencing：为何必需——以及为何这不是 Terra

A stablecoin that permanently depends on an external price
oracle has not escaped the system it claims to escape. If
SatUSD's redemption rate forever depends on what a Coinbase or
a Binance reports, the legacy financial system retains a veto
over SatUSD's operation — a single point of political,
regulatory, and technical attack. Worse: a bitcoin economy that
still needs fiat-side institutions to know what things are worth
has not actually migrated the price-signal function. It has
outsourced it.

任何永久依赖外部价格 oracle 的稳定币，都没能逃离它声称要逃离的体
系。如果 SatUSD 的赎回汇率永远取决于某个 Coinbase 或 Binance 报出
的数字，传统金融体系就保留着对 SatUSD 运行的否决权——一个政治、
监管与技术上的单一攻击点。更糟的是：一个仍需法币侧机构来告知万物
价值的比特币经济，根本没有完成价格信号功能的迁移——它只是把这个
功能外包了。

Self-referencing — deriving the canonical price from SatUSD's
own on-chain economic activity, secured by Bitcoin's consensus —
is therefore not a technical optimization. **It is the
definition of success.** When the internal market becomes the
authoritative price source, the bitcoin economy is, for the
first time, generating its own signals. That is the mission,
achieved.

Self-referencing——让权威价格从 SatUSD 自身的链上经济活动中派生，
由比特币共识担保——因此不是一项技术优化。**它就是成功的定义。**
当内部市场成为权威价格源，比特币经济第一次在生成自己的信号。那一
刻，使命即告达成。

Terra's UST is the obvious objection, so let us be precise
about the difference. **UST's circularity was in its
collateral**: UST was backed by LUNA, and LUNA's value derived
from expected demand for UST. Redemption minted LUNA, diluting
the very backing it redeemed against — a reflexive loop with no
exogenous floor. **SatUSD's collateral is bitcoin** — an asset
whose value owes nothing to SatUSD's existence. Redemption
transfers bitcoin; it mints nothing and dilutes nothing. What is
self-referenced here is not value but **information** — the
price signal — and even that only after the internal market has
earned authority through years of cross-checked operation, with
external anchors demoted to sanity checks rather than
dependencies. A system whose information is self-generated but
whose value is exogenous does not have Terra's failure mode. It
has the failure modes of any collateralized system —
undercollateralization in a crash, thin-market manipulation
while young — which are known, bounded, and engineered against
in the technical documents.

Terra 的 UST 是显而易见的质疑，所以让我们把区别讲得精确。**UST 的
循环在抵押品上**：UST 由 LUNA 背书，而 LUNA 的价值来自对 UST 需求
的预期。赎回会增发 LUNA、稀释它所赎回的那个背书本身——一个没有外
生地板的反身回路。**SatUSD 的抵押品是比特币**——其价值与 SatUSD
存在与否毫无关系的资产。赎回只是转移比特币，不增发任何东西、不稀
释任何背书。这里自指的不是价值，而是**信息**——价格信号——而且
即便是信息，也要等内部市场经过多年交叉校验的运行赢得权威之后；外
部锚降级为完整性检查，而非依赖。一个信息自生成、价值却外生的系
统，不具有 Terra 的失败模式。它具有的是一切抵押系统共有的失败模式
——暴跌中的抵押不足、幼年期的薄市场操纵——已知、有界，并在技术
文档中逐项设防。

Every architectural choice in this project is to be evaluated by
one criterion: **does it move us closer to, or further from, the
state where the external dependency can be removed?**

本项目的每一个架构决策，都用同一个标准评估：**它让我们更接近、还
是更远离「外部依赖可以被移除」的那个状态？**

## How, in principle / 原则上如何实现

The mission constrains the mechanism. Four principles:

使命约束机制。四条原则：

1. **Everything verifiable by anyone.** Every claim the protocol
   makes — reserve, supply, lineage, price — must be checkable
   by client software against Bitcoin's chain, not asserted by
   any authority.
1. **一切皆可由任何人验证。**协议做出的每一项断言——储备、供给、
   lineage、价格——都必须能由客户端软件对照比特币链自行核验，而
   非由任何权威宣称。

2. **Trust is priced by a market, not chosen by a decree.**
   Redemption runs over an open standard of competing rails —
   different oracle designs, speeds, sizes, fees, trust
   profiles. Users pick; market share is the judgment. The
   self-referencing rail does not get switched on by governance;
   it wins when it offers the best terms.
2. **信任由市场定价，而非由法令选定。**赎回运行在一个开放标准的竞
   争通道之上——不同的 oracle 设计、速度、额度、费率、信任画像。
   用户自选，市场份额即裁决。self-referencing 通道不靠治理开关上
   位；它在给出最优条件时自然胜出。

3. **Liveness is bought, not assumed.** Wherever the design
   needs someone to act, it must suffice that *anyone* may act,
   paid by the protocol's own economics — never that a specific
   party must.
3. **活性是买来的，不是假设来的。**凡设计中需要有人动手之处，必须
   做到**任何人**都可以动手、且由协议自身的经济学付酬——绝不依赖
   某个特定主体必须动手。

4. **Three exits, one philosophy.** The founder exits — the
   protocol runs without its creators. The transitional controls
   exit — scaffolding is enumerated and demolished. The asset
   itself exits — Phase 4 is written above. Nothing in this
   project is meant to be permanent except the bitcoin
   underneath it.
4. **三层退场，同一哲学。**创始人退场——协议脱离创造者而运行；过
   渡性控制退场——脚手架被逐项列明并拆除；资产本身退场——阶段 4
   已写在上文。本项目中没有任何东西是为永久而设的，除了它底下的比
   特币。

## A standing invitation / 一封持续有效的邀请

This is not a wager. This is a bet on a future that is already
arriving.

这不是一场赌局。这是对一个已经在到来的未来的下注。

Bitcoin's hash rate compounds. Its supply schedule is set in
stone for the next century. The dollar has lost half its
purchasing power since the turn of the century, and the loss is
accelerating. Central bank digital currencies are being
prototyped in two dozen jurisdictions. Account freezes for
political reasons are no longer unthinkable in liberal
democracies. The world is sorting itself into people who can opt
out of monetary politics and people who cannot.

比特币的算力在复利增长，其供给计划在未来一个世纪都已刻在石头上。
美元的购买力自世纪之交以来已经减半，且贬值在加速。央行数字货币正
在二十多个司法辖区被原型化。在自由民主国家，出于政治原因的账户冻
结已不再是不可想象之事。世界正在分化为「可以选择退出货币政治的
人」和「不能的人」。

SatUSD exists to make the first category larger.

SatUSD 存在的目的，是让前一类人变多。

We do not seek venture capital. We do not seek regulatory
approval. We do not seek institutional endorsement. We seek the
attention of:

我们不寻求 venture capital。我们不寻求监管批准。我们不寻求机构背
书。我们寻求以下人群的关注：

- **Bitcoin developers** who recognize that the next decade of
  this technology is not about price-go-up but about building
  the financial infrastructure that lets people live inside it.
- **比特币开发者**——理解这门技术的下一个十年不在于币价上涨，而在
  于构建能让人真正生活于其中的金融基础设施。

- **Cryptographers** willing to engage the unsolved problems of
  Bitcoin-L1 oracle design, threshold signing, and
  trust-minimized settlement.
- **密码学家**——愿意投入比特币 L1 oracle 设计、阈值签名、可信最小
  化结算这些未解问题。

- **Users** who have personally felt the cost of fiat coercion
  and will use early, imperfect alternatives so the next
  generation has better ones.
- **用户**——亲身感受过法币胁迫的代价，愿意使用早期不完美的替代方
  案，让下一代拥有更好的选择。

- **Critics** who will tell us bluntly where the design is
  wrong. Hostile criticism well-articulated is worth more to
  this project than friendly endorsement.
- **批评者**——会直截了当告诉我们设计错在哪里的人。表达清晰的敌意
  批评，对本项目的价值高于友好的背书。

This project is an attempt. The attempt may fail. If it does, we
will have documented the failure in the open, and the next
attempt will start further down the road we cleared. If it
succeeds, the world will be measurably freer within our
lifetimes — not through revolution, not through politics, but
through the quiet substitution of better money for worse.

本项目是一次尝试。尝试可能失败。如果失败，我们将公开记录失败，下
一次尝试将从我们清出的路面上更远处出发。如果成功，这个世界将在我
们有生之年变得可测量地更自由——不靠革命，不靠政治，而是靠更好的
货币对糟糕货币悄无声息的替代。

Either way: what we build will be open, what we learn will be
shared, and what we believe is written down here.

无论结果如何：我们构建的将开放，我们学到的将共享，我们相信的，已
白纸黑字写在这里。

---

*This document defines the project's intent and is the highest
authority in this repository. Technical documents define
implementation. Any implementation choice that contradicts this
document must either be revised, or be explicitly justified as a
deliberate, temporary compromise on the way to it — enumerated,
with its removal criteria stated.*

*本文定义项目意图，是本仓库的最高权威。技术文档定义实现。任何与本
文矛盾的实现选择，必须被修订，或被显式论证为通往使命途中有意的、
暂时的妥协——逐项列明，并写明拆除条件。*

*The vision articulated here is intended to outlive any
individual contributor, including its original drafters.*

*此处阐述的愿景，意在超越任何单一贡献者——包括其最初的起草者——
而长存。*
